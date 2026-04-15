//! Lightweight HTTP CONNECT proxy for network allowlisting.
//!
//! Runs in the parent process with real network access. Jailed processes
//! reach it via a veth pair. DNS is resolved at connection time.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Maximum bytes for a single request line or header line.
const MAX_LINE_BYTES: usize = 8192;
/// Maximum number of header lines to consume before giving up.
const MAX_HEADER_LINES: usize = 64;

/// A parsed domain allowlist pattern. Pre-parsed at construction time
/// so `is_allowed` doesn't re-parse on every connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainPattern {
    /// Exact match (case-insensitive): "api.anthropic.com"
    Exact(String),
    /// Wildcard: "*.example.com" matches any subdomain and the base domain.
    /// Stores the lowercased base (e.g., "example.com") and suffix (".example.com").
    Wildcard { base: String, suffix: String },
}

impl DomainPattern {
    /// Parse a domain pattern string.
    pub fn parse(pattern: &str) -> Self {
        let lower = pattern.to_lowercase();
        if let Some(base) = lower.strip_prefix("*.") {
            DomainPattern::Wildcard {
                suffix: format!(".{}", base),
                base: base.to_string(),
            }
        } else {
            DomainPattern::Exact(lower)
        }
    }

    /// Check if a host matches this pattern.
    pub fn matches(&self, host: &str) -> bool {
        let host_lower = host.to_lowercase();
        match self {
            DomainPattern::Exact(domain) => host_lower == *domain,
            DomainPattern::Wildcard { base, suffix } => {
                host_lower.ends_with(suffix.as_str()) || host_lower == *base
            }
        }
    }
}

/// Proxy configuration.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Allowed domain patterns (pre-parsed).
    pub allowlist: Vec<DomainPattern>,
    /// Port to listen on (default: 8080).
    pub port: u16,
    /// IP address to bind to (default: localhost).
    pub bind_ip: std::net::IpAddr,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            allowlist: Vec::new(),
            port: 8080,
            bind_ip: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        }
    }
}

/// Run the proxy server.
///
/// Sends `Ok(())` on `ready` once bound, or `Err(msg)` on bind failure.
/// Stops when `shutdown` is dropped or receives a signal.
pub async fn run_proxy(
    config: ProxyConfig,
    ready: std::sync::mpsc::SyncSender<Result<(), String>>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()> {
    let addr = SocketAddr::new(config.bind_ip, config.port);
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => {
            let _ = ready.send(Ok(()));
            l
        }
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return Err(e);
        }
    };
    let allowlist = Arc::new(config.allowlist);
    let mut shutdown = shutdown;

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _) = result?;
                let allowlist = allowlist.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, &allowlist).await {
                        eprintln!("proxy error: {}", e);
                    }
                });
            }
            _ = shutdown.changed() => {
                return Ok(());
            }
        }
    }
}

/// Handle a single client connection.
async fn handle_connection(
    mut client: TcpStream,
    allowlist: &[DomainPattern],
) -> std::io::Result<()> {
    // Phase 1: parse CONNECT request and validate against allowlist.
    let target_stream = {
        let (reader, mut writer) = client.split();
        let mut reader = BufReader::new(reader);

        let mut request_line = String::with_capacity(256);
        read_line_bounded(&mut reader, &mut request_line, MAX_LINE_BYTES).await?;

        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 3 || parts[0] != "CONNECT" {
            writer.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await?;
            return Ok(());
        }

        let (host, port) = parse_host_port(parts[1])?;

        if !is_allowed(&host, allowlist) {
            writer.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
            return Ok(());
        }

        // Consume remaining headers.
        for _ in 0..MAX_HEADER_LINES {
            let mut line = String::new();
            read_line_bounded(&mut reader, &mut line, MAX_LINE_BYTES).await?;
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }

        // Connect to target (DNS resolved here).
        let addr = format!("{}:{}", host, port);
        let stream = match TcpStream::connect(&addr).await {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("HTTP/1.1 502 Bad Gateway\r\n\r\n{}", e);
                writer.write_all(msg.as_bytes()).await?;
                return Ok(());
            }
        };

        writer.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
        stream
    }; // reader + writer dropped here, releasing borrow on client

    // Phase 2: bidirectional tunnel.
    let (mut cr, mut cw) = client.split();
    let mut target_stream = target_stream;
    let (mut tr, mut tw) = target_stream.split();
    tokio::select! {
        _ = tokio::io::copy(&mut cr, &mut tw) => {}
        _ = tokio::io::copy(&mut tr, &mut cw) => {}
    }
    Ok(())
}

/// Read a line from a buffered reader, rejecting lines over `max_bytes`.
async fn read_line_bounded<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut String,
    max_bytes: usize,
) -> std::io::Result<()> {
    let n = reader.read_line(buf).await?;
    if n > max_bytes {
        buf.clear();
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "line too long",
        ));
    }
    Ok(())
}

/// Parse "host:port" string.
fn parse_host_port(s: &str) -> std::io::Result<(String, u16)> {
    if let Some((host, port_str)) = s.rsplit_once(':') {
        let port = port_str.parse().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid port")
        })?;
        Ok((host.to_string(), port))
    } else {
        // Default to 443 for HTTPS
        Ok((s.to_string(), 443))
    }
}

