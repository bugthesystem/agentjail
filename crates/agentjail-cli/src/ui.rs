//! Modern UI rendering for the TUI.

use crate::app::{App, JailInfo, JailStatus, View, format_bytes, format_duration};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Gauge, List, ListItem, Padding, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
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
    // Dark background
    let bg_block = Block::default().style(Style::default().bg(BG));
    f.render_widget(bg_block, f.size());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Main
            Constraint::Length(2), // Footer
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
    let running = app
        .jails
        .values()
        .filter(|j| matches!(j.status, JailStatus::Running))
        .count();
    let total = app.jails.len();

    let header = Paragraph::new(Line::from(vec![
        Span::styled("◉ ", Style::default().fg(PRIMARY)),
        Span::styled(
            "agentjail",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  │  ", Style::default().fg(MUTED)),
        Span::styled(
            format!("{} running", running),
            Style::default().fg(SUCCESS),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            format!("{} total", total),
            Style::default().fg(MUTED),
        ),
    ]))
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
            ("↑↓", "navigate"),
            ("⏎", "details"),
            ("k", "kill"),
            ("c", "clear"),
            ("q", "quit"),
        ],
        View::Detail => vec![
            ("esc", "back"),
            ("k", "kill"),
            ("q", "quit"),
        ],
    };

    let spans: Vec<Span> = keys
        .iter()
        .enumerate()
        .flat_map(|(i, (key, action))| {
            let mut v = vec![
                Span::styled(
                    format!(" {} ", key),
                    Style::default().fg(BG).bg(MUTED),
                ),
                Span::styled(format!(" {} ", action), Style::default().fg(MUTED)),
            ];
            if i < keys.len() - 1 {
                v.push(Span::styled(" ", Style::default()));
            }
            v
        })
        .collect();

    let footer = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center);

    f.render_widget(footer, area);
}

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    let ids = app.sorted_ids();

    if ids.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("◇", Style::default().fg(MUTED))),
            Line::from(""),
            Line::from(Span::styled("No jails running", Style::default().fg(MUTED))),
            Line::from(""),
            Line::from(Span::styled(
                "Run `agentjail run` to start a sandbox",
                Style::default().fg(MUTED).add_modifier(Modifier::DIM),
            )),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(SURFACE)),
        );
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = ids
        .iter()
        .map(|&id| {
            let jail = &app.jails[&id];
            let is_selected = app.selected == Some(id);

            let (status_icon, status_color) = match jail.status {
                JailStatus::Running => ("●", SUCCESS),
                JailStatus::Completed(0) => ("✓", INFO),
                JailStatus::Completed(_) => ("✗", ERROR),
                JailStatus::TimedOut => ("⏱", WARNING),
                JailStatus::Killed => ("◼", ERROR),
            };

            let duration = format_duration(jail.started_at.elapsed());

            // Build the line with nice spacing
            let line = Line::from(vec![
                // Selection indicator
                Span::styled(
                    if is_selected { " ▸ " } else { "   " },
                    Style::default().fg(PRIMARY),
                ),
                // Status icon
                Span::styled(format!("{} ", status_icon), Style::default().fg(status_color)),
                // Command (main content)
                Span::styled(
                    format!("{:<30}", truncate(&jail.command, 30)),
                    if is_selected {
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                // Preset tag
                Span::styled(
                    format!(" {} ", jail.preset),
                    Style::default().fg(BG).bg(preset_color(&jail.preset)),
                ),
                // Duration (right aligned)
                Span::styled(
                    format!("  {:>8}", duration),
                    Style::default().fg(MUTED),
                ),
            ]);

            let style = if is_selected {
                Style::default().bg(SURFACE)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(Line::from(vec![
                    Span::styled(" Sandboxes ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(SURFACE))
                .padding(Padding::vertical(1)),
        )
        .highlight_style(Style::default());

    f.render_widget(list, area);
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let Some(jail) = app.selected_jail() else {
        let empty = Paragraph::new("No jail selected")
            .style(Style::default().fg(MUTED))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(SURFACE)),
            );
        f.render_widget(empty, area);
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Info card
            Constraint::Min(0),    // Output
        ])
        .split(area);

    render_info_card(f, jail, chunks[0]);
    render_output_panels(f, jail, chunks[1]);
}

fn render_info_card(f: &mut Frame, jail: &JailInfo, area: Rect) {
    let (status_icon, status_color, status_text) = match jail.status {
        JailStatus::Running => ("●", SUCCESS, "Running"),
        JailStatus::Completed(0) => ("✓", INFO, "Completed"),
        JailStatus::Completed(c) => ("✗", ERROR, "Failed"),
        JailStatus::TimedOut => ("⏱", WARNING, "Timed Out"),
        JailStatus::Killed => ("◼", ERROR, "Killed"),
    };

    let exit_code = match jail.status {
        JailStatus::Completed(c) => format!(" (exit {})", c),
        _ => String::new(),
    };

    let content = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Command   ", Style::default().fg(MUTED)),
            Span::styled(&jail.command, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Status    ", Style::default().fg(MUTED)),
            Span::styled(format!("{} ", status_icon), Style::default().fg(status_color)),
            Span::styled(status_text, Style::default().fg(status_color)),
            Span::styled(&exit_code, Style::default().fg(MUTED)),
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
            Span::styled(
                format_duration(jail.started_at.elapsed()),
                Style::default().fg(Color::White),
            ),
            Span::styled("    Memory  ", Style::default().fg(MUTED)),
            Span::styled(format_bytes(jail.memory_bytes), Style::default().fg(Color::White)),
        ]),
    ];

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(
                format!(" Jail #{} ", jail.id),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SURFACE));

    let para = Paragraph::new(content).block(block);
    f.render_widget(para, area);
}

fn render_output_panels(f: &mut Frame, jail: &JailInfo, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // stdout panel
    let stdout_lines: Vec<Line> = jail
        .stdout_lines
        .iter()
        .rev()
        .take(100)
        .rev()
        .map(|s| Line::from(Span::styled(s.as_str(), Style::default().fg(Color::White))))
        .collect();

    let stdout_block = Block::default()
        .title(Line::from(vec![
            Span::styled(" stdout ", Style::default().fg(SUCCESS)),
            Span::styled(
                format!(" {} lines ", jail.stdout_lines.len()),
                Style::default().fg(MUTED).add_modifier(Modifier::DIM),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SURFACE))
        .padding(Padding::horizontal(1));

    let stdout = Paragraph::new(stdout_lines)
        .wrap(Wrap { trim: false })
        .block(stdout_block);

    // stderr panel
    let stderr_lines: Vec<Line> = jail
        .stderr_lines
        .iter()
        .rev()
        .take(100)
        .rev()
        .map(|s| Line::from(Span::styled(s.as_str(), Style::default().fg(ERROR))))
        .collect();

    let stderr_block = Block::default()
        .title(Line::from(vec![
            Span::styled(" stderr ", Style::default().fg(ERROR)),
            Span::styled(
                format!(" {} lines ", jail.stderr_lines.len()),
                Style::default().fg(MUTED).add_modifier(Modifier::DIM),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SURFACE))
        .padding(Padding::horizontal(1));

    let stderr = Paragraph::new(stderr_lines)
        .wrap(Wrap { trim: false })
        .block(stderr_block);

    f.render_widget(stdout, chunks[0]);
    f.render_widget(stderr, chunks[1]);
}

fn preset_color(preset: &str) -> Color {
    match preset {
        "build" => Color::Rgb(139, 92, 246),  // Violet
        "agent" => Color::Rgb(56, 189, 248),  // Sky
        "dev" => Color::Rgb(34, 197, 94),     // Green
        "test" => Color::Rgb(250, 204, 21),   // Yellow
        _ => MUTED,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
