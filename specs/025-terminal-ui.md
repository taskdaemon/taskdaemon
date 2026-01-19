# Spec: Terminal User Interface

**ID:** 025-terminal-ui
**Status:** Draft
**Dependencies:** [023-analytics-metrics, 009-execution-tracking]

## Summary

Create a ratatui-based terminal user interface (TUI) that provides interactive monitoring and control of the TaskDaemon system. The TUI should offer multiple views for different aspects of the system with intuitive navigation.

## Acceptance Criteria

1. **Core Views**
   - Chat view for interactive communication
   - Plan overview with hierarchy
   - Execution monitoring
   - Records/logs view
   - System metrics dashboard

2. **Navigation**
   - Keyboard shortcuts
   - Tab/pane switching
   - Contextual commands
   - Help system

3. **Real-time Updates**
   - Live data streaming
   - Smooth animations
   - Efficient rendering
   - Minimal flicker

4. **Interactivity**
   - Command input
   - Item selection
   - Filtering/searching
   - Action triggers

## Implementation Phases

### Phase 1: Framework Setup
- Ratatui integration
- Basic layout system
- Event handling
- State management

### Phase 2: Core Views
- Chat interface
- Plan tree view
- Execution list
- Log viewer

### Phase 3: Advanced Features
- Real-time updates
- Search/filter
- Keyboard shortcuts
- Context menus

### Phase 4: Polish
- Animations
- Theme support
- Performance optimization
- Help documentation

## Technical Details

### Module Structure
```
src/tui/
├── mod.rs
├── app.rs         # Main application state
├── ui.rs          # UI rendering
├── views/         # Individual views
│   ├── chat.rs
│   ├── plan.rs
│   ├── executions.rs
│   ├── records.rs
│   └── metrics.rs
├── components/    # Reusable components
│   ├── table.rs
│   ├── tree.rs
│   ├── input.rs
│   └── status.rs
├── events.rs      # Event handling
├── commands.rs    # Command processing
└── theme.rs       # Theming system
```

### Application State
```rust
pub struct App {
    pub current_view: ViewType,
    pub views: HashMap<ViewType, Box<dyn View>>,
    pub global_state: GlobalState,
    pub command_buffer: String,
    pub mode: AppMode,
    pub notifications: Vec<Notification>,
}

pub enum ViewType {
    Chat,
    Plans,
    Executions,
    Records,
    Metrics,
}

pub enum AppMode {
    Normal,
    Command,
    Search,
    Select,
}

pub struct GlobalState {
    pub selected_plan: Option<Uuid>,
    pub selected_execution: Option<Uuid>,
    pub filter: FilterState,
    pub connection: DaemonConnection,
}

#[async_trait]
pub trait View: Send {
    fn render(&mut self, f: &mut Frame, area: Rect, state: &GlobalState);
    async fn handle_event(&mut self, event: Event, state: &mut GlobalState) -> Result<(), Error>;
    fn get_help(&self) -> Vec<(String, String)>;
}
```

### UI Layout
```rust
pub fn render_app(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),      // Status bar
            Constraint::Min(0),         // Main content
            Constraint::Length(3),      // Command/status area
        ])
        .split(f.size());

    // Status bar
    render_status_bar(f, chunks[0], app);

    // Main view
    if let Some(view) = app.views.get_mut(&app.current_view) {
        view.render(f, chunks[1], &app.global_state);
    }

    // Command area
    render_command_area(f, chunks[2], app);
}

fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),  // View tabs
            Constraint::Percentage(60),  // Status info
            Constraint::Percentage(20),  // Connection status
        ])
        .split(area);

    // View tabs
    let tabs = vec!["Chat", "Plans", "Executions", "Records", "Metrics"];
    let tab_index = app.current_view as usize;
    let tabs_widget = Tabs::new(tabs)
        .select(tab_index)
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    f.render_widget(tabs_widget, chunks[0]);

    // Status info
    let status_text = format!("Mode: {:?} | Plans: {} | Active: {}",
        app.mode,
        app.global_state.plan_count,
        app.global_state.active_loops,
    );
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(status, chunks[1]);

    // Connection status
    let conn_status = match &app.global_state.connection {
        DaemonConnection::Connected => "● Connected",
        DaemonConnection::Disconnected => "○ Disconnected",
        DaemonConnection::Reconnecting => "◐ Reconnecting",
    };
    let conn_widget = Paragraph::new(conn_status)
        .style(Style::default().fg(match &app.global_state.connection {
            DaemonConnection::Connected => Color::Green,
            DaemonConnection::Disconnected => Color::Red,
            DaemonConnection::Reconnecting => Color::Yellow,
        }))
        .alignment(Alignment::Right);
    f.render_widget(conn_widget, chunks[2]);
}
```

