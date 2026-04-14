//! TUI rendering — modern, borderless, information-dense.

use crate::app::{App, JailInfo, JailStatus, Stream, View, format_bytes, format_duration};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Padding, Paragraph, Wrap},
};

// Restrained palette — most of the UI is white/gray, color only for status.
const ACCENT: Color = Color::Rgb(99, 102, 241);     // Indigo — brand, selection
const GREEN: Color = Color::Rgb(34, 197, 94);        // Running / healthy
const RED: Color = Color::Rgb(248, 113, 113);         // Failed / error (softer red)
const AMBER: Color = Color::Rgb(251, 191, 36);       // Timeout / warning
const CYAN: Color = Color::Rgb(34, 211, 238);         // Completed / info
const DIM: Color = Color::Rgb(82, 82, 91);            // Labels, secondary
const FAINT: Color = Color::Rgb(52, 52, 59);          // Separators, empty bars
const SURFACE: Color = Color::Rgb(30, 30, 36);        // Subtle backgrounds
const BG: Color = Color::Rgb(17, 17, 21);             // Main background

pub fn render(f: &mut Frame, app: &App) {
    f.render_widget(Block::default().style(Style::default().bg(BG)), f.size());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2), // Header
            Constraint::Length(1), // Separator
            Constraint::Min(0),    // Main
            Constraint::Length(1), // Footer
        ])
        .split(f.size());

    render_header(f, app, chunks[0]);
    // Thin separator line
    f.render_widget(
        Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(FAINT)),
        chunks[1],
    );
    match app.view {
        View::List => render_list(f, app, chunks[2]),
        View::Detail => render_detail(f, app, chunks[2]),
    }
    render_footer(f, app, chunks[3]);
}

// ---------------------------------------------------------------------------
// Header — clean, minimal
// ---------------------------------------------------------------------------

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let (mut running, mut failed, mut done) = (0u32, 0u32, 0u32);
    for j in app.jails.values() {
        match j.status {
            JailStatus::Running => running += 1,
            JailStatus::Completed(0) => done += 1,
            _ => failed += 1,
        }
    }

    let mut spans = vec![
        Span::styled("  agentjail", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("  ", Style::default()),
    ];

    let parts: Vec<(u32, &str, Color)> = vec![
        (running, "running", GREEN),
        (failed, "failed", RED),
        (done, "done", CYAN),
    ];
    for (count, label, color) in &parts {
        if *count > 0 {
            spans.push(Span::styled(format!("{}", count), Style::default().fg(*color)));
            spans.push(Span::styled(format!(" {}  ", label), Style::default().fg(DIM)));
        }
    }
    if running == 0 && failed == 0 && done == 0 {
        spans.push(Span::styled("waiting for jails", Style::default().fg(DIM)));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---------------------------------------------------------------------------
// Footer — subtle key hints
// ---------------------------------------------------------------------------

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let keys: &[(&str, &str)] = match app.view {
        View::List => &[("j/k", "nav"), ("enter", "open"), ("K", "kill"), ("C", "clear"), ("q", "quit")],
        View::Detail => &[("j/k", "scroll"), ("G", "end"), ("esc", "back"), ("K", "kill"), ("q", "quit")],
    };

    let spans: Vec<Span> = keys.iter().flat_map(|(key, action)| {
        vec![
            Span::styled(format!(" {} ", key), Style::default().fg(SURFACE).bg(DIM)),
            Span::styled(format!(" {}  ", action), Style::default().fg(DIM)),
        ]
    }).collect();

    f.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}

// ---------------------------------------------------------------------------
// List view — each row is a mini dashboard
// ---------------------------------------------------------------------------

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    let ids = app.sorted_ids();

    if ids.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "No sandboxes running",
                Style::default().fg(DIM),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "agentjail run -s ./src -o ./out <cmd>",
                Style::default().fg(FAINT),
            )),
        ])
        .alignment(Alignment::Center);
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = ids.iter().map(|&id| {
        let j = &app.jails[&id];
        let sel = app.selected == Some(id);
        let (icon, color) = status_icon(&j.status);
        let elapsed = format_duration(j.started_at.elapsed());

        // Row: [sel] [icon] command                preset  net/sec  memory  time
        let cmd_style = if sel {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(212, 212, 216)) // Slightly off-white
        };

        let mut spans = vec![
            // Selection + status
            Span::styled(
                if sel { "  ▸ " } else { "    " },
                Style::default().fg(ACCENT),
            ),
            Span::styled(format!("{}  ", icon), Style::default().fg(color)),
            // Command — primary content
            Span::styled(format!("{:<22}", truncate(&j.command, 22)), cmd_style),
            // Preset pill
            Span::styled(
                format!(" {} ", j.preset),
                Style::default().fg(Color::Rgb(200, 200, 206)).bg(FAINT),
            ),
            Span::styled("  ", Style::default()),
            // Security: network + seccomp as compact text
            Span::styled(net_short(&j.network), Style::default().fg(net_color(&j.network))),
            Span::styled(" ", Style::default()),
            Span::styled(sec_short(&j.seccomp), Style::default().fg(sec_color(&j.seccomp))),
        ];

        // Memory bar — only if limit set and there's usage
        if j.memory_limit_mb > 0 {
            let ratio = (j.memory_bytes as f64) / (j.memory_limit_mb as f64 * 1024.0 * 1024.0);
            spans.push(Span::styled("  ", Style::default()));
            spans.extend(mini_bar(ratio, 8));
        }

        // Duration — right side
        spans.push(Span::styled(format!("  {:>6}", elapsed), Style::default().fg(DIM)));

        let bg = if sel { Style::default().bg(SURFACE) } else { Style::default() };
        ListItem::new(Line::from(spans)).style(bg)
    }).collect();

    f.render_widget(
        List::new(items).block(
            Block::default().padding(Padding::new(0, 0, 1, 0)), // top padding only
        ),
        area,
    );
}

