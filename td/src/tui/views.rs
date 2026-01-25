//! TUI views and rendering
//!
//! All rendering logic is contained here. The views module is responsible
//! for drawing the UI based on AppState, but never modifies state.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap};
use tracing::trace;

use super::state::{AppState, ConfirmDialog, DaemonStatus, InteractionMode, ReplMode, ReplRole, View};
use super::tree::LoopTree;

/// Status colors (k9s-inspired)
mod colors {
    use ratatui::style::Color;

    pub const RUNNING: Color = Color::Rgb(0, 255, 127); // Spring green
    pub const PENDING: Color = Color::Rgb(255, 215, 0); // Gold
    pub const COMPLETE: Color = Color::Rgb(50, 205, 50); // Lime green
    pub const FAILED: Color = Color::Rgb(220, 20, 60); // Crimson
    pub const BLOCKED: Color = Color::Rgb(255, 69, 0); // Orange red
    pub const DRAFT: Color = Color::Rgb(255, 255, 0); // Yellow - awaiting approval
    pub const HEADER: Color = Color::Rgb(0, 255, 255); // Cyan
    pub const KEYBIND: Color = Color::Rgb(0, 255, 255); // Cyan
    pub const SELECTED_BG: Color = Color::Rgb(40, 40, 40);
    pub const DIM: Color = Color::DarkGray;

    // REPL message colors
    pub const REPL_USER: Color = Color::Rgb(0, 255, 127); // Green
    pub const REPL_ASSISTANT: Color = Color::Rgb(100, 149, 237); // Cornflower blue
    pub const REPL_TOOL: Color = Color::Rgb(255, 215, 0); // Gold
    pub const REPL_ERROR: Color = Color::Rgb(220, 20, 60); // Crimson
}

/// Get color for a status string
fn status_color(status: &str) -> Color {
    trace!(%status, "status_color: called");
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
        "draft" => colors::DRAFT,
        _ => Color::Gray,
    }
}

