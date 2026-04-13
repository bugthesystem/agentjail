//! Modern UI rendering for the TUI.

use crate::app::{App, JailInfo, JailStatus, Stream, View, format_bytes, format_duration};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, LineGauge, List, ListItem, Padding, Paragraph, Wrap},
};

// Modern color palette
const PRIMARY: Color = Color::Rgb(139, 92, 246);    // Violet
const SUCCESS: Color = Color::Rgb(34, 197, 94);     // Green
const ERROR: Color = Color::Rgb(239, 68, 68);       // Red
const WARNING: Color = Color::Rgb(250, 204, 21);    // Yellow
const INFO: Color = Color::Rgb(56, 189, 248);       // Sky blue
const MUTED: Color = Color::Rgb(113, 113, 122);     // Zinc
const SURFACE: Color = Color::Rgb(39, 39, 42);      // Dark surface
const BG: Color = Color::Rgb(24, 24, 27);           // Background

pub fn render(f: &mut Frame, app: &App) {
    f.render_widget(Block::default().style(Style::default().bg(BG)), f.size());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(f.size());

    render_header(f, app, chunks[0]);
    match app.view {
        View::List => render_list(f, app, chunks[1]),
        View::Detail => render_detail(f, app, chunks[1]),
    }
    render_footer(f, app, chunks[2]);
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let mut running = 0u32;
    let mut failed = 0u32;
    let mut done = 0u32;
    for j in app.jails.values() {
        match j.status {
            JailStatus::Running => running += 1,
            JailStatus::Completed(0) => done += 1,
            JailStatus::Completed(_) | JailStatus::Killed => failed += 1,
            JailStatus::TimedOut => failed += 1,
        }
    }

    let mut spans = vec![
        Span::styled("◉ ", Style::default().fg(PRIMARY)),
        Span::styled("agentjail", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("  │  ", Style::default().fg(MUTED)),
    ];

    if running > 0 {
        spans.push(Span::styled(format!("{} running", running), Style::default().fg(SUCCESS)));
        spans.push(Span::styled("  ", Style::default()));
    }
    if failed > 0 {
        spans.push(Span::styled(format!("{} failed", failed), Style::default().fg(ERROR)));
        spans.push(Span::styled("  ", Style::default()));
    }
    if done > 0 {
        spans.push(Span::styled(format!("{} done", done), Style::default().fg(INFO)));
    }
    if running == 0 && failed == 0 && done == 0 {
        spans.push(Span::styled("no jails", Style::default().fg(MUTED)));
    }

    let header = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(SURFACE))
                .padding(Padding::horizontal(1)),
        );
    f.render_widget(header, area);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let keys = match app.view {
        View::List => vec![
            ("↑↓", "navigate"), ("⏎", "details"), ("K", "kill"), ("C", "clear"), ("q", "quit"),
        ],
        View::Detail => vec![
            ("↑↓", "scroll"), ("G", "end"), ("esc", "back"), ("K", "kill"), ("q", "quit"),
        ],
    };

    let spans: Vec<Span> = keys.iter().enumerate().flat_map(|(i, (key, action))| {
        let mut v = vec![
            Span::styled(format!(" {} ", key), Style::default().fg(BG).bg(MUTED)),
            Span::styled(format!(" {} ", action), Style::default().fg(MUTED)),
        ];
        if i < keys.len() - 1 {
            v.push(Span::styled(" ", Style::default()));
        }
        v
    }).collect();

    f.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}

