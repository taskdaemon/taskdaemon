//! TUI views and rendering

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap};

use super::app::{App, AppMode};

/// Main render function
pub fn render(app: &mut App, frame: &mut Frame) {
    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Main content
            Constraint::Length(3), // Footer
        ])
        .split(frame.area());

    // Render header
    render_header(app, frame, chunks[0]);

    // Render main content based on mode
    match app.mode {
        AppMode::Dashboard => render_dashboard(app, frame, chunks[1]),
        AppMode::LoopDetail => render_detail(app, frame, chunks[1]),
        AppMode::Metrics => render_metrics(app, frame, chunks[1]),
        AppMode::Help => {
            // Render dashboard behind help overlay
            render_dashboard(app, frame, chunks[1]);
            render_help_overlay(frame, chunks[1]);
        }
    }

    // Render footer
    render_footer(app, frame, chunks[2]);
}

/// Render the header bar
fn render_header(app: &App, frame: &mut Frame, area: Rect) {
    let mode_text = match app.mode {
        AppMode::Dashboard => "Dashboard",
        AppMode::LoopDetail => "Loop Detail",
        AppMode::Metrics => "Metrics",
        AppMode::Help => "Help",
    };

    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            "TaskDaemon ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("│ "),
        Span::styled(mode_text, Style::default().fg(Color::Yellow)),
        Span::raw(" │ "),
        Span::styled(
            format!("{} active", app.metrics.active_loops),
            Style::default().fg(Color::Green),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("{} completed", app.metrics.completed_loops),
            Style::default().fg(Color::Blue),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("{} failed", app.metrics.failed_loops),
            Style::default().fg(Color::Red),
        ),
    ])])
    .block(Block::default().borders(Borders::ALL).title(" Status "));

    frame.render_widget(header, area);
}

/// Render the dashboard view (loop list)
fn render_dashboard(app: &App, frame: &mut Frame, area: Rect) {
    // Split into loop list and preview
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Loop list
    render_loop_list(app, frame, chunks[0]);

    // Preview pane
    render_preview(app, frame, chunks[1]);
}

/// Render the loop list
fn render_loop_list(app: &App, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = app
        .loops
        .iter()
        .enumerate()
        .map(|(i, loop_display)| {
            let status_color = match loop_display.status.as_str() {
                "running" => Color::Green,
                "pending" => Color::Yellow,
                "complete" => Color::Blue,
                "failed" => Color::Red,
                _ => Color::Gray,
            };

            let content = Line::from(vec![
                Span::styled(format!("{:>3} ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:<12} ", loop_display.exec_id),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[{:^8}] ", loop_display.loop_type),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{:<10}", loop_display.status),
                    Style::default().fg(status_color),
                ),
                Span::styled(
                    format!(" iter:{:<3}", loop_display.iteration),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);

            if i == app.selected_loop {
                ListItem::new(content).style(Style::default().bg(Color::DarkGray).fg(Color::White))
            } else {
                ListItem::new(content)
            }
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Loops "))
        .highlight_style(Style::default().bg(Color::DarkGray));

    frame.render_widget(list, area);
}

/// Render the preview pane
fn render_preview(app: &App, frame: &mut Frame, area: Rect) {
    let content = if let Some(loop_data) = app.selected_loop_data() {
        vec![
            Line::from(vec![
                Span::styled("ID: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&loop_data.exec_id),
            ]),
            Line::from(vec![
                Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&loop_data.loop_type),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&loop_data.status),
            ]),
            Line::from(vec![
                Span::styled("Iteration: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(loop_data.iteration.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Started: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&loop_data.started_at),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Progress:",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(loop_data.progress.as_str()),
        ]
    } else {
        vec![Line::from("No loop selected")]
    };

    let preview = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title(" Preview "))
        .wrap(Wrap { trim: true });

    frame.render_widget(preview, area);
}

/// Render the detail view for a single loop
fn render_detail(app: &App, frame: &mut Frame, area: Rect) {
    let content = if let Some(loop_data) = app.selected_loop_data() {
        vec![
            Line::from(vec![
                Span::styled("Execution ID: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(&loop_data.exec_id, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Loop Type: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&loop_data.loop_type),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&loop_data.status),
            ]),
            Line::from(vec![
                Span::styled("Current Iteration: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(loop_data.iteration.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Started: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&loop_data.started_at),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Progress History:",
                Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )]),
            Line::from(""),
            Line::from(&*loop_data.progress),
        ]
    } else {
        vec![Line::from("No loop selected")]
    };

    let detail = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Loop Detail (←/→ to navigate, Esc to go back) "),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(detail, area);
}

/// Render the metrics view
fn render_metrics(app: &App, frame: &mut Frame, area: Rect) {
    let rows = vec![
        Row::new(vec![
            Cell::from("Active Loops"),
            Cell::from(app.metrics.active_loops.to_string()),
        ]),
        Row::new(vec![
            Cell::from("Completed Loops"),
            Cell::from(app.metrics.completed_loops.to_string()),
        ]),
        Row::new(vec![
            Cell::from("Failed Loops"),
            Cell::from(app.metrics.failed_loops.to_string()),
        ]),
        Row::new(vec![
            Cell::from("Total Iterations"),
            Cell::from(app.metrics.total_iterations.to_string()),
        ]),
        Row::new(vec![
            Cell::from("Total API Calls"),
            Cell::from(app.metrics.total_api_calls.to_string()),
        ]),
    ];

    let table = Table::new(rows, [Constraint::Percentage(50), Constraint::Percentage(50)])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Metrics (Esc to go back) "),
        )
        .header(Row::new(vec!["Metric", "Value"]).style(Style::default().add_modifier(Modifier::BOLD)));

    frame.render_widget(table, area);
}

/// Render help overlay
fn render_help_overlay(frame: &mut Frame, area: Rect) {
    // Calculate centered overlay
    let popup_area = centered_rect(60, 60, area);

    // Clear the area
    frame.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(vec![Span::styled(
            "Keyboard Shortcuts",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("q, Ctrl+c  ", Style::default().fg(Color::Cyan)),
            Span::raw("Quit"),
        ]),
        Line::from(vec![
            Span::styled("?, F1      ", Style::default().fg(Color::Cyan)),
            Span::raw("Toggle help"),
        ]),
        Line::from(vec![
            Span::styled("Esc        ", Style::default().fg(Color::Cyan)),
            Span::raw("Back / Close"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Dashboard",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("↑/↓, j/k   ", Style::default().fg(Color::Cyan)),
            Span::raw("Navigate loops"),
        ]),
        Line::from(vec![
            Span::styled("Enter      ", Style::default().fg(Color::Cyan)),
            Span::raw("View loop detail"),
        ]),
        Line::from(vec![
            Span::styled("m          ", Style::default().fg(Color::Cyan)),
            Span::raw("View metrics"),
        ]),
        Line::from(vec![
            Span::styled("r          ", Style::default().fg(Color::Cyan)),
            Span::raw("Refresh"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Loop Detail",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("←/→, h/l   ", Style::default().fg(Color::Cyan)),
            Span::raw("Previous/next loop"),
        ]),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .style(Style::default().bg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(help, popup_area);
}

/// Render the footer bar
fn render_footer(_app: &App, frame: &mut Frame, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" q", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" Quit "),
        Span::styled(" ?", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" Help "),
        Span::styled(" ↑↓", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" Navigate "),
        Span::styled(" Enter", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" Select "),
        Span::styled(" m", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" Metrics "),
    ]))
    .block(Block::default().borders(Borders::ALL));

    frame.render_widget(footer, area);
}

/// Helper to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