/// Get status icon
fn status_icon(status: &str) -> &'static str {
    trace!(%status, "status_icon: called");
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
pub fn render(state: &mut AppState, frame: &mut Frame) {
    trace!(?state.current_view, "render: called");
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
        View::Repl => render_repl_view(state, frame, chunks[1]),
        View::Loops => render_loops_tree(state, frame, chunks[1]),
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

/// Render header with view tabs and metrics
fn render_header(state: &AppState, frame: &mut Frame, area: Rect) {
    trace!("render_header: called");
    // Daemon status indicator (colored dot before TaskDaemon)
    let (indicator, indicator_color) = match state.daemon_status {
        DaemonStatus::Connected => ("●", Color::Green),
        DaemonStatus::VersionMismatch => ("●", Color::Yellow),
        DaemonStatus::Disconnected => ("●", Color::Red),
    };

    // Build left side: indicator + TaskDaemon + view tabs
    let mut left_spans = vec![
        Span::raw(" "),
        Span::styled(indicator, Style::default().fg(indicator_color)),
        Span::styled(
            " TaskDaemon",
            Style::default().fg(colors::HEADER).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
    ];

    // First view tab: Chat|Plan (special handling)
    let is_repl_view = matches!(state.current_view, View::Repl);
    if is_repl_view {
        // Show Chat|Plan with active mode highlighted
        if state.repl_mode == ReplMode::Chat {
            left_spans.push(Span::styled(
                "Chat",
                Style::default().fg(colors::HEADER).add_modifier(Modifier::BOLD),
            ));
            left_spans.push(Span::styled("|Plan", Style::default().fg(colors::DIM)));
        } else {
            left_spans.push(Span::styled("Chat|", Style::default().fg(colors::DIM)));
            left_spans.push(Span::styled(
                "Plan",
                Style::default().fg(colors::HEADER).add_modifier(Modifier::BOLD),
            ));
        }
    } else {
        left_spans.push(Span::styled("Chat|Plan", Style::default().fg(colors::DIM)));
    }

    // Remaining view tab: Loops
    let other_tabs = [(
        "Loops",
        matches!(
            state.current_view,
            View::Loops | View::Executions | View::Records { .. }
        ),
    )];

    for (name, is_active) in other_tabs.iter() {
        left_spans.push(Span::styled(" · ", Style::default().fg(colors::DIM)));
        if *is_active {
            left_spans.push(Span::styled(
                *name,
                Style::default().fg(colors::HEADER).add_modifier(Modifier::BOLD),
            ));
        } else {
            left_spans.push(Span::styled(*name, Style::default().fg(colors::DIM)));
        }
    }

    // Add filter indicator if active
    if !state.filter_text.is_empty() {
        left_spans.push(Span::styled(" │ ", Style::default().fg(colors::DIM)));
        left_spans.push(Span::styled(
            format!("/{}", &state.filter_text),
            Style::default().fg(Color::Magenta),
        ));
    }

    // Build right side: metrics
    let mut right_parts: Vec<String> = Vec::new();

    // Session token stats (only show if there's been activity)
    if state.session_input_tokens > 0 || state.session_output_tokens > 0 {
        // Split input/output so they can have different colors
        right_parts.push(format!("↑{}", format_tokens(state.session_input_tokens)));
        right_parts.push(format!("↓{}", format_tokens(state.session_output_tokens)));
        // Show cost if significant
        if state.session_cost_usd >= 0.001 {
            right_parts.push(format!("${:.2}", state.session_cost_usd));
        }
    }

    // Only show daemon stats if there's actually something to show
    // (avoids cluttering Chat view with "0 records | 0 active")
    let has_daemon_activity = state.total_records > 0
        || state.executions_draft > 0
        || state.executions_active > 0
        || state.executions_complete > 0
        || state.executions_failed > 0;

    if has_daemon_activity {
        if state.executions_active > 0 {
            right_parts.push(format!("{} active", state.executions_active));
        }
        if state.executions_draft > 0 {
            right_parts.push(format!("{} drafts", state.executions_draft));
        }
        if state.executions_complete > 0 {
            right_parts.push(format!("{} done", state.executions_complete));
        }
        if state.executions_failed > 0 {
            right_parts.push(format!("{} failed", state.executions_failed));
        }
    }

    // Calculate widths for right-justification
    // Inner width = area width - 2 (borders)
    let inner_width = area.width.saturating_sub(2) as usize;
    let left_width: usize = left_spans.iter().map(|s| s.width()).sum();
    let right_text = right_parts.join(" │ ");
    let right_width = right_text.len() + 1; // +1 for trailing space

    // Calculate padding between left and right
    let padding = inner_width.saturating_sub(left_width + right_width);

    // Build the complete line
    let mut spans = left_spans;
    if padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
    }

    // Add right-side metrics with colors
    for (i, part) in right_parts.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(colors::DIM)));
        }
        let color = if part.contains("active") {
            colors::RUNNING
        } else if part.contains("drafts") {
            colors::DRAFT
        } else if part.contains("done") {
            colors::COMPLETE
        } else if part.contains("failed") {
            colors::FAILED
        } else if part.starts_with('↑') {
            Color::Green // Input tokens - cheap
        } else if part.starts_with('↓') {
            Color::Red // Output tokens - expensive
        } else if part.starts_with('$') {
            Color::Yellow // Cost
        } else {
            Color::White
        };
        spans.push(Span::styled(part.clone(), Style::default().fg(color)));
    }
    spans.push(Span::raw(" "));

    let header = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL));

    frame.render_widget(header, area);
}

/// Calculate the number of lines needed for wrapped input text
fn calculate_input_height(input: &str, width: u16) -> u16 {
    trace!(input_len = input.len(), width, "calculate_input_height: called");
    if input.is_empty() {
        return 1; // Minimum 1 line for empty input
    }

    // Account for "> " prefix (2 chars) and cursor "_" (1 char)
    let effective_width = width.saturating_sub(3) as usize;
    if effective_width == 0 {
        return 1;
    }

    // Count lines needed for wrapped content
    let lines = input.len().div_ceil(effective_width);
    lines.clamp(1, 10) as u16 // Cap at 10 lines max
}

/// Render REPL view with conversation history and input (unified single border)
fn render_repl_view(state: &mut AppState, frame: &mut Frame, area: Rect) {
    trace!(?state.repl_mode, "render_repl_view: called");
    // Title changes based on REPL mode
    let title = match state.repl_mode {
        ReplMode::Chat => " Chat ",
        ReplMode::Plan => " Plan ",
    };

    // Single outer border with title
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(colors::HEADER));

    let inner = block.inner(area);

    // Calculate input height dynamically based on content
    let input_height = calculate_input_height(&state.repl_input, inner.width);

    // Split inner area: history fills remaining, input at bottom
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),               // History (fills remaining)
            Constraint::Length(input_height), // Input (dynamic height)
        ])
        .split(inner);

    // Render the outer block first
    frame.render_widget(block, area);

    // Render history without borders
    render_repl_history(state, frame, chunks[0]);

    // Render input with wrapping, no borders
    render_repl_input(state, frame, chunks[1]);
}

