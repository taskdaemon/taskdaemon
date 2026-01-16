//! TUI views and rendering
//!
//! All rendering logic is contained here. The views module is responsible
//! for drawing the UI based on AppState, but never modifies state.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use super::app::App;
use super::state::{ConfirmDialog, InteractionMode, LayoutMode, ResourceItem, ResourceView};

/// Status colors based on the design spec (Appendix A)
mod colors {
    use ratatui::style::Color;

    pub const RUNNING: Color = Color::Rgb(0, 255, 127);
    pub const PENDING: Color = Color::Rgb(255, 215, 0);
    pub const COMPLETE: Color = Color::Rgb(50, 205, 50);
    pub const FAILED: Color = Color::Rgb(220, 20, 60);
    pub const BLOCKED: Color = Color::Rgb(255, 69, 0);
    pub const HEADER: Color = Color::Rgb(0, 255, 255);
    pub const KEYBIND: Color = Color::Rgb(0, 255, 255);
    pub const SELECTED_BG: Color = Color::Rgb(40, 40, 40);
}

/// Get color for a status string
fn status_color(status: &str) -> Color {
    match status {
        "running" => colors::RUNNING,
        "pending" => colors::PENDING,
        "complete" => colors::COMPLETE,
        "failed" => colors::FAILED,
        "blocked" => colors::BLOCKED,
        "paused" => Color::Yellow,
        "stopped" | "cancelled" => Color::DarkGray,
        _ => Color::Gray,
    }
}

/// Main render function
pub fn render(app: &mut App, frame: &mut Frame) {
    let state = app.state();

    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Main content
            Constraint::Length(3), // Footer/command bar
        ])
        .split(frame.area());

    // Render header
    render_header(app, frame, chunks[0]);

    // Render main content based on layout mode
    match state.layout_mode {
        LayoutMode::Dashboard => render_dashboard(app, frame, chunks[1]),
        LayoutMode::Split => render_split(app, frame, chunks[1]),
        LayoutMode::Grid => render_grid(app, frame, chunks[1]),
        LayoutMode::Focus => render_focus(app, frame, chunks[1]),
    }

    // Render footer/command bar
    render_footer(app, frame, chunks[2]);

    // Render overlays (help, confirm dialog)
    match &state.interaction_mode {
        InteractionMode::Help => {
            render_help_overlay(frame, frame.area());
        }
        InteractionMode::Confirm(dialog) => {
            render_confirm_dialog(dialog, frame, frame.area());
        }
        _ => {}
    }
}

/// Render the header bar
fn render_header(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.state();

    let view_text = state.current_view.display_name();
    let layout_text = match state.layout_mode {
        LayoutMode::Dashboard => "Dashboard",
        LayoutMode::Split => "Split",
        LayoutMode::Grid => "Grid",
        LayoutMode::Focus => "Focus",
    };

    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            "TaskDaemon ",
            Style::default().fg(colors::HEADER).add_modifier(Modifier::BOLD),
        ),
        Span::raw("│ "),
        Span::styled(view_text, Style::default().fg(Color::Yellow)),
        Span::raw(" │ "),
        Span::styled(layout_text, Style::default().fg(Color::Magenta)),
        Span::raw(" │ "),
        Span::styled(
            format!("{} plans", state.metrics.plans_total),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("{} active", state.metrics.ralphs_active),
            Style::default().fg(colors::RUNNING),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("{} complete", state.metrics.ralphs_complete),
            Style::default().fg(colors::COMPLETE),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("{} failed", state.metrics.ralphs_failed),
            Style::default().fg(colors::FAILED),
        ),
    ])])
    .block(Block::default().borders(Borders::ALL).title(" Status "));

    frame.render_widget(header, area);
}

/// Render dashboard layout (sidebar + main)
fn render_dashboard(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Main resource list
    render_resource_list(app, frame, chunks[0]);

    // Preview/detail pane
    render_preview(app, frame, chunks[1]);
}