### Chat View
```rust
pub struct ChatView {
    messages: Vec<ChatMessage>,
    input: String,
    scroll_offset: u16,
    selected_loop: Option<LoopId>,
}

impl View for ChatView {
    fn render(&mut self, f: &mut Frame, area: Rect, state: &GlobalState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),      // Messages
                Constraint::Length(3),   // Input
            ])
            .split(area);

        // Messages area
        let messages_area = chunks[0];
        let message_widgets: Vec<ListItem> = self.messages.iter()
            .map(|msg| {
                let style = match msg.role {
                    MessageRole::User => Style::default().fg(Color::Cyan),
                    MessageRole::Assistant => Style::default().fg(Color::Green),
                    MessageRole::System => Style::default().fg(Color::Gray),
                };

                let content = vec![
                    Spans::from(vec![
                        Span::styled(format!("[{}] ", msg.timestamp.format("%H:%M:%S")),
                            Style::default().fg(Color::DarkGray)),
                        Span::styled(&msg.sender, style.add_modifier(Modifier::BOLD)),
                    ]),
                    Spans::from(msg.content.clone()),
                ];

                ListItem::new(content)
            })
            .collect();

        let messages_list = List::new(message_widgets)
            .block(Block::default()
                .borders(Borders::ALL)
                .title("Chat")
                .border_style(Style::default().fg(Color::White)));

        f.render_stateful_widget(messages_list, messages_area, &mut self.list_state);

        // Input area
        let input_widget = Paragraph::new(self.input.as_str())
            .block(Block::default()
                .borders(Borders::ALL)
                .title("Input (Enter to send, Ctrl+C to cancel)")
                .border_style(Style::default().fg(
                    if state.mode == AppMode::Command { Color::Yellow } else { Color::White }
                )));
        f.render_widget(input_widget, chunks[1]);
    }

    async fn handle_event(&mut self, event: Event, state: &mut GlobalState) -> Result<(), Error> {
        match event {
            Event::Key(KeyEvent { code: KeyCode::Enter, .. }) => {
                if !self.input.is_empty() {
                    self.send_message(state).await?;
                }
            }
            Event::Key(KeyEvent { code: KeyCode::Char(c), .. }) => {
                self.input.push(c);
            }
            Event::Key(KeyEvent { code: KeyCode::Backspace, .. }) => {
                self.input.pop();
            }
            Event::Key(KeyEvent { code: KeyCode::Up, .. }) => {
                self.scroll_up();
            }
            Event::Key(KeyEvent { code: KeyCode::Down, .. }) => {
                self.scroll_down();
            }
            _ => {}
        }
        Ok(())
    }
}
```

### Plan View
```rust
pub struct PlanView {
    tree: TreeState<PlanNode>,
    details_panel: DetailsPanel,
    filter: String,
}

impl View for PlanView {
    fn render(&mut self, f: &mut Frame, area: Rect, state: &GlobalState) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40),  // Tree
                Constraint::Percentage(60),  // Details
            ])
            .split(area);

        // Plan tree
        self.render_tree(f, chunks[0]);

        // Details panel
        if let Some(selected) = self.tree.selected() {
            self.details_panel.render(f, chunks[1], selected);
        }
    }

    fn render_tree(&self, f: &mut Frame, area: Rect) {
        let items = self.build_tree_items();

        let tree_widget = Tree::new(items)
            .block(Block::default()
                .borders(Borders::ALL)
                .title("Plans")
                .border_style(Style::default().fg(Color::White)))
            .highlight_style(Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD));

        f.render_stateful_widget(tree_widget, area, &mut self.tree.clone());
    }
}
```

