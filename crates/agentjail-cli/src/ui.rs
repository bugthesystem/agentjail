//! TUI rendering.

use crate::app::{App, JailInfo, JailStatus, Stream, View, format_bytes, format_duration};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Padding, Paragraph, Wrap},
};

// Palette: mostly monochrome, color = status only.
const ACCENT: Color = Color::Rgb(129, 140, 248);     // Indigo-400
const GREEN: Color = Color::Rgb(74, 222, 128);        // Green-400
const RED: Color = Color::Rgb(252, 129, 129);          // Red-400
const AMBER: Color = Color::Rgb(252, 211, 77);        // Amber-300
const CYAN: Color = Color::Rgb(103, 232, 249);         // Cyan-300
const TEXT: Color = Color::Rgb(228, 228, 231);         // Zinc-200
const DIM: Color = Color::Rgb(113, 113, 122);          // Zinc-500
const FAINT: Color = Color::Rgb(63, 63, 70);           // Zinc-700
const BG: Color = Color::Rgb(9, 9, 11);               // Zinc-950

pub fn render(f: &mut Frame, app: &App) {
    f.render_widget(Block::default().style(Style::default().bg(BG)), f.size());

    let area = centered(f.size(), 100); // Cap width for readability

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Main
            Constraint::Length(2), // Footer
        ])
        .split(area);

    render_header(f, app, chunks[0]);
    match app.view {
        View::List => render_list(f, app, chunks[1]),
        View::Detail => render_detail(f, app, chunks[1]),
    }
    render_footer(f, app, chunks[2]);
}

/// Horizontally center content up to max_width.
fn centered(area: Rect, max_width: u16) -> Rect {
    if area.width <= max_width {
        return area;
    }
    let pad = (area.width - max_width) / 2;
    Rect::new(area.x + pad, area.y, max_width, area.height)
}

// ---------------------------------------------------------------------------
// Header
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
        Span::styled(" ◉ ", Style::default().fg(ACCENT)),
        Span::styled("agentjail ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
    ];

    for (n, label, color) in [(running, "running", GREEN), (failed, "failed", RED), (done, "done", CYAN)] {
        if n > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(FAINT)));
            spans.push(Span::styled(format!("{n}"), Style::default().fg(color).add_modifier(Modifier::BOLD)));
            spans.push(Span::styled(format!(" {label}"), Style::default().fg(DIM)));
        }
    }

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(FAINT))
                .padding(Padding::new(1, 1, 1, 0)),
        ),
        area,
    );
}

// ---------------------------------------------------------------------------
// Footer
// ---------------------------------------------------------------------------

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let keys: &[(&str, &str)] = match app.view {
        View::List => &[("j/k", "navigate"), ("enter", "inspect"), ("K", "kill"), ("C", "clear"), ("q", "quit")],
        View::Detail => &[("j/k", "scroll"), ("G", "bottom"), ("esc", "back"), ("K", "kill"), ("q", "quit")],
    };

    let spans: Vec<Span> = keys.iter().flat_map(|(k, a)| vec![
        Span::styled(format!(" {k} "), Style::default().fg(Color::Rgb(24, 24, 27)).bg(DIM)),
        Span::styled(format!(" {a}  "), Style::default().fg(FAINT)),
    ]).collect();

    f.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}

// ---------------------------------------------------------------------------
// List view — clean rows, generous spacing
// ---------------------------------------------------------------------------

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    let ids = app.sorted_ids();

    if ids.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(""),
                Line::from(""),
                Line::from(Span::styled("No sandboxes", Style::default().fg(DIM))),
                Line::from(""),
                Line::from(Span::styled(
                    "agentjail run -s ./src -o ./out <command>",
                    Style::default().fg(FAINT),
                )),
            ])
            .alignment(Alignment::Center),
            area,
        );
        return;
    }

    // Two-line items: main row + subtitle row
    let items: Vec<ListItem> = ids.iter().map(|&id| {
        let j = &app.jails[&id];
        let sel = app.selected == Some(id);
        let (icon, color) = status_icon(&j.status);
        let elapsed = format_duration(j.started_at.elapsed());

        let cmd_style = if sel {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT)
        };

        // Line 1: status icon + command + elapsed
        let line1 = Line::from(vec![
            Span::styled(if sel { "  ▸ " } else { "    " }, Style::default().fg(ACCENT)),
            Span::styled(format!("{icon} "), Style::default().fg(color)),
            Span::styled(format!("{:<30}", truncate(&j.command, 30)), cmd_style),
            Span::styled(elapsed, Style::default().fg(DIM)),
        ]);

        // Line 2: subtitle with metadata
        let mut meta = vec![
            Span::styled("      ", Style::default()), // indent to align with command
            Span::styled(&j.preset, Style::default().fg(FAINT)),
        ];
        if j.network != "none" && !j.network.is_empty() {
            meta.push(Span::styled(format!("  net:{}", j.network), Style::default().fg(FAINT)));
        }
        if j.seccomp != "disabled" && !j.seccomp.is_empty() {
            meta.push(Span::styled(format!("  sec:{}", j.seccomp), Style::default().fg(FAINT)));
        }
        if j.memory_limit_mb > 0 {
            meta.push(Span::styled(
                format!("  mem:{}/{} MB", j.memory_bytes / (1024 * 1024), j.memory_limit_mb),
                Style::default().fg(FAINT),
            ));
        }
        let line2 = Line::from(meta);

        let bg = if sel { Style::default().bg(Color::Rgb(24, 24, 30)) } else { Style::default() };
        ListItem::new(vec![line1, line2, Line::from("")]).style(bg) // blank line = row spacing
    }).collect();

    f.render_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 1, 0))),
        area,
    );
}

