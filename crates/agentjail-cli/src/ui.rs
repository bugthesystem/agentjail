//! UI rendering for the TUI.

use crate::app::{App, JailInfo, JailStatus, View, format_bytes, format_duration};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

const BRAND_COLOR: Color = Color::Rgb(99, 102, 241); // Indigo

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Main
            Constraint::Length(1), // Footer
        ])
        .split(f.size());

    render_header(f, chunks[0]);

    match app.view {
        View::List => render_list(f, app, chunks[1]),
        View::Detail => render_detail(f, app, chunks[1]),
    }

    render_footer(f, app, chunks[2]);
}

fn render_header(f: &mut Frame, area: Rect) {
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " agentjail ",
            Style::default()
                .fg(Color::White)
                .bg(BRAND_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled("sandbox monitor", Style::default().fg(Color::DarkGray)),
    ]))
    .block(Block::default().borders(Borders::BOTTOM));

    f.render_widget(header, area);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let help = match app.view {
        View::List => "j/k: nav | Enter: detail | K: kill | C: clear | q: quit",
        View::Detail => "Esc: back | K: kill | Tab: stdout/stderr | q: quit",
    };

    let running = app
        .jails
        .values()
        .filter(|j| matches!(j.status, JailStatus::Running))
        .count();

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {} running ", running),
            Style::default().fg(BRAND_COLOR),
        ),
        Span::raw("| "),
        Span::styled(help, Style::default().fg(Color::DarkGray)),
    ]));

    f.render_widget(footer, area);
}

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    let ids = app.sorted_ids();

    if ids.is_empty() {
        let empty = Paragraph::new("No jails running")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" Jails "));
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = ids
        .iter()
        .map(|&id| {
            let jail = &app.jails[&id];
            let is_selected = app.selected == Some(id);

            let status_style = match jail.status {
                JailStatus::Running => Style::default().fg(Color::Green),
                JailStatus::Completed(0) => Style::default().fg(Color::Blue),
                JailStatus::Completed(_) => Style::default().fg(Color::Red),
                JailStatus::TimedOut => Style::default().fg(Color::Yellow),
                JailStatus::Killed => Style::default().fg(Color::Magenta),
            };

            let line = Line::from(vec![
                Span::styled(
                    if is_selected { "▶ " } else { "  " },
                    Style::default().fg(BRAND_COLOR),
                ),
                Span::styled(
                    format!("{:>3} ", id),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<10} ", jail.status.as_str()),
                    status_style,
                ),
                Span::styled(
                    format!("{:<8} ", jail.preset),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(&jail.command),
                Span::styled(
                    format!("  {}", format_duration(jail.started_at.elapsed())),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);

            let style = if is_selected {
                Style::default().bg(Color::Rgb(30, 30, 40))
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(" Jails "));

    f.render_widget(list, area);
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let Some(jail) = app.selected_jail() else {
        let empty = Paragraph::new("No jail selected")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" Detail "));
        f.render_widget(empty, area);
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // Info
            Constraint::Min(0),    // Output
        ])
        .split(area);

    render_info(f, jail, chunks[0]);
    render_output(f, jail, chunks[1]);
}

fn render_info(f: &mut Frame, jail: &JailInfo, area: Rect) {
    let status_style = match jail.status {
        JailStatus::Running => Style::default().fg(Color::Green),
        JailStatus::Completed(0) => Style::default().fg(Color::Blue),
        JailStatus::Completed(_) => Style::default().fg(Color::Red),
        JailStatus::TimedOut => Style::default().fg(Color::Yellow),
        JailStatus::Killed => Style::default().fg(Color::Magenta),
    };

    let info = vec![
        Line::from(vec![
            Span::styled("Command: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&jail.command),
        ]),
        Line::from(vec![
            Span::styled("Status:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(jail.status.as_str(), status_style),
            Span::raw("  "),
            Span::styled("PID: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}", jail.pid)),
        ]),
        Line::from(vec![
            Span::styled("Preset:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&jail.preset, Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("Elapsed: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format_duration(jail.started_at.elapsed())),
        ]),
        Line::from(vec![
            Span::styled("Memory:  ", Style::default().fg(Color::DarkGray)),
            Span::raw(format_bytes(jail.memory_bytes)),
            Span::raw("  "),
            Span::styled("CPU: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{:.1}%", jail.cpu_percent)),
        ]),
    ];

    let para = Paragraph::new(info)
        .block(Block::default().borders(Borders::ALL).title(format!(
            " Jail #{} ",
            jail.id
        )));

    f.render_widget(para, area);
}

fn render_output(f: &mut Frame, jail: &JailInfo, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // stdout
    let stdout_text: Vec<Line> = jail
        .stdout_lines
        .iter()
        .rev()
        .take(100)
        .rev()
        .map(|s| Line::raw(s.as_str()))
        .collect();

    let stdout = Paragraph::new(stdout_text)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" stdout ")
                .border_style(Style::default().fg(Color::Green)),
        );

    // stderr
    let stderr_text: Vec<Line> = jail
        .stderr_lines
        .iter()
        .rev()
        .take(100)
        .rev()
        .map(|s| Line::raw(s.as_str()))
        .collect();

    let stderr = Paragraph::new(stderr_text)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" stderr ")
                .border_style(Style::default().fg(Color::Red)),
        );

    f.render_widget(stdout, chunks[0]);
    f.render_widget(stderr, chunks[1]);
}