/// Compact memory bar using thin block characters.
fn mini_bar(ratio: f64, width: usize) -> Vec<Span<'static>> {
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = (ratio * width as f64).round() as usize;
    let empty = width - filled;
    let color = if ratio > 0.85 { RED } else if ratio > 0.6 { AMBER } else { GREEN };

    vec![
        Span::styled("▐".repeat(filled), Style::default().fg(color)),
        Span::styled("▐".repeat(empty), Style::default().fg(FAINT)),
    ]
}

// ---------------------------------------------------------------------------
// Detail view — borderless info section + bordered output
// ---------------------------------------------------------------------------

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let Some(jail) = app.selected_jail() else {
        f.render_widget(
            Paragraph::new(Span::styled("No jail selected", Style::default().fg(DIM)))
                .alignment(Alignment::Center),
            area,
        );
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // Info
            Constraint::Length(1),  // Separator
            Constraint::Min(0),     // Output
        ])
        .split(area);

    render_info(f, jail, chunks[0]);
    f.render_widget(
        Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(FAINT)),
        chunks[1],
    );
    render_output(f, jail, app.scroll_offset, chunks[2]);
}

fn render_info(f: &mut Frame, jail: &JailInfo, area: Rect) {
    let (icon, color, status_text) = match jail.status {
        JailStatus::Running => ("●", GREEN, "Running"),
        JailStatus::Completed(0) => ("✓", CYAN, "Completed"),
        JailStatus::Completed(_) => ("✗", RED, "Failed"),
        JailStatus::TimedOut => ("⏱", AMBER, "Timed Out"),
        JailStatus::Killed => ("■", RED, "Killed"),
    };

    let exit_str = match jail.status {
        JailStatus::Completed(c) if c != 0 => format!(" exit {}", c),
        _ => String::new(),
    };

    let elapsed = jail.started_at.elapsed();
    let w = Style::default().fg(Color::White); // White for values
    let d = Style::default().fg(DIM);           // Dim for labels
    let pad = "    ";

    let mut lines = vec![
        Line::from(""),
        // Command — hero text
        Line::from(vec![
            Span::styled(pad, d),
            Span::styled(&jail.command, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("  ", d),
            Span::styled(format!(" {} ", jail.preset), Style::default().fg(Color::Rgb(200, 200, 206)).bg(FAINT)),
        ]),
        Line::from(""),
        // Status + PID
        Line::from(vec![
            Span::styled(pad, d),
            Span::styled(format!("{} {}", icon, status_text), Style::default().fg(color)),
            Span::styled(&exit_str, d),
            Span::styled("    pid ", d),
            Span::styled(format!("{}", jail.pid), w),
            Span::styled("    elapsed ", d),
            Span::styled(format_duration(elapsed), w),
            Span::styled("    mem ", d),
            Span::styled(format_bytes(jail.memory_bytes), w),
        ]),
        // Security row
        Line::from(vec![
            Span::styled(pad, d),
            Span::styled("net ", d),
            Span::styled(&jail.network, Style::default().fg(net_color(&jail.network))),
            Span::styled("    seccomp ", d),
            Span::styled(&jail.seccomp, Style::default().fg(sec_color(&jail.seccomp))),
        ]),
    ];

    // Timeout progress — inline text bar
    if jail.timeout_secs > 0 {
        let secs = elapsed.as_secs();
        let ratio = (secs as f64 / jail.timeout_secs as f64).clamp(0.0, 1.0);
        let bar_w = 20;
        let filled = (ratio * bar_w as f64).round() as usize;
        let empty = bar_w - filled;
        let bar_color = if ratio > 0.9 { RED } else if ratio > 0.75 { AMBER } else { GREEN };

        lines.push(Line::from(vec![
            Span::styled(pad, d),
            Span::styled("timeout ", d),
            Span::styled("━".repeat(filled), Style::default().fg(bar_color)),
            Span::styled("━".repeat(empty), Style::default().fg(FAINT)),
            Span::styled(format!(" {}s/{}s", secs, jail.timeout_secs), d),
        ]));
    }

    f.render_widget(Paragraph::new(lines), area);
}

fn render_output(f: &mut Frame, jail: &JailInfo, scroll_offset: usize, area: Rect) {
    let total = jail.output.len();
    let visible = area.height.saturating_sub(2) as usize;

    let end = (scroll_offset + 1).min(total);
    let start = end.saturating_sub(visible);

    let lines: Vec<Line> = jail.output.iter()
        .skip(start).take(end - start)
        .map(|(stream, text)| {
            let (prefix, color) = match stream {
                Stream::Stdout => ("", Color::Rgb(190, 190, 196)),
                Stream::Stderr => ("", RED),
            };
            Line::from(Span::styled(
                format!("{}{}", prefix, text),
                Style::default().fg(color),
            ))
        })
        .collect();

    let mut title_spans = vec![
        Span::styled(" output ", Style::default().fg(Color::White)),
    ];
    if total > 0 {
        title_spans.push(Span::styled(
            format!("{} ", total),
            Style::default().fg(DIM),
        ));
    }

    let block = Block::default()
        .title(Line::from(title_spans))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(FAINT))
        .padding(Padding::horizontal(1));

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(block),
        area,
    );
}

// ---------------------------------------------------------------------------
// Helpers — compact, consistent
// ---------------------------------------------------------------------------

fn status_icon(status: &JailStatus) -> (&'static str, Color) {
    match status {
        JailStatus::Running => ("●", GREEN),
        JailStatus::Completed(0) => ("✓", CYAN),
        JailStatus::Completed(_) => ("✗", RED),
        JailStatus::TimedOut => ("⏱", AMBER),
        JailStatus::Killed => ("■", RED),
    }
}

fn net_short(network: &str) -> &'static str {
    match network {
        "allowlist" => "⇄",
        "loopback" => "↻",
        _ => "◌",
    }
}

fn net_color(network: &str) -> Color {
    match network {
        "allowlist" => CYAN,
        "loopback" => DIM,
        _ => FAINT,
    }
}

fn sec_short(seccomp: &str) -> &'static str {
    match seccomp {
        "strict" => "◆◆",
        "standard" => "◆",
        _ => "○",
    }
}

fn sec_color(seccomp: &str) -> Color {
    match seccomp {
        "strict" => AMBER,
        "standard" => GREEN,
        _ => FAINT,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