/// Check if host matches any pattern in allowlist.
fn is_allowed(host: &str, allowlist: &[DomainPattern]) -> bool {
    allowlist.iter().any(|p| p.matches(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_list(patterns: &[&str]) -> Vec<DomainPattern> {
        patterns.iter().map(|p| DomainPattern::parse(p)).collect()
    }

    #[test]
    fn test_is_allowed_empty_blocks_everything() {
        let allowlist = parse_list(&[]);
        assert!(!is_allowed("api.anthropic.com", &allowlist));
        assert!(!is_allowed("google.com", &allowlist));
        assert!(!is_allowed("localhost", &allowlist));
        assert!(!is_allowed("127.0.0.1", &allowlist));
    }

    #[test]
    fn test_is_allowed_exact() {
        let allowlist = parse_list(&["api.anthropic.com"]);
        assert!(is_allowed("api.anthropic.com", &allowlist));
        assert!(is_allowed("API.ANTHROPIC.COM", &allowlist));
        assert!(!is_allowed("evil.com", &allowlist));
        assert!(!is_allowed("anthropic.com", &allowlist));
        assert!(!is_allowed("sub.api.anthropic.com", &allowlist));
    }

    #[test]
    fn test_is_allowed_wildcard() {
        let allowlist = parse_list(&["*.openai.com"]);
        assert!(is_allowed("api.openai.com", &allowlist));
        assert!(is_allowed("chat.openai.com", &allowlist));
        assert!(is_allowed("openai.com", &allowlist));
        assert!(!is_allowed("openai.com.evil.com", &allowlist));
        assert!(!is_allowed("notopenai.com", &allowlist));
    }

    #[test]
    fn test_domain_pattern_parse() {
        assert_eq!(
            DomainPattern::parse("api.example.com"),
            DomainPattern::Exact("api.example.com".into())
        );
        assert!(matches!(
            DomainPattern::parse("*.example.com"),
            DomainPattern::Wildcard { .. }
        ));
    }

    #[test]
    fn test_parse_host_port() {
        let (h, p) = parse_host_port("example.com:443").unwrap();
        assert_eq!(h, "example.com");
        assert_eq!(p, 443);

        let (h, p) = parse_host_port("example.com:8080").unwrap();
        assert_eq!(h, "example.com");
        assert_eq!(p, 8080);

        let (h, p) = parse_host_port("example.com").unwrap();
        assert_eq!(h, "example.com");
        assert_eq!(p, 443);
    }

    #[tokio::test]
    async fn test_read_line_bounded_rejects_oversize() {
        let long_line = format!("{}\n", "A".repeat(10_000));
        let mut cursor = std::io::Cursor::new(long_line.into_bytes());
        let mut reader = tokio::io::BufReader::new(&mut cursor);
        let mut buf = String::new();

        let result = read_line_bounded(&mut reader, &mut buf, 8192).await;
        assert!(result.is_err(), "Should reject lines over MAX_LINE_BYTES");
    }

    #[tokio::test]
    async fn test_read_line_bounded_accepts_normal() {
        let line = "CONNECT example.com:443 HTTP/1.1\r\n";
        let mut cursor = std::io::Cursor::new(line.as_bytes().to_vec());
        let mut reader = tokio::io::BufReader::new(&mut cursor);
        let mut buf = String::new();

        let result = read_line_bounded(&mut reader, &mut buf, 8192).await;
        assert!(result.is_ok(), "Should accept normal-sized lines");
        assert!(buf.contains("CONNECT"));
    }

    /// Helper: send a CONNECT request through the proxy and return the response status line.
    async fn proxy_connect(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .expect("connect to proxy");
        let request = format!("CONNECT {} HTTP/1.1\r\nHost: {}\r\n\r\n", target, target);
        stream
            .write_all(request.as_bytes())
            .await
            .expect("write request");
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .await
            .expect("read response");
        response
    }

    #[tokio::test]
    async fn test_proxy_blocks_disallowed_domain() {
        // Bind to a random port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let allowlist = Arc::new(parse_list(&["allowed.example.com"]));

        // Spawn proxy handler for one connection
        let al = allowlist.clone();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, &al).await.unwrap();
        });

        let response = proxy_connect(port, "evil.com:443").await;
        assert!(
            response.contains("403"),
            "Disallowed domain should get 403, got: {}",
            response.trim()
        );
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_proxy_blocks_everything_when_empty_allowlist() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let allowlist: Arc<Vec<DomainPattern>> = Arc::new(vec![]);

        let al = allowlist.clone();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, &al).await.unwrap();
        });

        let response = proxy_connect(port, "google.com:443").await;
        assert!(
            response.contains("403"),
            "Empty allowlist should block everything, got: {}",
            response.trim()
        );
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_proxy_rejects_non_connect_method() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let allowlist = Arc::new(parse_list(&["anything.com"]));

        let al = allowlist.clone();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, &al).await.unwrap();
        });

        // Send a GET instead of CONNECT
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap();
        stream
            .write_all(b"GET http://anything.com/ HTTP/1.1\r\nHost: anything.com\r\n\r\n")
            .await
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        assert!(
            response.contains("400"),
            "Non-CONNECT should get 400, got: {}",
            response.trim()
        );
        let _ = handle.await;
    }
}
