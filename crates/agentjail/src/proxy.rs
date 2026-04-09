//! Lightweight HTTP CONNECT proxy for network allowlisting.
//!
//! Runs inside the jail's network namespace, allowing only connections
//! to whitelisted domains. DNS is resolved at connection time.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Maximum bytes for a single request line or header line.
const MAX_LINE_BYTES: usize = 8192;
/// Maximum number of header lines to consume before giving up.
const MAX_HEADER_LINES: usize = 64;

/// Proxy configuration.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Allowed domain patterns (e.g., "api.anthropic.com", "*.openai.com").
    pub allowlist: Vec<String>,
    /// Port to listen on (default: 8080).
    pub port: u16,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            allowlist: Vec::new(),
            port: 8080,
        }
    }
}

/// Run the proxy server.
///
/// Sends `Ok(())` on `ready` once bound, or `Err(msg)` on bind failure.
pub async fn run_proxy(
    config: ProxyConfig,
    ready: std::sync::mpsc::SyncSender<Result<(), String>>,
) -> std::io::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
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

    loop {
        let (stream, _) = listener.accept().await?;
        let allowlist = allowlist.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, &allowlist).await {
                eprintln!("proxy error: {}", e);
            }
        });
    }
}

/// Handle a single client connection.
async fn handle_connection(
    mut client: TcpStream,
    allowlist: &[String],
) -> std::io::Result<()> {
    let (reader, mut writer) = client.split();
    let mut reader = BufReader::new(reader);

    // Read the request line (bounded to prevent memory exhaustion).
    let mut request_line = String::with_capacity(256);
    read_line_bounded(&mut reader, &mut request_line, MAX_LINE_BYTES).await?;

    // Parse CONNECT request: "CONNECT host:port HTTP/1.1"
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 3 || parts[0] != "CONNECT" {
        writer.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await?;
        return Ok(());
    }

    let target = parts[1];
    let (host, port) = parse_host_port(target)?;

    // Check allowlist
    if !is_allowed(&host, allowlist) {
        writer.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
        return Ok(());
    }

    // Consume remaining headers (bounded count and size).
    for _ in 0..MAX_HEADER_LINES {
        let mut line = String::new();
        read_line_bounded(&mut reader, &mut line, MAX_LINE_BYTES).await?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
    }

    // Connect to target (DNS resolved here)
    let target_addr = format!("{}:{}", host, port);
    let target_stream = match TcpStream::connect(&target_addr).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("HTTP/1.1 502 Bad Gateway\r\n\r\n{}", e);
            writer.write_all(msg.as_bytes()).await?;
            return Ok(());
        }
    };

    // Send success response
    writer.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;

    // Reunite the split client stream
    drop(reader);
    drop(writer);

    // Tunnel bidirectionally
    let mut client = client;
    let mut target = target_stream;
    let (mut client_read, mut client_write) = client.split();
    let (mut target_read, mut target_write) = target.split();

    let client_to_target = tokio::io::copy(&mut client_read, &mut target_write);
    let target_to_client = tokio::io::copy(&mut target_read, &mut client_write);

    tokio::select! {
        _ = client_to_target => {}
        _ = target_to_client => {}
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
fn is_allowed(host: &str, allowlist: &[String]) -> bool {
    let host_lower = host.to_lowercase();

    for pattern in allowlist {
        let pattern_lower = pattern.to_lowercase();

        if pattern_lower.starts_with("*.") {
            // Wildcard: *.example.com matches foo.example.com
            let suffix = &pattern_lower[1..]; // ".example.com"
            if host_lower.ends_with(suffix) || host_lower == pattern_lower[2..] {
                return true;
            }
        } else if host_lower == pattern_lower {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_allowed_exact() {
        let allowlist = vec!["api.anthropic.com".into()];
        assert!(is_allowed("api.anthropic.com", &allowlist));
        assert!(is_allowed("API.ANTHROPIC.COM", &allowlist));
        assert!(!is_allowed("evil.com", &allowlist));
    }

    #[test]
    fn test_is_allowed_wildcard() {
        let allowlist = vec!["*.openai.com".into()];
        assert!(is_allowed("api.openai.com", &allowlist));
        assert!(is_allowed("chat.openai.com", &allowlist));
        assert!(is_allowed("openai.com", &allowlist));
        assert!(!is_allowed("openai.com.evil.com", &allowlist));
        assert!(!is_allowed("notopenai.com", &allowlist));
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
}
