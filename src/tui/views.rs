//! TUI views and rendering
//!
//! All rendering logic is contained here. The views module is responsible
//! for drawing the UI based on AppState, but never modifies state.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap};

use super::state::{AppState, ConfirmDialog, InteractionMode, View};

/// Status colors (k9s-inspired)
mod colors {
    use ratatui::style::Color;

    pub const RUNNING: Color = Color::Rgb(0, 255, 127); // Spring green
    pub const PENDING: Color = Color::Rgb(255, 215, 0); // Gold
    pub const COMPLETE: Color = Color::Rgb(50, 205, 50); // Lime green
    pub const FAILED: Color = Color::Rgb(220, 20, 60); // Crimson
    pub const BLOCKED: Color = Color::Rgb(255, 69, 0); // Orange red
    pub const HEADER: Color = Color::Rgb(0, 255, 255); // Cyan
    pub const KEYBIND: Color = Color::Rgb(0, 255, 255); // Cyan
    pub const SELECTED_BG: Color = Color::Rgb(40, 40, 40);
    pub const DIM: Color = Color::DarkGray;
}

/// Get color for a status string
fn status_color(status: &str) -> Color {
    match status {
        "running" => colors::RUNNING,
        "pending" => colors::PENDING,
        "complete" | "completed" => colors::COMPLETE,
        "failed" => colors::FAILED,
        "blocked" => colors::BLOCKED,
        "paused" => Color::Yellow,
        "stopped" | "cancelled" => Color::DarkGray,
        "rebasing" => Color::Magenta,
        "in_progress" => colors::RUNNING,
        "ready" => colors::PENDING,
        "draft" => colors::DIM,
        _ => Color::Gray,
    }
}

/// Get status icon
fn status_icon(status: &str) -> &'static str {
    match status {
        "running" | "in_progress" => "●",
        "pending" | "ready" => "○",
        "blocked" => "?",
        "complete" | "completed" => "✓",
        "failed" => "✗",
        "cancelled" | "stopped" => "⊘",
        "paused" => "◑",
        "rebasing" => "↻",
        "draft" => "◌",
        _ => " ",
    }
}

/// Main render function
pub fn render(state: &AppState, frame: &mut Frame) {
    // Create main layout: header, content, footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Main content
            Constraint::Length(3), // Footer
        ])
        .split(frame.area());

    // Render header (breadcrumb + metrics)
    render_header(state, frame, chunks[0]);

    // Render main content based on current view
    match &state.current_view {
        View::Records { .. } => render_records_table(state, frame, chunks[1]),
        View::Executions => render_executions_table(state, frame, chunks[1]),
        View::Logs { .. } => render_logs_view(state, frame, chunks[1]),
        View::Describe { .. } => render_describe_view(state, frame, chunks[1]),
    }

    // Render footer (context-sensitive keybinds or input)
    render_footer(state, frame, chunks[2]);

    // Render overlays
    match &state.interaction_mode {
        InteractionMode::Help => render_help_overlay(frame, frame.area()),
        InteractionMode::Confirm(dialog) => render_confirm_dialog(dialog, frame, frame.area()),
        _ => {}
    }
}

/// Render header with breadcrumb and metrics
fn render_header(state: &AppState, frame: &mut Frame, area: Rect) {
    let breadcrumb = state.breadcrumb();
    let mut spans = vec![
        Span::styled(
            " TaskDaemon ",
            Style::default().fg(colors::HEADER).add_modifier(Modifier::BOLD),
        ),
        Span::raw("│ "),
        Span::styled(breadcrumb, Style::default().fg(Color::Yellow)),
    ];

    // Add filter indicator if active
    if !state.filter_text.is_empty() {
        spans.push(Span::raw(" │ "));
        spans.push(Span::styled(
            format!("Filter: /{}", &state.filter_text),
            Style::default().fg(Color::Magenta),
        ));
    }

    // Add metrics
    spans.push(Span::raw(" │ "));
    spans.push(Span::styled(
        format!("{} records", state.total_records),
        Style::default().fg(Color::Cyan),
    ));
    spans.push(Span::raw(" │ "));
    spans.push(Span::styled(
        format!("{} active", state.executions_active),
        Style::default().fg(colors::RUNNING),
    ));
    if state.executions_complete > 0 {
        spans.push(Span::raw(" │ "));
        spans.push(Span::styled(
            format!("{} complete", state.executions_complete),
            Style::default().fg(colors::COMPLETE),
        ));
    }
    if state.executions_failed > 0 {
        spans.push(Span::raw(" │ "));
        spans.push(Span::styled(
            format!("{} failed", state.executions_failed),
            Style::default().fg(colors::FAILED),
        ));
    }

    let header = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL));

    frame.render_widget(header, area);
}