/// Generate a tool-aware summary for collapsed tool output
fn tool_summary(tool_name: &str, content: &str) -> Option<String> {
    trace!(%tool_name, content_len = content.len(), "tool_summary: called");
    let line_count = content.lines().count();

    match tool_name {
        "grep" | "Grep" => Some(format!("Found {} lines", line_count)),
        "glob" | "Glob" => {
            let file_count = content.lines().filter(|l| !l.is_empty()).count();
            Some(format!("Found {} files", file_count))
        }
        "edit" | "Edit" => {
            // Count + and - lines for diff summary
            let added = content.lines().filter(|l| l.starts_with('+')).count();
            let removed = content.lines().filter(|l| l.starts_with('-')).count();
            if added > 0 || removed > 0 {
                Some(format!("Added {} lines, removed {} lines", added, removed))
            } else {
                None
            }
        }
        _ => None, // No summary, show content preview
    }
}

/// Render REPL history content (no borders)
fn render_repl_history(state: &mut AppState, frame: &mut Frame, area: Rect) {
    trace!(history_len = state.repl_history.len(), "render_repl_history: called");
    use super::state::{COLLAPSE_PREVIEW_LINES, COLLAPSE_THRESHOLD};

    let mut lines: Vec<Line> = Vec::new();

    for msg in &state.repl_history {
        match &msg.role {
            ReplRole::User => {
                // User messages: "> content"
                for (i, content_line) in msg.content.lines().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            Span::styled(
                                "> ",
                                Style::default().fg(colors::REPL_USER).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(content_line, Style::default().fg(colors::REPL_USER)),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(content_line, Style::default().fg(colors::REPL_USER)),
                        ]));
                    }
                }
            }
            ReplRole::Assistant => {
                // Assistant messages: "  content" (rendered as markdown)
                let markdown_text = tui_markdown::from_str(&msg.content);
                for line in markdown_text.lines.iter() {
                    let mut spans = vec![Span::raw("  ")];
                    spans.extend(line.spans.iter().cloned());
                    lines.push(Line::from(spans));
                }
            }
            ReplRole::ToolResult { tool_name } => {
                // Tool results: "● ToolName(args)" header, "└" content
                let args_str = msg.tool_args.as_deref().unwrap_or("");
                let header = if args_str.is_empty() {
                    format!("● {}()", tool_name)
                } else {
                    format!("● {}({})", tool_name, args_str)
                };

                // Header line
                lines.push(Line::from(vec![Span::styled(
                    header,
                    Style::default().fg(colors::REPL_TOOL),
                )]));

                let line_count = msg.line_count();
                let is_collapsible = line_count > COLLAPSE_THRESHOLD;
                let show_collapsed = is_collapsible && !msg.expanded;

                if show_collapsed {
                    // Check for tool-aware summary
                    if let Some(summary) = tool_summary(tool_name, &msg.content) {
                        // Show summary line
                        lines.push(Line::from(vec![
                            Span::styled("└ ", Style::default().fg(colors::DIM)),
                            Span::styled(summary, Style::default().fg(colors::DIM)),
                            Span::styled(" (ctrl+o to expand)", Style::default().fg(colors::DIM)),
                        ]));
                    } else {
                        // Show preview lines
                        for (i, content_line) in msg.content.lines().take(COLLAPSE_PREVIEW_LINES).enumerate() {
                            let prefix = if i == 0 { "└ " } else { "  " };
                            lines.push(Line::from(vec![
                                Span::styled(prefix, Style::default().fg(colors::DIM)),
                                Span::styled(content_line, Style::default().fg(colors::DIM)),
                            ]));
                        }
                        // Show collapse indicator
                        let hidden = line_count - COLLAPSE_PREVIEW_LINES;
                        lines.push(Line::from(vec![Span::styled(
                            format!("  … +{} lines (ctrl+o to expand)", hidden),
                            Style::default().fg(colors::DIM),
                        )]));
                    }
                } else {
                    // Show all content
                    for (i, content_line) in msg.content.lines().enumerate() {
                        let prefix = if i == 0 { "└ " } else { "  " };
                        lines.push(Line::from(vec![
                            Span::styled(prefix, Style::default().fg(colors::DIM)),
                            Span::styled(content_line, Style::default().fg(colors::DIM)),
                        ]));
                    }
                }
            }
            ReplRole::Error => {
                // Error messages: "! content"
                for (i, content_line) in msg.content.lines().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            Span::styled(
                                "! ",
                                Style::default().fg(colors::REPL_ERROR).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(content_line, Style::default().fg(colors::REPL_ERROR)),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(content_line, Style::default().fg(colors::REPL_ERROR)),
                        ]));
                    }
                }
            }
        }

        // Add blank line after each message for readability
        lines.push(Line::from(""));
    }

    // If streaming, show the response buffer with a cursor
    if state.repl_streaming {
        if !state.repl_response_buffer.is_empty() {
            for (i, content_line) in state.repl_response_buffer.lines().enumerate() {
                if i == 0 {
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default().fg(colors::REPL_ASSISTANT)),
                        Span::styled(content_line, Style::default().fg(Color::White)),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(content_line, Style::default().fg(Color::White)),
                    ]));
                }
            }
        }
        // Show rich streaming status (Claude Code style)
        // Format: "* Pondering... (ctrl+c to interrupt · 45s · ↑1.2K ↓...)"
        let word = if state.streaming_word.is_empty() { "Thinking" } else { &state.streaming_word };

        let elapsed = state
            .streaming_start
            .map(|t| format_streaming_duration(t.elapsed()))
            .unwrap_or_else(|| "0s".to_string());

        let input_str = state
            .streaming_input_tokens
            .map(|t| format!("↑{}", format_tokens(t)))
            .unwrap_or_else(|| "↑...".to_string());

        let output_str = state
            .streaming_output_tokens
            .map(|t| format!("↓{}", format_tokens(t)))
            .unwrap_or_else(|| "↓...".to_string());

        let status = format!(
            "* {}... (ctrl+c to interrupt · {} · {} {})",
            word, elapsed, input_str, output_str
        );

        lines.push(Line::from(vec![Span::styled(status, Style::default().fg(colors::DIM))]));
    }

    // Show welcome message if empty (varies by mode)
    if state.repl_history.is_empty() && !state.repl_streaming {
        trace!(
            "Showing welcome message: mode={:?}, lines_before={}, scroll={:?}",
            state.repl_mode,
            lines.len(),
            state.repl_scroll
        );
        let (welcome_title, welcome_desc) = match state.repl_mode {
            ReplMode::Chat => (
                "Welcome to TaskDaemon Chat",
                "Type a message and press Enter to chat with the AI assistant.",
            ),
            ReplMode::Plan => (
                "Welcome to TaskDaemon Plan",
                "Describe your goal, then type /create to generate a plan using the Rule of Five methodology (3-5 review passes for completeness, correctness, edge cases, architecture, and clarity).",
            ),
        };

        lines.push(Line::from(vec![Span::styled(
            welcome_title,
            Style::default().fg(colors::HEADER).add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            welcome_desc,
            Style::default().fg(colors::DIM),
        )]));
    }

    // Calculate scroll offset
    let viewport_height = area.height as usize;
    let viewport_width = area.width as usize;

    // Calculate visual line count (accounting for line wrapping)
    let content_height: usize = lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            if viewport_width == 0 || line_width == 0 {
                1
            } else {
                line_width.div_ceil(viewport_width)
            }
        })
        .sum();

    let max_scroll = content_height.saturating_sub(viewport_height);

    // Update cached max_scroll for scroll methods to use
    state.repl_max_scroll = max_scroll;

    // Use manual scroll if set, otherwise auto-scroll to bottom
    let scroll = state.repl_scroll.unwrap_or(max_scroll).min(max_scroll);

    let history = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));

    frame.render_widget(history, area);
}

