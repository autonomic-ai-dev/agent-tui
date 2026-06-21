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
    widgets::{Block, Borders, BorderType, Gauge, List, ListItem, Paragraph},
    Terminal,
};
use futures::StreamExt;
use chrono::Local;

const CYBER_BLUE: Color = Color::Rgb(0, 229, 255);
const CYBER_GREEN: Color = Color::Rgb(0, 255, 157);
const CYBER_PURPLE: Color = Color::Rgb(179, 0, 255);
const CYBER_RED: Color = Color::Rgb(255, 0, 85);
const TEXT_MUTED: Color = Color::Rgb(136, 136, 153);

#[derive(Debug)]
enum AppEvent {
    Input(KeyCode),
    Tick,
    NatsMsg { subject: String, payload: String },
}

struct AppState {
    logs: Vec<String>,
    cpu: u16,
    mem: u16,
    workflows: Vec<String>,
    context: Vec<String>,
    running: bool,
}

impl AppState {
    fn new() -> Self {
        Self {
            logs: vec![],
            cpu: 0,
            mem: 0,
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
            if let Some(cpu_str) = payload.split("CPU: ").nth(1).and_then(|s| s.split('%').next()) {
                if let Ok(c) = cpu_str.trim().parse::<u16>() {
                    self.cpu = c.min(100);
                }
            }
            if let Some(mem_str) = payload.split("MEM: ").nth(1).and_then(|s| s.split("MB").next()) {
                if let Ok(m) = mem_str.trim().parse::<u16>() {
                    self.mem = m;
                }
            }
        } else if subject.starts_with("events.spine") {
            self.workflows.insert(0, format!("▶ {}", payload));
            self.workflows.truncate(20);
        } else if subject.starts_with("events.brain") {
            self.context.insert(0, format!("■ {}", payload));
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

    // Main chunks: Title (1), Health (3), Middle (10+), Bottom (7)
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),    // Title
            Constraint::Length(3),    // Health
            Constraint::Min(10),      // Workflows + Logs
            Constraint::Length(7),    // Context
        ])
        .split(size);

    // Title
    let title = Paragraph::new(Line::from(vec![
        Span::styled(" AUTONOMIC AI ", Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" OBSERVABILITY DASHBOARD ", Style::default().fg(TEXT_MUTED)),
    ]));
    f.render_widget(title, main_chunks[0]);

    // Health Layout
    let health_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[1]);

    // CPU Gauge
    let cpu_color = if app.cpu > 80 { CYBER_RED } else { CYBER_BLUE };
    let cpu_gauge = Gauge::default()
        .block(Block::default().title(" System CPU ").borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(cpu_color)))
        .gauge_style(Style::default().fg(cpu_color).bg(Color::DarkGray))
        .percent(app.cpu)
        .label(format!("{}%", app.cpu));
    f.render_widget(cpu_gauge, health_chunks[0]);

    // Mem Gauge (calc percent based on 16GB max for demo)
    let mem_percent = ((app.mem as f32 / 16000.0) * 100.0).min(100.0) as u16;
    let mem_gauge = Gauge::default()
        .block(Block::default().title(" System Memory ").borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(CYBER_PURPLE)))
        .gauge_style(Style::default().fg(CYBER_PURPLE).bg(Color::DarkGray))
        .percent(mem_percent)
        .label(format!("{}MB", app.mem));
    f.render_widget(mem_gauge, health_chunks[1]);

    // Middle chunks: Left (Workflows), Right (Logs)
    let middle_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(main_chunks[2]);

    // Workflows Panel
    let wf_items: Vec<ListItem> = app.workflows
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let style = if i == 0 { Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD) } else { Style::default().fg(TEXT_MUTED) };
            ListItem::new(w.as_str()).style(style)
        })
        .collect();
    let wf_list = List::new(wf_items)
        .block(Block::default().title(" DAG Workflows ").borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(CYBER_GREEN)));
    f.render_widget(wf_list, middle_chunks[0]);

    // Logs Panel
    let log_items: Vec<ListItem> = app.logs
        .iter()
        .map(|l| ListItem::new(l.as_str()))
        .collect();
    let log_list = List::new(log_items)
        .block(Block::default().title(" Sandbox Logs ").borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(CYBER_BLUE)))
        .style(Style::default().fg(Color::Rgb(163, 190, 140))); // Fira Code aesthetic
    f.render_widget(log_list, middle_chunks[1]);

    // Context Panel
    let ctx_items: Vec<ListItem> = app.context
        .iter()
        .map(|c| ListItem::new(c.as_str()))
        .collect();
    let ctx_list = List::new(ctx_items)
        .block(Block::default().title(" Brain Context Retrieval ").borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(CYBER_PURPLE)))
        .style(Style::default().fg(Color::Rgb(224, 179, 255)));
    f.render_widget(ctx_list, main_chunks[3]);
}