/// Render Records table (generic Loop records)
fn render_records_table(state: &AppState, frame: &mut Frame, area: Rect) {
    let filtered = state.filtered_records();
    let selected_idx = state.records_selection.selected_index;

    // Get type filter for title
    let title = match &state.current_view {
        View::Records {
            type_filter: Some(t),
            parent_filter: Some(p),
        } => {
            format!(" {} > {} ({}) ", t, &p[..8.min(p.len())], filtered.len())
        }
        View::Records {
            type_filter: Some(t),
            parent_filter: None,
        } => {
            format!(" {} ({}) ", t, filtered.len())
        }
        View::Records {
            type_filter: None,
            parent_filter: Some(p),
        } => {
            format!(" Records > {} ({}) ", &p[..8.min(p.len())], filtered.len())
        }
        _ => format!(" Records ({}) ", filtered.len()),
    };

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(i, record)| {
            let row_style = if i == selected_idx {
                Style::default().bg(colors::SELECTED_BG)
            } else {
                Style::default()
            };

            Row::new(vec![
                format!("{} {}", status_icon(&record.status), &record.title),
                record.loop_type.clone(),
                record.status.clone(),
                record.phases_progress.clone(),
                record.created.clone(),
            ])
            .style(row_style)
        })
        .collect();

    let widths = [
        Constraint::Min(30),    // NAME
        Constraint::Length(12), // TYPE
        Constraint::Length(12), // STATUS
        Constraint::Length(8),  // PHASES
        Constraint::Length(12), // CREATED
    ];

    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["NAME", "TYPE", "STATUS", "PHASES", "CREATED"])
                .style(Style::default().add_modifier(Modifier::BOLD).fg(colors::HEADER)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(colors::HEADER)),
        );

    frame.render_widget(table, area);

    if filtered.is_empty() {
        render_empty_message(frame, area, "No records found.");
    }
}

/// Render Executions table (running LoopExecutions)
fn render_executions_table(state: &AppState, frame: &mut Frame, area: Rect) {
    let filtered = state.filtered_executions();
    let selected_idx = state.executions_selection.selected_index;

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(i, exec_item)| {
            let row_style = if i == selected_idx {
                Style::default().bg(colors::SELECTED_BG)
            } else {
                Style::default()
            };

            Row::new(vec![
                format!("{} {}", status_icon(&exec_item.status), &exec_item.name),
                exec_item.loop_type.clone(),
                exec_item.iteration.clone(),
                exec_item.status.clone(),
                exec_item.duration.clone(),
            ])
            .style(row_style)
        })
        .collect();

    let widths = [
        Constraint::Min(30),    // NAME
        Constraint::Length(12), // TYPE
        Constraint::Length(8),  // ITER
        Constraint::Length(12), // STATUS
        Constraint::Length(10), // DURATION
    ];

    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["NAME", "TYPE", "ITER", "STATUS", "DURATION"])
                .style(Style::default().add_modifier(Modifier::BOLD).fg(colors::HEADER)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Executions ({}) ", filtered.len()))
                .border_style(Style::default().fg(colors::HEADER)),
        );

    frame.render_widget(table, area);

    if filtered.is_empty() {
        render_empty_message(frame, area, "No running executions. Press <n> to create a new task.");
    }
}