/// Render REPL input with wrapping (no borders)
fn render_repl_input(state: &AppState, frame: &mut Frame, area: Rect) {
    trace!(
        input_len = state.repl_input.len(),
        streaming = state.repl_streaming,
        "render_repl_input: called"
    );
    let input_style = if state.repl_streaming {
        Style::default().fg(colors::DIM)
    } else {
        Style::default().fg(Color::White)
    };

    // Split input at cursor position for rendering cursor in the middle
    let cursor_pos = state.repl_cursor_pos.min(state.repl_input.len());
    let (before_cursor, after_cursor) = state.repl_input.split_at(cursor_pos);

    let mut spans = vec![Span::styled(
        "> ",
        Style::default().fg(colors::REPL_USER).add_modifier(Modifier::BOLD),
    )];

    // Text before cursor
    if !before_cursor.is_empty() {
        spans.push(Span::styled(before_cursor, input_style));
    }

    // Cursor (blinking underscore or block depending on whether there's text after)
    if !state.repl_streaming {
        if after_cursor.is_empty() {
            // Cursor at end - show blinking underscore
            spans.push(Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)));
        } else {
            // Cursor in middle - highlight the next character
            // Get the first character after cursor
            let mut chars = after_cursor.chars();
            if let Some(c) = chars.next() {
                spans.push(Span::styled(
                    c.to_string(),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::SLOW_BLINK),
                ));
                // Rest of the text after cursor
                let remaining: String = chars.collect();
                if !remaining.is_empty() {
                    spans.push(Span::styled(remaining, input_style));
                }
            }
        }
    } else {
        // Streaming - no cursor, just show the rest
        if !after_cursor.is_empty() {
            spans.push(Span::styled(after_cursor, input_style));
        }
    }

    let input_content = Line::from(spans);
    let input = Paragraph::new(input_content).wrap(Wrap { trim: false });

    frame.render_widget(input, area);
}