### Execution View
```rust
pub struct ExecutionView {
    executions: Vec<ExecutionInfo>,
    table_state: TableState,
    sort_column: SortColumn,
    filter: ExecutionFilter,
}

impl View for ExecutionView {
    fn render(&mut self, f: &mut Frame, area: Rect, state: &GlobalState) {
        let header = Row::new(vec![
            Cell::from("ID").style(Style::default().fg(Color::Yellow)),
            Cell::from("Type"),
            Cell::from("Status"),
            Cell::from("Started"),
            Cell::from("Duration"),
            Cell::from("Progress"),
        ]);

        let rows: Vec<Row> = self.executions.iter()
            .filter(|e| self.filter.matches(e))
            .map(|e| {
                let status_style = match e.status {
                    ExecutionStatus::Running => Style::default().fg(Color::Green),
                    ExecutionStatus::Completed => Style::default().fg(Color::Blue),
                    ExecutionStatus::Failed => Style::default().fg(Color::Red),
                    ExecutionStatus::Cancelled => Style::default().fg(Color::Yellow),
                    _ => Style::default(),
                };

                Row::new(vec![
                    Cell::from(e.id.to_string()[..8].to_string()),
                    Cell::from(e.loop_type.clone()),
                    Cell::from(e.status.to_string()).style(status_style),
                    Cell::from(e.started_at.format("%H:%M:%S").to_string()),
                    Cell::from(format_duration(e.duration)),
                    Cell::from(format!("{}/{}", e.current_iteration, e.max_iterations)),
                ])
            })
            .collect();

        let table = Table::new(rows)
            .header(header)
            .block(Block::default()
                .borders(Borders::ALL)
                .title("Executions")
                .border_style(Style::default().fg(Color::White)))
            .widths(&[
                Constraint::Length(10),
                Constraint::Length(15),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
            ])
            .highlight_style(Style::default().add_modifier(Modifier::BOLD))
            .highlight_symbol("► ");

        f.render_stateful_widget(table, area, &mut self.table_state);
    }
}
```

### Event Handling
```rust
pub struct EventHandler {
    rx: mpsc::Receiver<Event>,
    tick_rate: Duration,
}

impl EventHandler {
    pub async fn next(&mut self) -> Result<Event, Error> {
        tokio::select! {
            Some(event) = self.rx.recv() => Ok(event),
            _ = sleep(self.tick_rate) => Ok(Event::Tick),
        }
    }
}

pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Tick,
    DaemonUpdate(DaemonEvent),
}

pub async fn handle_app_event(app: &mut App, event: Event) -> Result<(), Error> {
    match event {
        Event::Key(key) => handle_key_event(app, key).await?,
        Event::DaemonUpdate(update) => handle_daemon_update(app, update).await?,
        Event::Tick => app.tick().await?,
        _ => {}
    }
    Ok(())
}

async fn handle_key_event(app: &mut App, key: KeyEvent) -> Result<(), Error> {
    match app.mode {
        AppMode::Normal => {
            match key.code {
                KeyCode::Tab => app.next_view(),
                KeyCode::BackTab => app.prev_view(),
                KeyCode::Char('q') => app.quit(),
                KeyCode::Char(':') => app.enter_command_mode(),
                KeyCode::Char('/') => app.enter_search_mode(),
                KeyCode::Char('?') => app.show_help(),
                _ => {
                    // Pass to current view
                    if let Some(view) = app.views.get_mut(&app.current_view) {
                        view.handle_event(Event::Key(key), &mut app.global_state).await?;
                    }
                }
            }
        }
        AppMode::Command => handle_command_mode_key(app, key).await?,
        AppMode::Search => handle_search_mode_key(app, key).await?,
        AppMode::Select => handle_select_mode_key(app, key).await?,
    }
    Ok(())
}
```

## Notes

- Use double buffering to prevent flicker
- Implement lazy loading for large data sets
- Consider supporting mouse interaction for better UX
- Provide customizable key bindings