/// Render split layout (two resource lists side by side)
fn render_split(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_resource_list(app, frame, chunks[0]);
    render_preview(app, frame, chunks[1]);
}

/// Render grid layout (2x2 views)
fn render_grid(app: &App, frame: &mut Frame, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);

    let bottom_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    // Show different resource types in each quadrant
    render_resource_list_for_view(app, frame, top_cols[0], ResourceView::Plans);
    render_resource_list_for_view(app, frame, top_cols[1], ResourceView::Specs);
    render_resource_list_for_view(app, frame, bottom_cols[0], ResourceView::Phases);
    render_resource_list_for_view(app, frame, bottom_cols[1], ResourceView::Ralphs);
}

/// Render focus layout (full-screen single view)
fn render_focus(app: &App, frame: &mut Frame, area: Rect) {
    render_resource_list(app, frame, area);
}

/// Render the current resource list
fn render_resource_list(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.state();
    render_resource_list_for_view(app, frame, area, state.current_view);
}

/// Render a specific resource list
fn render_resource_list_for_view(app: &App, frame: &mut Frame, area: Rect, view: ResourceView) {
    let state = app.state();
    let items = match view {
        ResourceView::Plans => &state.plans,
        ResourceView::Specs => &state.specs,
        ResourceView::Phases => &state.phases,
        ResourceView::Ralphs => &state.ralphs,
        _ => {
            // Analytics views have different rendering
            render_analytics_view(app, frame, area, view);
            return;
        }
    };

    let selection = state.selection.get(&view).cloned().unwrap_or_default();
    let is_current = view == state.current_view;

    // Filter items if filter is active
    let filtered_items: Vec<&ResourceItem> = items
        .iter()
        .filter(|item| {
            // Apply text filter
            if !state.filter.text.is_empty() && !item.name.to_lowercase().contains(&state.filter.text.to_lowercase()) {
                return false;
            }
            // Apply status filter
            if let Some(ref status) = state.filter.status_filter
                && item.status != *status
            {
                return false;
            }
            // Apply attention filter
            if state.filter.attention_only && !item.needs_attention {
                return false;
            }
            // Apply parent filter
            if let Some(ref parent) = selection.parent_filter
                && item.parent_id.as_ref() != Some(parent)
            {
                return false;
            }
            true
        })
        .collect();

    let list_items: Vec<ListItem> = filtered_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let status_color = status_color(&item.status);
            let icon = item.status_icon();

            let mut spans = vec![
                Span::styled(format!("{:>3} ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} ", icon), Style::default().fg(status_color)),
                Span::styled(
                    format!("{:<20} ", item.truncated_name(20)),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[{:^8}] ", item.resource_type),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(format!("{:<10}", item.status), Style::default().fg(status_color)),
            ];

            if let Some(iter) = item.iteration {
                spans.push(Span::styled(
                    format!(" iter:{:<3}", iter),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            if item.needs_attention {
                spans.push(Span::styled(" !", Style::default().fg(colors::BLOCKED)));
            }

            let line = Line::from(spans);

            if is_current && i == selection.selected_index {
                ListItem::new(line).style(Style::default().bg(colors::SELECTED_BG).fg(Color::White))
            } else {
                ListItem::new(line)
            }
        })
        .collect();

    // Build title with filter info
    let mut title = format!(" {} ", view.display_name());
    if is_current && !state.filter.text.is_empty() {
        title.push_str(&format!("[filter: {}] ", state.filter.text));
    }
    if selection.parent_filter.is_some() {
        title.push_str("[filtered by parent] ");
    }

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(if is_current { Style::default().fg(colors::HEADER) } else { Style::default() }),
        )
        .highlight_style(Style::default().bg(colors::SELECTED_BG));

    frame.render_widget(list, area);

    // Show empty state message
    if filtered_items.is_empty() {
        let message = if items.is_empty() {
            format!("No {} found.", view.display_name().to_lowercase())
        } else {
            "No items match current filter.".to_string()
        };

        let inner = area.inner(ratatui::layout::Margin {
            horizontal: 2,
            vertical: 2,
        });

        let empty = Paragraph::new(message)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(empty, inner);
    }
}

/// Render analytics views (metrics, costs, history, deps)
fn render_analytics_view(app: &App, frame: &mut Frame, area: Rect, view: ResourceView) {
    let state = app.state();

    let content = match view {
        ResourceView::Metrics => render_metrics_content(state),
        ResourceView::Costs => render_costs_content(state),
        ResourceView::History => render_history_content(state),
        ResourceView::Dependencies => render_deps_content(state),
        _ => vec![Line::from("Unknown view")],
    };

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", view.display_name())),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn render_metrics_content(state: &super::state::AppState) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("Success Rate: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{:.1}%", state.metrics_data.success_rate * 100.0)),
        ]),
        Line::from(vec![
            Span::styled("Avg Iterations: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{:.1}", state.metrics_data.avg_iterations_to_complete)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "By Status:",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )]),
        Line::from(format!("  Active: {}", state.metrics.ralphs_active)),
        Line::from(format!("  Complete: {}", state.metrics.ralphs_complete)),
        Line::from(format!("  Failed: {}", state.metrics.ralphs_failed)),
        Line::from(""),
        Line::from(vec![
            Span::styled("Total Iterations: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{}", state.metrics.total_iterations)),
        ]),
        Line::from(vec![
            Span::styled("Total API Calls: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{}", state.metrics.total_api_calls)),
        ]),
    ]
}