/// Render Logs view
fn render_logs_view(state: &AppState, frame: &mut Frame, area: Rect) {
    let target_id = if let View::Logs { target_id } = &state.current_view {
        target_id.clone()
    } else {
        return;
    };

    let lines: Vec<Line> = state
        .logs
        .iter()
        .map(|entry| {
            let prefix_style = if entry.is_error {
                Style::default().fg(colors::FAILED)
            } else if entry.is_stdout {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(colors::DIM)
            };

            let prefix = if entry.is_error {
                format!("[iter {}] ERROR: ", entry.iteration)
            } else if entry.is_stdout {
                format!("[iter {}] STDOUT: ", entry.iteration)
            } else {
                format!("[iter {}] ", entry.iteration)
            };

            Line::from(vec![Span::styled(prefix, prefix_style), Span::raw(&entry.text)])
        })
        .collect();

    // Add cursor if following
    let mut display_lines = lines;
    if state.logs_follow && !display_lines.is_empty() {
        display_lines.push(Line::from(Span::styled(
            "▌",
            Style::default().add_modifier(Modifier::SLOW_BLINK),
        )));
    }

    let follow_indicator = if state.logs_follow { " [following]" } else { "" };
    let title = format!(" Logs: {}{} ", truncate_str(&target_id, 30), follow_indicator);

    let logs = Paragraph::new(display_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(colors::HEADER)),
        )
        .wrap(Wrap { trim: false })
        .scroll((state.logs_scroll as u16, 0));

    frame.render_widget(logs, area);

    if state.logs.is_empty() {
        render_empty_message(frame, area, "No logs yet.");
    }
}

/// Render Describe view
fn render_describe_view(state: &AppState, frame: &mut Frame, area: Rect) {
    let data = match &state.describe_data {
        Some(d) => d,
        None => {
            render_empty_message(frame, area, "Loading...");
            return;
        }
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Name:        ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&data.title),
        ]),
        Line::from(vec![
            Span::styled("Type:        ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&data.loop_type),
        ]),
        Line::from(vec![
            Span::styled("Status:      ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(&data.status, Style::default().fg(status_color(&data.status))),
        ]),
    ];

    if let Some(ref parent) = data.parent_id {
        lines.push(Line::from(vec![
            Span::styled("Parent:      ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(parent),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("Created:     ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(&data.created),
    ]));

    // Fields section
    for (key, value) in &data.fields {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<12} ", key), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(value),
        ]));
    }

    // Children section
    if !data.children.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            format!("Children: {}", data.children.len()),
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]));
        for child_id in &data.children {
            lines.push(Line::from(vec![Span::raw("  • "), Span::raw(child_id)]));
        }
    }

    // Execution summary
    if let Some(ref exec) = data.execution {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Current Execution:",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]));
        lines.push(Line::from(vec![Span::raw("  Iteration: "), Span::raw(&exec.iteration)]));
        lines.push(Line::from(vec![Span::raw("  Duration:  "), Span::raw(&exec.duration)]));
        if !exec.progress.is_empty() {
            lines.push(Line::from(vec![Span::raw("  Progress:  "), Span::raw(&exec.progress)]));
        }
    }

    let title = format!(" Describe: {} ", truncate_str(&data.title, 30));

    let describe = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(colors::HEADER)),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(describe, area);
}