/// Render Records table (generic Loop records)
fn render_records_table(state: &AppState, frame: &mut Frame, area: Rect) {
    trace!("render_records_table: called");
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
    trace!("render_executions_table: called");
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
        Constraint::Percentage(50), // NAME - take more space for slug/title
        Constraint::Length(8),      // TYPE
        Constraint::Length(6),      // ITER
        Constraint::Length(10),     // STATUS
        Constraint::Length(10),     // DURATION
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
        render_empty_message(frame, area, "No running executions. Press [n] to create a new task.");
    }
}

/// Render hierarchical Loops tree view
fn render_loops_tree(state: &AppState, frame: &mut Frame, area: Rect) {
    trace!("render_loops_tree: called");
    let tree = &state.loops_tree;
    let selected_id = tree.selected_id();

    // Build lines for each visible node with tree graphics
    let lines: Vec<Line> = tree
        .visible_nodes()
        .iter()
        .map(|id| {
            let node = tree.get(id).expect("visible node should exist");
            let is_selected = selected_id.is_some_and(|sel| sel == id);

            // Build prefix with tree graphics
            let prefix = build_tree_prefix(tree, id, node.depth);

            // Expand/collapse indicator
            let expand_icon = if node.has_children() {
                if node.expanded { "▼ " } else { "▶ " }
            } else {
                "  "
            };

            // Status icon
            let status_icon_str = status_icon(&node.item.status);

            // Type indicator (for non-ralph levels)
            let type_indicator = match node.item.loop_type.as_str() {
                "plan" => "Plan: ",
                "spec" => "Spec: ",
                "phase" => "Phase: ",
                "ralph" => "",
                _ => "",
            };

            // Progress (e.g., "[2/5]" for non-leaf, "(iter 3/10)" for ralph)
            let progress = if node.item.loop_type == "ralph" {
                format!(" ({})", node.item.iteration)
            } else {
                format!(" {}", node.progress_string())
            };

            // Build the line
            let style = if is_selected {
                Style::default().bg(colors::SELECTED_BG)
            } else {
                Style::default()
            };

            let exec_status_color = status_color(&node.item.status);

            // Build base spans
            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(colors::DIM)),
                Span::styled(expand_icon, Style::default().fg(colors::DIM)),
                Span::styled(status_icon_str, Style::default().fg(exec_status_color)),
                Span::raw(" "),
                Span::styled(type_indicator, Style::default().fg(colors::DIM)),
                Span::styled(&node.item.name, style),
                Span::styled(progress, Style::default().fg(colors::DIM)),
            ];

            // Add artifact info if present (e.g., "→ plan.md ✓")
            if let Some(ref artifact_file) = node.item.artifact_file {
                // Extract just the filename from the path
                let filename = std::path::Path::new(artifact_file)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(artifact_file);

                // Get artifact status icon and color
                let (artifact_icon, artifact_color) = if let Some(ref status) = node.item.artifact_status {
                    (status_icon(status), status_color(status))
                } else {
                    ("○", Color::DarkGray)
                };

                spans.push(Span::styled(" → ", Style::default().fg(colors::DIM)));
                spans.push(Span::styled(format!("{} ", filename), Style::default().fg(Color::Cyan)));
                spans.push(Span::styled(artifact_icon, Style::default().fg(artifact_color)));
            }

            Line::from(spans)
        })
        .collect();

    // Calculate scroll offset for selection visibility
    let inner_height = area.height.saturating_sub(2) as usize; // -2 for borders
    let selected_idx = tree.selected_index().unwrap_or(0);
    let scroll_offset = if selected_idx >= state.loops_scroll + inner_height {
        selected_idx.saturating_sub(inner_height - 1)
    } else if selected_idx < state.loops_scroll {
        selected_idx
    } else {
        state.loops_scroll
    };

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Loops ({}) ", tree.len()))
                .border_style(Style::default().fg(colors::HEADER)),
        )
        .scroll((scroll_offset as u16, 0));

    frame.render_widget(paragraph, area);

    if tree.is_empty() {
        render_empty_message(
            frame,
            area,
            "No loops yet. Use the Plan pane (Tab) to create a new Plan.",
        );
    }
}