fn render_costs_content(state: &super::state::AppState) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("Total Cost: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("${:.2}", state.costs_data.total_cost_usd),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Token Breakdown:",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )]),
        Line::from(format!("  Input: {}", state.costs_data.token_breakdown.input_tokens)),
        Line::from(format!("  Output: {}", state.costs_data.token_breakdown.output_tokens)),
        Line::from(format!(
            "  Cache Read: {}",
            state.costs_data.token_breakdown.cache_read_tokens
        )),
        Line::from(format!(
            "  Cache Creation: {}",
            state.costs_data.token_breakdown.cache_creation_tokens
        )),
    ]
}

fn render_history_content(state: &super::state::AppState) -> Vec<Line<'static>> {
    if state.history_data.events.is_empty() {
        return vec![Line::from("No events recorded yet.")];
    }

    state
        .history_data
        .events
        .iter()
        .take(20)
        .map(|event| {
            Line::from(format!(
                "{}: {} - {}",
                event.event_type.display_name(),
                event.resource_name,
                event.details
            ))
        })
        .collect()
}

fn render_deps_content(_state: &super::state::AppState) -> Vec<Line<'static>> {
    vec![
        Line::from("Dependency graph visualization"),
        Line::from("(ASCII art representation would go here)"),
        Line::from(""),
        Line::from("Use arrow keys to navigate nodes."),
    ]
}