// ---------------------------------------------------------------------------
// Detail view
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
            Constraint::Length(9), // Info
            Constraint::Min(0),    // Output
        ])
        .split(area);

    render_info(f, jail, chunks[0]);
    render_output(f, jail, app.scroll_offset, chunks[1]);
}

fn render_info(f: &mut Frame, jail: &JailInfo, area: Rect) {
    let (icon, color, label) = status_label(&jail.status);
    let elapsed = jail.started_at.elapsed();
    let d = Style::default().fg(DIM);
    let w = Style::default().fg(TEXT);

    let exit_str = match jail.status {
        JailStatus::Completed(c) if c != 0 => format!("  exit {c}"),
        _ => String::new(),
    };

    let mut lines = vec![
        Line::from(""),
        // Command — hero
        Line::from(vec![
            Span::styled("  ", d),
            Span::styled(&jail.command, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        // Status line
        Line::from(vec![
            Span::styled("  ", d),
            Span::styled(format!("{icon} {label}"), Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(&exit_str, d),
        ]),
        // Metrics
        Line::from(vec![
            Span::styled("  pid ", d), Span::styled(format!("{}", jail.pid), w),
            Span::styled("  ·  elapsed ", d), Span::styled(format_duration(elapsed), w),
            Span::styled("  ·  mem ", d), Span::styled(format_bytes(jail.memory_bytes), w),
        ]),
        // Security
        Line::from(vec![
            Span::styled("  network ", d), Span::styled(&jail.network, w),
            Span::styled("  ·  seccomp ", d), Span::styled(&jail.seccomp, w),
            Span::styled("  ·  preset ", d), Span::styled(&jail.preset, w),
        ]),
    ];

    // Timeout bar
    if jail.timeout_secs > 0 {
        let secs = elapsed.as_secs();
        let ratio = (secs as f64 / jail.timeout_secs as f64).clamp(0.0, 1.0);
        let bar_color = if ratio > 0.9 { RED } else if ratio > 0.75 { AMBER } else { GREEN };
        let w = 30;
        let filled = (ratio * w as f64).round() as usize;
        lines.push(Line::from(vec![
            Span::styled("  timeout ", d),
            Span::styled("━".repeat(filled), Style::default().fg(bar_color)),
            Span::styled("━".repeat(w - filled), Style::default().fg(FAINT)),
            Span::styled(format!(" {secs}s/{t}s", t = jail.timeout_secs), d),
        ]));
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(FAINT)),
        ),
        area,
    );
}

fn render_output(f: &mut Frame, jail: &JailInfo, scroll: usize, area: Rect) {
    let total = jail.output.len();
    let vis = area.height.saturating_sub(2) as usize;
    let end = (scroll + 1).min(total);
    let start = end.saturating_sub(vis);

    let lines: Vec<Line> = jail.output.iter()
        .skip(start).take(end - start)
        .map(|(stream, text)| {
            let color = match stream {
                Stream::Stdout => Color::Rgb(161, 161, 170), // Zinc-400
                Stream::Stderr => RED,
            };
            Line::from(Span::styled(text.as_str(), Style::default().fg(color)))
        })
        .collect();

    let mut title = vec![Span::styled(" output ", Style::default().fg(TEXT))];
    if total > 0 {
        title.push(Span::styled(format!("({total}) "), Style::default().fg(FAINT)));
    }

    let block = Block::default()
        .title(Line::from(title))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(FAINT))
        .padding(Padding::horizontal(1));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn status_icon(s: &JailStatus) -> (&'static str, Color) {
    match s {
        JailStatus::Running => ("●", GREEN),
        JailStatus::Completed(0) => ("✓", CYAN),
        JailStatus::Completed(_) => ("✗", RED),
        JailStatus::TimedOut => ("⏱", AMBER),
        JailStatus::Killed => ("■", RED),
    }
}

fn status_label(s: &JailStatus) -> (&'static str, Color, &'static str) {
    match s {
        JailStatus::Running => ("●", GREEN, "Running"),
        JailStatus::Completed(0) => ("✓", CYAN, "Completed"),
        JailStatus::Completed(_) => ("✗", RED, "Failed"),
        JailStatus::TimedOut => ("⏱", AMBER, "Timed Out"),
        JailStatus::Killed => ("■", RED, "Killed"),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}…", &s[..max - 1]) }
}