// ---------------------------------------------------------------------------
// List view
// ---------------------------------------------------------------------------

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    let ids = app.sorted_ids();

    if ids.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("◇", Style::default().fg(MUTED))),
            Line::from(""),
            Line::from(Span::styled("No jails running", Style::default().fg(MUTED))),
            Line::from(Span::styled(
                "Run `agentjail run` to start a sandbox",
                Style::default().fg(MUTED).add_modifier(Modifier::DIM),
            )),
        ])
        .alignment(Alignment::Center)
        .block(rounded_block(""));
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = ids.iter().map(|&id| {
        let jail = &app.jails[&id];
        let sel = app.selected == Some(id);
        let (icon, color) = status_style(&jail.status);
        let dur = format_duration(jail.started_at.elapsed());

        let mut spans = vec![
            Span::styled(if sel { " ▸ " } else { "   " }, Style::default().fg(PRIMARY)),
            Span::styled(format!("{} ", icon), Style::default().fg(color)),
            Span::styled(
                format!("{:<24}", truncate(&jail.command, 24)),
                if sel {
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
            Span::styled(
                format!(" {} ", jail.preset),
                Style::default().fg(BG).bg(preset_color(&jail.preset)),
            ),
            Span::styled(
                format!(" {}", seccomp_icon(&jail.seccomp)),
                Style::default().fg(seccomp_color(&jail.seccomp)),
            ),
            Span::styled(
                network_icon(&jail.network),
                Style::default().fg(network_color(&jail.network)),
            ),
        ];

        // Memory bar if limit is set
        if jail.memory_limit_mb > 0 {
            spans.push(Span::styled(" ", Style::default()));
            let ratio = (jail.memory_bytes as f64) / (jail.memory_limit_mb as f64 * 1024.0 * 1024.0);
            spans.extend(memory_bar(ratio));
        }

        spans.push(Span::styled(format!("  {:>6}", dur), Style::default().fg(MUTED)));

        let style = if sel { Style::default().bg(SURFACE) } else { Style::default() };
        ListItem::new(Line::from(spans)).style(style)
    }).collect();

    let list = List::new(items).block(
        Block::default()
            .title(Line::from(Span::styled(
                " Sandboxes ",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(SURFACE))
            .padding(Padding::vertical(1)),
    );
    f.render_widget(list, area);
}

/// Inline memory bar: `[████░░]`
fn memory_bar(ratio: f64) -> Vec<Span<'static>> {
    let ratio = ratio.clamp(0.0, 1.0);
    let width = 6;
    let filled = (ratio * width as f64).round() as usize;
    let empty = width - filled;

    let bar_color = if ratio > 0.9 { ERROR } else if ratio > 0.7 { WARNING } else { SUCCESS };

    vec![
        Span::styled("[", Style::default().fg(MUTED)),
        Span::styled("█".repeat(filled), Style::default().fg(bar_color)),
        Span::styled("░".repeat(empty), Style::default().fg(SURFACE)),
        Span::styled("]", Style::default().fg(MUTED)),
    ]
}

// ---------------------------------------------------------------------------
// Detail view
// ---------------------------------------------------------------------------

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let Some(jail) = app.selected_jail() else {
        f.render_widget(
            Paragraph::new("No jail selected")
                .style(Style::default().fg(MUTED))
                .alignment(Alignment::Center)
                .block(rounded_block("")),
            area,
        );
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(0)])
        .split(area);

    render_info_card(f, jail, chunks[0]);
    render_output(f, jail, app.scroll_offset, chunks[1]);
}

