use std::{io, time::Duration};
use tokio::sync::mpsc;
use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};
use futures::StreamExt;
use chrono::Local;

#[derive(Debug)]
enum AppEvent {
    Input(KeyCode),
    Tick,
    NatsMsg { subject: String, payload: String },
}

struct AppState {
    logs: Vec<String>,
    health: Vec<String>,
    workflows: Vec<String>,
    context: Vec<String>,
    running: bool,
}

impl AppState {
    fn new() -> Self {
        Self {
            logs: vec![],
            health: vec![],
            workflows: vec![],
            context: vec![],
            running: true,
        }
    }

    fn push_log(&mut self, log: String) {
        let ts = Local::now().format("%H:%M:%S");
        self.logs.push(format!("[{ts}] {log}"));
        if self.logs.len() > 100 {
            self.logs.remove(0);
        }
    }

    fn update_from_nats(&mut self, subject: &str, payload: &str) {
        if subject.starts_with("events.heart") {
            self.health.insert(0, format!("{} -> {}", subject, payload));
            self.health.truncate(10);
        } else if subject.starts_with("events.spine") {
            self.workflows.insert(0, format!("{} -> {}", subject, payload));
            self.workflows.truncate(20);
        } else if subject.starts_with("events.brain") {
            self.context.insert(0, format!("{} -> {}", subject, payload));
            self.context.truncate(10);
        } else if subject.starts_with("events.muscle") {
            self.push_log(format!("{}", payload));
        } else {
            self.push_log(format!("{} -> {}", subject, payload));
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup Terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // State & Channels
    let mut app = AppState::new();
    let (tx, mut rx) = mpsc::channel(100);

    // Input Thread
    let tx_in = tx.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(Duration::from_millis(50)).unwrap() {
                if let CEvent::Key(key) = event::read().unwrap() {
                    if key.kind == KeyEventKind::Press {
                        if tx_in.send(AppEvent::Input(key.code)).await.is_err() {
                            break;
                        }
                    }
                }
            }
            if tx_in.send(AppEvent::Tick).await.is_err() {
                break;
            }
        }
    });

    // NATS Thread
    let tx_nats = tx.clone();
    tokio::spawn(async move {
        // We try to connect; if it fails, we just send a mock message and backoff
        match async_nats::connect("127.0.0.1:4222").await {
            Ok(client) => {
                let _ = tx_nats.send(AppEvent::NatsMsg {
                    subject: "system.info".into(),
                    payload: "Connected to NATS at 127.0.0.1:4222".into(),
                }).await;

                if let Ok(mut sub) = client.subscribe("events.>").await {
                    while let Some(msg) = sub.next().await {
                        let payload = String::from_utf8_lossy(&msg.payload).to_string();
                        let subject = msg.subject.to_string();
                        let _ = tx_nats.send(AppEvent::NatsMsg { subject, payload }).await;
                    }
                }
            }
            Err(e) => {
                let _ = tx_nats.send(AppEvent::NatsMsg {
                    subject: "system.error".into(),
                    payload: format!("Failed to connect to NATS: {}", e),
                }).await;
            }
        }
    });

    // Main Loop
    while app.running {
        terminal.draw(|f| ui(f, &app))?;

        if let Some(evt) = rx.recv().await {
            match evt {
                AppEvent::Input(key) => {
                    if key == KeyCode::Char('q') || key == KeyCode::Esc {
                        app.running = false;
                    }
                }
                AppEvent::Tick => {}
                AppEvent::NatsMsg { subject, payload } => {
                    app.update_from_nats(&subject, &payload);
                }
            }
        }
    }

    // Teardown
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn ui(f: &mut ratatui::Frame, app: &AppState) {
    let size = f.area();

    // Main chunks: Top (Health), Middle (Workflows + Logs), Bottom (Context)
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),    // Health
            Constraint::Min(10),      // Workflows + Logs
            Constraint::Length(7),    // Context
        ])
        .split(size);

    // Middle chunks: Left (Workflows), Right (Logs)
    let middle_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(main_chunks[1]);

    // 1. Health Panel
    let health_text = app.health.iter().map(|s| Line::from(s.as_str())).collect::<Vec<_>>();
    let health_block = Paragraph::new(health_text)
        .block(Block::default().title(" Organ Health (events.heart.*) ").borders(Borders::ALL))
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(health_block, main_chunks[0]);

    // 2. Workflows Panel
    let wf_items: Vec<ListItem> = app.workflows
        .iter()
        .map(|w| ListItem::new(w.as_str()))
        .collect();
    let wf_list = List::new(wf_items)
        .block(Block::default().title(" DAG Workflows (events.spine.*) ").borders(Borders::ALL))
        .style(Style::default().fg(Color::Green));
    f.render_widget(wf_list, middle_chunks[0]);

    // 3. Logs Panel
    let log_items: Vec<ListItem> = app.logs
        .iter()
        .map(|l| ListItem::new(l.as_str()))
        .collect();
    let log_list = List::new(log_items)
        .block(Block::default().title(" Sandbox Logs (events.muscle.*) ").borders(Borders::ALL));
    f.render_widget(log_list, middle_chunks[1]);

    // 4. Context Panel
    let ctx_items: Vec<ListItem> = app.context
        .iter()
        .map(|c| ListItem::new(c.as_str()))
        .collect();
    let ctx_list = List::new(ctx_items)
        .block(Block::default().title(" Context Retrieval (events.brain.*) ").borders(Borders::ALL))
        .style(Style::default().fg(Color::Magenta));
    f.render_widget(ctx_list, main_chunks[2]);
}