/// Build tree prefix with Unicode box-drawing characters
fn build_tree_prefix(tree: &LoopTree, id: &str, depth: usize) -> String {
    trace!(%id, depth, "build_tree_prefix: called");
    if depth == 0 {
        return String::new();
    }

    let mut prefix = String::new();

    // Get ancestor chain to determine which vertical lines to draw
    let node = tree.get(id).expect("node should exist");
    let mut ancestors: Vec<(String, bool)> = Vec::new();

    // Walk up the tree to get all ancestors
    let mut current_id = id.to_string();
    let mut current_node = node;

    while let Some(ref parent_id) = current_node.item.parent_id {
        let is_last = tree.is_last_child(&current_id);
        ancestors.push((current_id.clone(), is_last));

        if let Some(parent) = tree.get(parent_id) {
            current_id = parent_id.clone();
            current_node = parent;
        } else {
            break;
        }
    }

    ancestors.reverse();

    // Build prefix: for each ancestor level, draw │ if not last child, or space if last
    for (i, (_ancestor_id, is_last)) in ancestors.iter().enumerate() {
        if i == ancestors.len() - 1 {
            // This is the current node's connection
            if *is_last {
                prefix.push_str("└─");
            } else {
                prefix.push_str("├─");
            }
        } else {
            // This is an ancestor's vertical line
            if *is_last {
                prefix.push_str("  "); // No vertical line (ancestor was last child)
            } else {
                prefix.push_str("│ "); // Vertical continuation
            }
        }
    }

    prefix
}

/// Render Logs view
fn render_logs_view(state: &AppState, frame: &mut Frame, area: Rect) {
    trace!("render_logs_view: called");
    let target_id = if let View::Logs { target_id } = &state.current_view {
        target_id.clone()
    } else {
        return;
    };

    // First, add stored logs from completed iterations
    let mut display_lines: Vec<Line> = state
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

    // Add live streaming output for running executions
    if let Some(live_buf) = state.get_live_output(&target_id) {
        if !display_lines.is_empty() && !live_buf.content.is_empty() {
            display_lines.push(Line::from(""));
            display_lines.push(Line::from(Span::styled(
                "── Live Output ──",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            )));
        }
        for line in live_buf.content.lines() {
            display_lines.push(Line::from(vec![
                Span::styled("[live] ", Style::default().fg(Color::Green)),
                Span::raw(line.to_string()),
            ]));
        }
    }

    // Add cursor if following
    if state.logs_follow && !display_lines.is_empty() {
        display_lines.push(Line::from(Span::styled(
            "▌",
            Style::default().add_modifier(Modifier::SLOW_BLINK),
        )));
    }

    let follow_indicator = if state.logs_follow { " [following]" } else { "" };
    let has_live = state.get_live_output(&target_id).is_some();
    let live_indicator = if has_live { " [live]" } else { "" };
    let title = format!(
        " Logs: {}{}{} ",
        truncate_str(&target_id, 30),
        follow_indicator,
        live_indicator
    );

    let logs = Paragraph::new(display_lines.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(colors::HEADER)),
        )
        .wrap(Wrap { trim: false })
        .scroll((state.logs_scroll as u16, 0));

    frame.render_widget(logs, area);

    if display_lines.is_empty() {
        render_empty_message(frame, area, "No logs yet.");
    }
}