/// Render footer with context-sensitive keybinds
fn render_footer(state: &AppState, frame: &mut Frame, area: Rect) {
    let content = match &state.interaction_mode {
        InteractionMode::Filter(text) => Line::from(vec![
            Span::styled("/", Style::default().fg(colors::KEYBIND)),
            Span::raw(text),
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
        ]),
        InteractionMode::Command(text) => Line::from(vec![
            Span::styled(":", Style::default().fg(colors::KEYBIND)),
            Span::raw(text),
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
        ]),
        InteractionMode::TaskInput(text) => Line::from(vec![
            Span::styled(
                "New Task: ",
                Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD),
            ),
            Span::raw(text),
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
            Span::styled("  (Enter to create, Esc to cancel)", Style::default().fg(colors::DIM)),
        ]),
        _ => {
            // Show error or context-sensitive keybinds
            if let Some(ref error) = state.error_message {
                Line::from(Span::styled(
                    format!(" Error: {}", error),
                    Style::default().fg(colors::FAILED),
                ))
            } else {
                // Show keybinds based on current view
                let keybinds = match &state.current_view {
                    View::Records { .. } => vec![
                        ("<Enter>", "Children"),
                        ("<d>", "Describe"),
                        ("<l>", "Logs"),
                        ("<Esc>", "Back"),
                    ],
                    View::Executions => vec![
                        ("<n>", "New Task"),
                        ("<d>", "Describe"),
                        ("<l>", "Logs"),
                        ("<x>", "Cancel"),
                        ("<D>", "Delete"),
                    ],
                    View::Logs { .. } => vec![("<Esc>", "Back"), ("<f>", "Follow")],
                    View::Describe { .. } => vec![("<Esc>", "Back"), ("<l>", "Logs")],
                };

                let mut spans = vec![Span::raw(" ")];
                for (key, action) in keybinds {
                    spans.push(Span::styled(
                        key,
                        Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD),
                    ));
                    spans.push(Span::raw(format!(" {} ", action)));
                }
                spans.push(Span::raw("│ "));
                spans.push(Span::styled(
                    "<?>",
                    Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" Help "));
                spans.push(Span::styled(
                    "<q>",
                    Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" Quit"));

                Line::from(spans)
            }
        }
    };

    let footer = Paragraph::new(content).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
}

/// Render help overlay
fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(60, 70, area);
    frame.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(vec![Span::styled(
            "Keyboard Shortcuts",
            Style::default()
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                .fg(colors::HEADER),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Global",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        key_line(":", "Command mode (:records, :executions, :<type>)"),
        key_line("/", "Filter current view"),
        key_line("?", "Toggle help"),
        key_line("q", "Quit"),
        key_line("Esc", "Back / Clear filter"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Navigation",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        key_line("j/↓", "Move down"),
        key_line("k/↑", "Move up"),
        key_line("g", "Go to top"),
        key_line("G", "Go to bottom"),
        key_line("Enter", "Drill into selected"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Actions",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        key_line("l", "View logs/progress"),
        key_line("d", "Describe (full details)"),
        key_line("x", "Cancel selected"),
        key_line("p", "Pause selected"),
        key_line("r", "Resume selected"),
        key_line("D", "Delete selected"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Logs View",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        key_line("f", "Toggle follow mode"),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help (? to close) ")
                .style(Style::default().bg(Color::Black)),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(help, popup_area);
}

/// Helper to create a key binding line
fn key_line<'a>(key: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{:<12}", key), Style::default().fg(colors::KEYBIND)),
        Span::raw(desc),
    ])
}

/// Render confirmation dialog
fn render_confirm_dialog(dialog: &ConfirmDialog, frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(50, 20, area);
    frame.render_widget(Clear, popup_area);

    let yes_style = if dialog.selected_button {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };

    let no_style = if !dialog.selected_button {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };

    let content = vec![
        Line::from(""),
        Line::from(dialog.message.as_str()),
        Line::from(""),
        Line::from(vec![
            Span::raw("       "),
            Span::styled(" No ", no_style),
            Span::raw("    "),
            Span::styled(" Yes ", yes_style),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Tab/←→: switch  Enter: confirm  Esc: cancel",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let dialog_widget = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Confirm ")
                .style(Style::default().bg(Color::Black)),
        )
        .alignment(ratatui::layout::Alignment::Center);

    frame.render_widget(dialog_widget, popup_area);
}

/// Render empty state message
fn render_empty_message(frame: &mut Frame, area: Rect, message: &str) {
    let inner = area.inner(ratatui::layout::Margin {
        horizontal: 2,
        vertical: 2,
    });

    let empty = Paragraph::new(message)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Center);

    frame.render_widget(empty, inner);
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

/// Truncate a string for display
fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len { s } else { &s[..max_len] }
}