/// Render the preview/detail pane
fn render_preview(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.state();

    let content = if let Some(item) = state.selected_item() {
        vec![
            Line::from(vec![
                Span::styled("ID: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&item.id),
            ]),
            Line::from(vec![
                Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&item.name),
            ]),
            Line::from(vec![
                Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(&item.resource_type),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(&item.status, Style::default().fg(status_color(&item.status))),
            ]),
            if let Some(parent) = &item.parent_id {
                Line::from(vec![
                    Span::styled("Parent: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(parent),
                ])
            } else {
                Line::from("")
            },
            if let Some(iter) = item.iteration {
                Line::from(vec![
                    Span::styled("Iteration: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(iter.to_string()),
                ])
            } else {
                Line::from("")
            },
            Line::from(""),
            if let Some(progress) = &item.progress {
                Line::from(vec![
                    Span::styled("Progress: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(progress),
                ])
            } else {
                Line::from("")
            },
            if item.needs_attention {
                Line::from(vec![
                    Span::styled("! ", Style::default().fg(colors::BLOCKED)),
                    Span::styled(
                        item.attention_reason.as_deref().unwrap_or("Needs attention"),
                        Style::default().fg(colors::BLOCKED),
                    ),
                ])
            } else {
                Line::from("")
            },
        ]
    } else {
        vec![Line::from("No item selected")]
    };

    let preview = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title(" Preview "))
        .wrap(Wrap { trim: true });

    frame.render_widget(preview, area);
}

/// Render the footer bar (with command/search input)
fn render_footer(app: &App, frame: &mut Frame, area: Rect) {
    let state = app.state();

    let content = match &state.interaction_mode {
        InteractionMode::Search(text) => Line::from(vec![
            Span::styled("/", Style::default().fg(colors::KEYBIND)),
            Span::raw(text),
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
        ]),
        InteractionMode::Command(text) => Line::from(vec![
            Span::styled(":", Style::default().fg(colors::KEYBIND)),
            Span::raw(text),
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
        ]),
        _ => {
            // Show error message if present, otherwise show keybinds
            if let Some(ref error) = state.error_message {
                Line::from(vec![Span::styled(
                    format!("Error: {}", error),
                    Style::default().fg(colors::FAILED),
                )])
            } else {
                Line::from(vec![
                    Span::styled(" q", Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD)),
                    Span::raw(" Quit "),
                    Span::styled(" ?", Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD)),
                    Span::raw(" Help "),
                    Span::styled(" jk", Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD)),
                    Span::raw(" Navigate "),
                    Span::styled(
                        " Enter",
                        Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" Drill down "),
                    Span::styled(" /", Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD)),
                    Span::raw(" Search "),
                    Span::styled(" :", Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD)),
                    Span::raw(" Command "),
                    Span::styled(
                        format!(" Sort:{}", state.sort_order.display_name()),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])
            }
        }
    };

    let footer = Paragraph::new(content).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
}

/// Render help overlay
fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(70, 80, area);
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
            "Navigation",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("  j/↓ k/↑    ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Navigate up/down"),
        ]),
        Line::from(vec![
            Span::styled("  g          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Go to top"),
        ]),
        Line::from(vec![
            Span::styled("  G          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Go to bottom"),
        ]),
        Line::from(vec![
            Span::styled("  Enter      ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Drill down into selection"),
        ]),
        Line::from(vec![
            Span::styled("  Esc        ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Go back / Clear filter"),
        ]),
        Line::from(vec![
            Span::styled("  1-9        ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Quick jump to item"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Modes",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("  /          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Search/filter mode"),
        ]),
        Line::from(vec![
            Span::styled("  :          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Command mode"),
        ]),
        Line::from(vec![
            Span::styled("  ?          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Toggle help"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Layout",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("  d          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Dashboard layout"),
        ]),
        Line::from(vec![
            Span::styled("  Space      ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Cycle layout mode"),
        ]),
        Line::from(vec![
            Span::styled("  f          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Focus (full-screen) mode"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Actions",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("  r          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Refresh data"),
        ]),
        Line::from(vec![
            Span::styled("  s          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Cycle sort order"),
        ]),
        Line::from(vec![
            Span::styled("  p          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Pause selected loop"),
        ]),
        Line::from(vec![
            Span::styled("  x          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Cancel selected loop"),
        ]),
        Line::from(vec![
            Span::styled("  R          ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Restart failed loop"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Commands (:)",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("  :plans     ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Show plans view"),
        ]),
        Line::from(vec![
            Span::styled("  :specs     ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Show specs view"),
        ]),
        Line::from(vec![
            Span::styled("  :ralphs    ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Show ralphs (loops) view"),
        ]),
        Line::from(vec![
            Span::styled("  :metrics   ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Show metrics view"),
        ]),
        Line::from(vec![
            Span::styled("  :q         ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Quit"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  q, Ctrl+c  ", Style::default().fg(colors::KEYBIND)),
            Span::raw("Quit application"),
        ]),
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