/// Render Describe view with scroll support
fn render_describe_view(state: &mut AppState, frame: &mut Frame, area: Rect) {
    trace!("render_describe_view: called");
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

        // For running executions, show live output status
        if data.status == "running" {
            if let Some(live_buf) = state.get_live_output(&data.id) {
                let line_count = live_buf.content.lines().count();
                let last_line = live_buf.content.lines().last().unwrap_or("").to_string();
                let truncated = if last_line.len() > 60 {
                    format!("{}...", &last_line[..60])
                } else {
                    last_line
                };
                lines.push(Line::from(vec![
                    Span::raw("  Live:      "),
                    Span::styled(format!("{} lines", line_count), Style::default().fg(Color::Green)),
                ]));
                if !truncated.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("  Latest:    "),
                        Span::styled(truncated, Style::default().fg(Color::Yellow)),
                    ]));
                }
            } else {
                lines.push(Line::from(vec![
                    Span::raw("  Live:      "),
                    Span::styled("waiting for output...", Style::default().fg(Color::DarkGray)),
                ]));
            }
        } else if !exec.progress.is_empty() {
            lines.push(Line::from(vec![Span::raw("  Progress:  "), Span::raw(&exec.progress)]));
        }
    }

    // Artifact section
    if data.artifact_path.is_some() || data.artifact_status.is_some() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Artifact:",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]));
        if let Some(ref path) = data.artifact_path {
            // Show the full artifact path
            lines.push(Line::from(vec![
                Span::raw("  Path:   "),
                Span::styled(path, Style::default().fg(Color::Cyan)),
            ]));
        }
        if let Some(ref status) = data.artifact_status {
            lines.push(Line::from(vec![
                Span::raw("  Status: "),
                Span::styled(
                    format!("{} {}", status_icon(status), status),
                    Style::default().fg(status_color(status)),
                ),
            ]));
        }
    }

    // Worktree section (show where the work is being done)
    if let Some(ref worktree) = data.worktree {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Worktree:",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]));
        lines.push(Line::from(vec![
            Span::raw("  Path:   "),
            Span::styled(worktree, Style::default().fg(Color::Cyan)),
        ]));
    }

    // Aggregate Metrics section (only show if there's activity)
    if data.total_input_tokens > 0 || data.total_output_tokens > 0 || data.total_duration_ms > 0 {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Metrics:",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]));
        let total_tokens = data.total_input_tokens + data.total_output_tokens;
        lines.push(Line::from(vec![
            Span::raw("  Tokens:   "),
            Span::styled(
                format_tokens(data.total_input_tokens),
                Style::default().fg(Color::Green),
            ),
            Span::raw(" in / "),
            Span::styled(format_tokens(data.total_output_tokens), Style::default().fg(Color::Red)),
            Span::raw(" out ("),
            Span::raw(format_tokens(total_tokens)),
            Span::raw(" total)"),
        ]));
        if data.total_duration_ms > 0 {
            lines.push(Line::from(vec![
                Span::raw("  Duration: "),
                Span::raw(format_duration_ms(data.total_duration_ms)),
            ]));
        }
    }

    // Show output or plan content based on toggle state
    if state.describe_show_output {
        // Output section
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Output:",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]));
        lines.push(Line::from(""));

        // For running executions, show live streaming output if available
        let live_output = if data.status == "running" {
            state.get_live_output(&data.id).map(|buf| buf.content.as_str())
        } else {
            None
        };

        if let Some(live_content) = live_output {
            // Show live streaming output
            if live_content.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    "(Waiting for output...)",
                    Style::default().fg(Color::Yellow),
                )]));
            } else {
                for line in live_content.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
        } else if let Some(ref output) = data.output {
            // Fall back to stored progress output
            for line in output.lines() {
                lines.push(Line::from(line.to_string()));
            }
        } else {
            lines.push(Line::from(vec![Span::styled(
                "(No output yet)",
                Style::default().fg(Color::DarkGray),
            )]));
        }
    } else if let Some(ref plan) = data.plan_content {
        // Plan content section (rendered as markdown)
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Plan Content:",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]));
        lines.push(Line::from(""));
        let markdown_text = tui_markdown::from_str(plan);
        for line in markdown_text.lines {
            lines.push(line.clone());
        }
    }

    let title = format!(" Describe: {} ", truncate_str(&data.title, 30));

    // Calculate viewport and content dimensions for scrolling
    let viewport_height = area.height.saturating_sub(2) as usize; // -2 for borders
    let viewport_width = area.width.saturating_sub(2) as usize; // -2 for borders

    // Calculate content height accounting for line wrapping
    let content_height: usize = lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            if viewport_width == 0 || line_width == 0 {
                1
            } else {
                line_width.div_ceil(viewport_width)
            }
        })
        .sum();

    let max_scroll = content_height.saturating_sub(viewport_height);
    state.describe_max_scroll = max_scroll;

    // Clamp scroll to valid range
    let scroll = state.describe_scroll.min(max_scroll);

    let describe = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(colors::HEADER)),
        )
        .wrap(Wrap { trim: true })
        .scroll((scroll as u16, 0));

    frame.render_widget(describe, area);
}