fn render_info_card(f: &mut Frame, jail: &JailInfo, area: Rect) {
    let (icon, color, text) = match jail.status {
        JailStatus::Running => ("●", SUCCESS, "Running"),
        JailStatus::Completed(0) => ("✓", INFO, "Completed"),
        JailStatus::Completed(_) => ("✗", ERROR, "Failed"),
        JailStatus::TimedOut => ("⏱", WARNING, "Timed Out"),
        JailStatus::Killed => ("◼", ERROR, "Killed"),
    };

    let exit_str = match jail.status {
        JailStatus::Completed(c) => format!(" (exit {})", c),
        _ => String::new(),
    };

    let elapsed = jail.started_at.elapsed();
    let mut content = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Command   ", Style::default().fg(MUTED)),
            Span::styled(&jail.command, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Status    ", Style::default().fg(MUTED)),
            Span::styled(format!("{} ", icon), Style::default().fg(color)),
            Span::styled(text, Style::default().fg(color)),
            Span::styled(&exit_str, Style::default().fg(MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  PID       ", Style::default().fg(MUTED)),
            Span::styled(format!("{}", jail.pid), Style::default().fg(Color::White)),
            Span::styled("    Preset  ", Style::default().fg(MUTED)),
            Span::styled(
                format!(" {} ", jail.preset),
                Style::default().fg(BG).bg(preset_color(&jail.preset)),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Elapsed   ", Style::default().fg(MUTED)),
            Span::styled(format_duration(elapsed), Style::default().fg(Color::White)),
            Span::styled("    Memory  ", Style::default().fg(MUTED)),
            Span::styled(format_bytes(jail.memory_bytes), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  Network   ", Style::default().fg(MUTED)),
            Span::styled(&jail.network, Style::default().fg(network_color(&jail.network))),
            Span::styled("    Seccomp ", Style::default().fg(MUTED)),
            Span::styled(&jail.seccomp, Style::default().fg(seccomp_color(&jail.seccomp))),
        ]),
    ];

    // Timeout progress bar
    if jail.timeout_secs > 0 {
        let ratio = (elapsed.as_secs() as f64 / jail.timeout_secs as f64).clamp(0.0, 1.0);
        let bar_color = if ratio > 0.9 { ERROR } else if ratio > 0.75 { WARNING } else { SUCCESS };
        let label = format!("{}s / {}s", elapsed.as_secs(), jail.timeout_secs);

        content.push(Line::from(""));

        // We'll render the LineGauge separately below the paragraph
        let gauge = LineGauge::default()
            .ratio(ratio)
            .label(Span::styled(
                format!("  Timeout   {}", label),
                Style::default().fg(MUTED),
            ))
            .gauge_style(Style::default().fg(bar_color))
            .line_set(ratatui::symbols::line::NORMAL);

        // Render paragraph in the top portion, gauge in the last line
        let inner_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(
                Block::default()
                    .title(Line::from(Span::styled(
                        format!(" Jail #{} ", jail.id),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    )))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(SURFACE))
                    .inner(area),
            );

        // Render the block first
        f.render_widget(
            Block::default()
                .title(Line::from(Span::styled(
                    format!(" Jail #{} ", jail.id),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(SURFACE)),
            area,
        );

        let para = Paragraph::new(content);
        f.render_widget(para, inner_chunks[0]);
        f.render_widget(gauge, inner_chunks[1]);
    } else {
        let block = Block::default()
            .title(Line::from(Span::styled(
                format!(" Jail #{} ", jail.id),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(SURFACE));
        f.render_widget(Paragraph::new(content).block(block), area);
    }
}

fn render_output(f: &mut Frame, jail: &JailInfo, scroll_offset: usize, area: Rect) {
    let total = jail.output.len();
    let visible_height = area.height.saturating_sub(2) as usize; // borders

    // Calculate window: show `visible_height` lines ending at scroll_offset
    let end = (scroll_offset + 1).min(total);
    let start = end.saturating_sub(visible_height);

    let lines: Vec<Line> = jail.output.iter().skip(start).take(end - start).map(|(stream, text)| {
        let color = match stream {
            Stream::Stdout => Color::White,
            Stream::Stderr => ERROR,
        };
        Line::from(Span::styled(text.as_str(), Style::default().fg(color)))
    }).collect();

    let scroll_indicator = if total > visible_height {
        format!(" {}/{} ", end, total)
    } else {
        String::new()
    };

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" output ", Style::default().fg(SUCCESS)),
            Span::styled(
                format!("{}+{} ", jail.stdout_count, jail.stderr_count),
                Style::default().fg(MUTED).add_modifier(Modifier::DIM),
            ),
        ]))
        .title_bottom(Line::from(Span::styled(
            scroll_indicator,
            Style::default().fg(MUTED).add_modifier(Modifier::DIM),
        )).alignment(Alignment::Right))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SURFACE))
        .padding(Padding::horizontal(1));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn rounded_block(title: &str) -> Block {
    let mut b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SURFACE));
    if !title.is_empty() {
        b = b.title(Line::from(Span::styled(
            format!(" {} ", title),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )));
    }
    b
}

fn status_style(status: &JailStatus) -> (&'static str, Color) {
    match status {
        JailStatus::Running => ("●", SUCCESS),
        JailStatus::Completed(0) => ("✓", INFO),
        JailStatus::Completed(_) => ("✗", ERROR),
        JailStatus::TimedOut => ("⏱", WARNING),
        JailStatus::Killed => ("◼", ERROR),
    }
}

fn preset_color(preset: &str) -> Color {
    match preset {
        "build" => PRIMARY,
        "install" => PRIMARY,
        "agent" => INFO,
        "dev" => SUCCESS,
        "test" => WARNING,
        "gpu" => Color::Rgb(236, 72, 153), // Pink
        _ => MUTED,
    }
}

fn seccomp_icon(seccomp: &str) -> &'static str {
    match seccomp {
        "strict" => "◆",
        "standard" => "◆",
        "disabled" | "" => "○",
        _ => "○",
    }
}

fn seccomp_color(seccomp: &str) -> Color {
    match seccomp {
        "strict" => WARNING,
        "standard" => SUCCESS,
        _ => MUTED,
    }
}

fn network_icon(network: &str) -> &'static str {
    match network {
        "allowlist" => "⇄",
        "loopback" => "↻",
        "none" | "" => "◌",
        _ => "◌",
    }
}

fn network_color(network: &str) -> Color {
    match network {
        "allowlist" => INFO,
        "loopback" => MUTED,
        _ => Color::Rgb(63, 63, 70), // Very dim
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