/// Render footer with context-sensitive keybinds
fn render_footer(state: &AppState, frame: &mut Frame, area: Rect) {
    trace!(?state.interaction_mode, "render_footer: called");
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
                    View::Repl => {
                        if state.repl_mode == ReplMode::Plan {
                            vec![("[Enter]", "Send"), ("/create", "Create Plan"), ("/clear", "Clear")]
                        } else {
                            vec![("[Enter]", "Send"), ("/clear", "Clear")]
                        }
                    }
                    View::Loops => vec![
                        ("[Enter]", "Describe"),
                        ("[s]", "State"),
                        ("[o]", "Output"),
                        ("[L]", "Logs"),
                        ("[x]", "Cancel"),
                    ],
                    View::Records { .. } => vec![
                        ("[Enter]", "Children"),
                        ("[d]", "Describe"),
                        ("[l]", "Logs"),
                        ("[Esc]", "Back"),
                    ],
                    View::Executions => vec![
                        ("[n]", "New Task"),
                        ("[d]", "Describe"),
                        ("[l]", "Logs"),
                        ("[x]", "Cancel"),
                        ("[D]", "Delete"),
                    ],
                    View::Logs { .. } => vec![("[Esc]", "Back"), ("[f]", "Follow")],
                    View::Describe { .. } => {
                        vec![("[Esc]", "Back"), ("[s]", "State"), ("[o]", "Output"), ("[l]", "Logs")]
                    }
                };

                // Left side: view-specific keybinds
                let mut left_spans = vec![Span::raw(" ")];
                for (key, action) in keybinds {
                    left_spans.push(Span::styled(
                        key,
                        Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD),
                    ));
                    left_spans.push(Span::raw(format!(" {} ", action)));
                }

                // Right side: Views, Help, Quit (left-justified grouping)
                let right_line = Line::from(vec![
                    Span::styled(
                        "[Tab]",
                        Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" Views "),
                    Span::styled("[?]", Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD)),
                    Span::raw(" Help "),
                    Span::styled("[q]", Style::default().fg(colors::KEYBIND).add_modifier(Modifier::BOLD)),
                    Span::raw(" Quit "),
                ]);

                // Render with layout split
                let footer_block = Block::default().borders(Borders::ALL);
                let inner = footer_block.inner(area);
                frame.render_widget(footer_block, area);

                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Min(0), Constraint::Length(right_line.width() as u16)])
                    .split(inner);

                let left_para = Paragraph::new(Line::from(left_spans));
                let right_para = Paragraph::new(right_line);

                frame.render_widget(left_para, chunks[0]);
                frame.render_widget(right_para, chunks[1]);
                return;
            }
        }
    };

    let footer = Paragraph::new(content).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
}

/// Render help overlay
fn render_help_overlay(frame: &mut Frame, area: Rect) {
    trace!("render_help_overlay: called");
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
        key_line("Tab", "Cycle views (Chat/Plan → Executions → Records)"),
        key_line(":", "Command mode (:records, :executions, :<type>)"),
        key_line("/", "Filter current view"),
        key_line("?", "Toggle help"),
        key_line("q", "Quit"),
        key_line("Esc", "Back / Clear filter"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Chat/Plan View",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        key_line("Enter", "Send message (Chat) or create plan (Plan)"),
        key_line("/create", "Create plan from conversation (Rule of Five)"),
        key_line("/clear", "Clear conversation history"),
        key_line("o", "Toggle tool output expand/collapse"),
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
        key_line("s", "Start draft (begin execution)"),
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
    trace!("render_confirm_dialog: called");
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
    trace!(%message, "render_empty_message: called");
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
    trace!(percent_x, percent_y, "centered_rect: called");
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
    trace!(s_len = s.len(), max_len, "truncate_str: called");
    if s.len() <= max_len { s } else { &s[..max_len] }
}

/// Format token count for display (e.g., "1.2K", "3.5M")
fn format_tokens(count: u64) -> String {
    trace!(count, "format_tokens: called");
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Format duration for display (e.g., "45s", "1m 15s")
fn format_streaming_duration(d: std::time::Duration) -> String {
    trace!(?d, "format_streaming_duration: called");
    let secs = d.as_secs();
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

/// Format duration from milliseconds for display (e.g., "45s", "1m 15s", "2h 30m")
fn format_duration_ms(ms: u64) -> String {
    trace!(ms, "format_duration_ms: called");
    let secs = ms / 1000;
    let mins = secs / 60;
    let hours = mins / 60;

    if hours > 0 {
        format!("{}h {}m", hours, mins % 60)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs % 60)
    } else if secs > 0 {
        format!("{}s", secs)
    } else {
        format!("{}ms", ms)
    }
}
