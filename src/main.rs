use chrono::Local;
use crossterm::{
    ExecutableCommand,
    event::{self, Event as CEvent, KeyCode, KeyEventKind},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Gauge, List, ListItem, Paragraph, Row, Table, Tabs,
    },
};
use std::{io, time::Duration};
use tokio::sync::mpsc;

mod nats_config;
mod telemetry_source;
mod usage_tab;
use telemetry_source::{
    LiveOrganStatus, fetch_brain_context, fetch_heart_telemetry, fetch_spine_workflows,
    fetch_system_metrics, probe_organ_mesh,
};
use usage_tab::{ModelUsageRow, UsageViewData, render_usage_tab};

const CYBER_BLUE: Color = Color::Rgb(0, 229, 255);
const CYBER_GREEN: Color = Color::Rgb(0, 255, 157);
const CYBER_PURPLE: Color = Color::Rgb(179, 0, 255);
const CYBER_RED: Color = Color::Rgb(255, 0, 85);
const CYBER_YELLOW: Color = Color::Rgb(255, 204, 0);
const TEXT_MUTED: Color = Color::Rgb(136, 136, 153);
const BG_DARK: Color = Color::Rgb(13, 17, 23);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview = 0,
    Workflows = 1,
    Mesh = 2,
    Brain = 3,
    Logs = 4,
    Usage = 5,
}

impl Tab {
    #[allow(dead_code)]
    fn from_index(i: usize) -> Self {
        match i {
            0 => Tab::Overview,
            1 => Tab::Workflows,
            2 => Tab::Mesh,
            3 => Tab::Brain,
            4 => Tab::Logs,
            5 => Tab::Usage,
            _ => Tab::Overview,
        }
    }

    fn to_index(self) -> usize {
        self as usize
    }

    fn title(self) -> &'static str {
        match self {
            Tab::Overview => " 1: OVERVIEW ",
            Tab::Workflows => " 2: WORKFLOWS DAG ",
            Tab::Mesh => " 3: ORGAN MESH ",
            Tab::Brain => " 4: BRAIN CONTEXT ",
            Tab::Logs => " 5: EVENT STREAM ",
            Tab::Usage => " 6: USAGE & BUDGET ",
        }
    }

    fn next(self) -> Self {
        match self {
            Tab::Overview => Tab::Workflows,
            Tab::Workflows => Tab::Mesh,
            Tab::Mesh => Tab::Brain,
            Tab::Brain => Tab::Logs,
            Tab::Logs => Tab::Usage,
            Tab::Usage => Tab::Overview,
        }
    }

    fn prev(self) -> Self {
        match self {
            Tab::Overview => Tab::Usage,
            Tab::Workflows => Tab::Overview,
            Tab::Mesh => Tab::Workflows,
            Tab::Brain => Tab::Mesh,
            Tab::Logs => Tab::Brain,
            Tab::Usage => Tab::Logs,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OrganMeta {
    pub name: &'static str,
    pub role: &'static str,
    pub category: &'static str,
    pub status: &'static str,
    pub port: &'static str,
}

pub const DEFAULT_ORGANS: [OrganMeta; 11] = [
    OrganMeta {
        name: "agent-body",
        role: "Organ Supervisor / Gateway",
        category: "Core",
        status: "OPERATIONAL",
        port: "3200",
    },
    OrganMeta {
        name: "agent-brain",
        role: "Routing, Memory & MCP Federation",
        category: "Core",
        status: "OPERATIONAL",
        port: "7842",
    },
    OrganMeta {
        name: "agent-spine",
        role: "Deterministic YAML Workflow DAGs",
        category: "Execution",
        status: "OPERATIONAL",
        port: "7843",
    },
    OrganMeta {
        name: "agent-muscle",
        role: "Sandboxed Execution & Training",
        category: "Execution",
        status: "OPERATIONAL",
        port: "7844",
    },
    OrganMeta {
        name: "agent-heart",
        role: "Token Budget & Cost Guardrails",
        category: "Core",
        status: "OPERATIONAL",
        port: "7845",
    },
    OrganMeta {
        name: "agent-mouth",
        role: "AST Policy & Slack Approvals",
        category: "Interface",
        status: "OPERATIONAL",
        port: "7846",
    },
    OrganMeta {
        name: "agent-immune",
        role: "Security Scan & Sandbox Verify",
        category: "Core",
        status: "OPERATIONAL",
        port: "7847",
    },
    OrganMeta {
        name: "agent-nerves",
        role: "NATS / JetStream Event Bridge",
        category: "Core",
        status: "OPERATIONAL",
        port: "4222",
    },
    OrganMeta {
        name: "agent-eyes",
        role: "DOM Index & Visual VLM Describe",
        category: "Sensory",
        status: "OPERATIONAL",
        port: "7849",
    },
    OrganMeta {
        name: "agent-tui",
        role: "Mission Control Terminal HUD",
        category: "Interface",
        status: "OPERATIONAL",
        port: "LOCAL",
    },
    OrganMeta {
        name: "agent-ci-dashboard",
        role: "Avionics Web Mission Control",
        category: "Interface",
        status: "OPERATIONAL",
        port: "5173",
    },
];

#[derive(Debug)]
enum AppEvent {
    Input(KeyCode),
    Tick,
    NatsMsg { subject: String, payload: String },
}

struct AppState {
    active_tab: Tab,
    logs: Vec<String>,
    cpu: u16,
    mem: u16,
    total_mem_gb: u16,
    token_budget_used: u16,
    event_count: u64,
    workflows: Vec<String>,
    context: Vec<String>,
    organs: Vec<LiveOrganStatus>,
    running: bool,
    nats_connected: bool,
    nats_url: String,
    usage_data: UsageViewData,
    system_monitor: sysinfo::System,
}

impl AppState {
    fn new(nats_url: String) -> Self {
        let root = nats_config::autonomic_root();
        let mut sys = sysinfo::System::new();
        let sys_metrics = fetch_system_metrics(&mut sys);
        let organs = probe_organ_mesh(&DEFAULT_ORGANS);
        let usage_data = fetch_heart_telemetry(&root).unwrap_or_default();
        let budget_pct = if usage_data.monthly_ceiling_usd > 0.0 {
            ((usage_data.today_spend_usd / usage_data.monthly_ceiling_usd) * 100.0).min(100.0)
                as u16
        } else {
            0
        };
        let workflows = fetch_spine_workflows(&root);
        let context = fetch_brain_context(&root);

        Self {
            active_tab: Tab::Overview,
            logs: vec![],
            cpu: sys_metrics.cpu_pct,
            mem: sys_metrics.mem_mb,
            total_mem_gb: sys_metrics.total_mem_gb,
            token_budget_used: budget_pct,
            event_count: 0,
            workflows,
            context,
            organs,
            running: true,
            nats_connected: false,
            nats_url,
            usage_data,
            system_monitor: sys,
        }
    }

    fn refresh_system_metrics(&mut self) {
        let metrics = fetch_system_metrics(&mut self.system_monitor);
        self.cpu = metrics.cpu_pct;
        self.mem = metrics.mem_mb;
        self.total_mem_gb = metrics.total_mem_gb;
    }

    fn refresh_organ_mesh(&mut self) {
        self.organs = probe_organ_mesh(&DEFAULT_ORGANS);
    }

    fn refresh_all_telemetry(&mut self) {
        let root = nats_config::autonomic_root();
        self.refresh_system_metrics();
        self.refresh_organ_mesh();
        if let Some(usage) = fetch_heart_telemetry(&root) {
            if usage.monthly_ceiling_usd > 0.0 {
                self.token_budget_used =
                    ((usage.today_spend_usd / usage.monthly_ceiling_usd) * 100.0).min(100.0) as u16;
            }
            self.usage_data = usage;
        }
        self.workflows = fetch_spine_workflows(&root);
        self.context = fetch_brain_context(&root);
        self.push_log(
            "Manual refresh requested — synced organ telemetry, brain facts, and heart budget"
                .into(),
        );
    }

    fn refresh_local_cache_telemetry(&mut self) {
        let root = nats_config::autonomic_root();
        if let Some(usage) = fetch_heart_telemetry(&root) {
            if usage.monthly_ceiling_usd > 0.0 {
                self.token_budget_used =
                    ((usage.today_spend_usd / usage.monthly_ceiling_usd) * 100.0).min(100.0) as u16;
            }
            self.usage_data = usage;
        }
        self.workflows = fetch_spine_workflows(&root);
        self.context = fetch_brain_context(&root);
    }

    fn push_log(&mut self, log: String) {
        let ts = Local::now().format("%H:%M:%S");
        self.logs.push(format!("[{ts}] {log}"));
        if self.logs.len() > 200 {
            self.logs.remove(0);
        }
    }

    fn update_from_nats(&mut self, subject: &str, payload: &str) {
        self.event_count += 1;
        if subject == "events.heart.usage" {
            self.usage_data.today_calls += 1;
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(cost) = v.get("estimated_cost_usd").and_then(|c| c.as_f64()) {
                    self.usage_data.today_spend_usd += cost;
                }
                let model = v.get("model").and_then(|m| m.as_str()).unwrap_or("unknown");
                let provider = v
                    .get("provider")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                let prompt = v.get("prompt_tokens").and_then(|p| p.as_u64()).unwrap_or(0);
                let compl = v
                    .get("completion_tokens")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0);
                let cache = v
                    .get("cache_read_tokens")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0);
                let cost = v
                    .get("estimated_cost_usd")
                    .and_then(|c| c.as_f64())
                    .unwrap_or(0.0);
                let billing = v
                    .get("billing_type")
                    .and_then(|b| b.as_str())
                    .unwrap_or("metered_api");

                if let Some(row) = self
                    .usage_data
                    .model_breakdowns
                    .iter_mut()
                    .find(|r| r.model == model && r.provider == provider)
                {
                    row.calls += 1;
                    row.prompt_tokens += prompt;
                    row.completion_tokens += compl;
                    row.cache_read += cache;
                    row.estimated_cost_usd += cost;
                } else {
                    self.usage_data.model_breakdowns.push(ModelUsageRow {
                        provider: provider.to_string(),
                        model: model.to_string(),
                        calls: 1,
                        prompt_tokens: prompt,
                        completion_tokens: compl,
                        cache_read: cache,
                        estimated_cost_usd: cost,
                        billing_type: billing.to_string(),
                    });
                }
                if self.usage_data.monthly_ceiling_usd > 0.0 {
                    self.token_budget_used = ((self.usage_data.today_spend_usd
                        / self.usage_data.monthly_ceiling_usd)
                        * 100.0)
                        .min(100.0) as u16;
                }
            }
            self.push_log(format!("[USAGE] Recorded turn usage on {subject}"));
        } else if subject == "events.heart.quota" {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload)
                && let Some(pct) = v.get("utilization_pct").and_then(|u| u.as_f64())
            {
                self.usage_data.sub_claude_quota_pct = (pct as u16).min(100);
            }
            self.push_log("[QUOTA] Updated provider subscription window".into());
        } else if subject == "events.heart.budget.alert" {
            self.push_log(format!(
                "[ALERT] Budget alert threshold exceeded: {payload}"
            ));
        } else if subject.starts_with("events.spine") || subject.starts_with("autonomic.spine") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) {
                let wf_name = v
                    .get("workflow_name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("spine");
                let ev_type = v
                    .get("event_type")
                    .and_then(|e| e.as_str())
                    .unwrap_or("state");
                let node = v
                    .get("payload")
                    .and_then(|p| p.get("node_name"))
                    .and_then(|n| n.as_str());
                let desc = if let Some(n) = node {
                    format!("▶ @{wf_name} [{ev_type}: {n}]")
                } else {
                    format!("▶ @{wf_name} [{ev_type}]")
                };
                self.workflows.insert(0, desc);
            } else {
                self.workflows.insert(0, format!("▶ {payload}"));
            }
            self.workflows.truncate(30);
            self.push_log(format!("[SPINE] {subject}"));
        } else if subject.starts_with("events.brain") || subject.starts_with("autonomic.brain") {
            let single_line = payload.replace('\n', " ");
            let trimmed = single_line.trim();
            let shortened = if trimmed.chars().count() > 97 {
                let truncated: String = trimmed.chars().take(94).collect();
                format!("■ {truncated}...")
            } else {
                format!("■ {trimmed}")
            };
            self.context.insert(0, shortened);
            self.context.truncate(20);
            self.push_log(format!("[BRAIN] {subject}"));
        } else if subject.starts_with("events.muscle") || subject.starts_with("autonomic.compute") {
            self.push_log(format!("[MUSCLE] {payload}"));
        } else {
            self.push_log(format!("{subject} -> {payload}"));
        }
    }
}

pub fn handle_cli_args(args: &[String]) -> Option<String> {
    if args
        .iter()
        .any(|a| a == "--version" || a == "-v" || a == "-V")
    {
        Some(format!("agent-tui {}", env!("CARGO_PKG_VERSION")))
    } else if args.iter().any(|a| a == "--help" || a == "-h") {
        Some(format!(
            "agent-tui {}\nAutonomous 11-Organ HUD & Telemetry Mission Control\n\nUSAGE:\n    agent-tui [OPTIONS]\n\nOPTIONS:\n    -h, --help       Print help information\n    -v, --version    Print version information",
            env!("CARGO_PKG_VERSION")
        ))
    } else {
        None
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = stdout.execute(LeaveAlternateScreen);
        let _ = stdout.execute(crossterm::cursor::Show);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(msg) = handle_cli_args(&args) {
        println!("{msg}");
        return Ok(());
    }

    if !crossterm::tty::IsTty::is_tty(&std::io::stdin()) {
        eprintln!("Error: agent-tui requires an interactive terminal (TTY).");
        std::process::exit(1);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let nats_cfg = nats_config::resolve_nats_config();
    let mut app = AppState::new(nats_cfg.display_url.clone());
    let (tx, mut rx) = mpsc::channel(100);

    // Input Reader Thread
    let tx_in = tx.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(Duration::from_millis(50)).unwrap()
                && let CEvent::Key(key) = event::read().unwrap()
                && key.kind == KeyEventKind::Press
                && tx_in.send(AppEvent::Input(key.code)).await.is_err()
            {
                break;
            }
            if tx_in.send(AppEvent::Tick).await.is_err() {
                break;
            }
        }
    });

    // NATS Event Listener
    let tx_nats = tx.clone();
    let nats_target = nats_cfg.display_url.clone();
    let auth_desc = nats_cfg.auth_source.description();
    tokio::spawn(async move {
        match nats_cfg.connect().await {
            Ok(client) => {
                let _ = tx_nats
                    .send(AppEvent::NatsMsg {
                        subject: "system.info".into(),
                        payload: format!("Connected to NATS bus at {nats_target} ({auth_desc})"),
                    })
                    .await;

                let client_events = client.clone();
                let tx_events = tx_nats.clone();
                tokio::spawn(async move {
                    if let Ok(mut sub) = client_events.subscribe("events.>").await {
                        while let Some(msg) = sub.next().await {
                            let payload = String::from_utf8_lossy(&msg.payload).to_string();
                            let subject = msg.subject.to_string();
                            let _ = tx_events.send(AppEvent::NatsMsg { subject, payload }).await;
                        }
                    }
                });

                let tx_autonomic = tx_nats.clone();
                tokio::spawn(async move {
                    if let Ok(mut sub) = client.subscribe("autonomic.>").await {
                        while let Some(msg) = sub.next().await {
                            let payload = String::from_utf8_lossy(&msg.payload).to_string();
                            let subject = msg.subject.to_string();
                            let _ = tx_autonomic
                                .send(AppEvent::NatsMsg { subject, payload })
                                .await;
                        }
                    }
                });
            }
            Err(e) => {
                let _ = tx_nats
                    .send(AppEvent::NatsMsg {
                        subject: "system.warn".into(),
                        payload: format!(
                            "NATS unavailable ({e}) — operating in local telemetry cache"
                        ),
                    })
                    .await;
            }
        }
    });

    let mut tick_counter: u64 = 0;

    // Main App Event Loop
    while app.running {
        terminal.draw(|f| ui(f, &app))?;

        if let Some(evt) = rx.recv().await {
            match evt {
                AppEvent::Input(key) => match key {
                    KeyCode::Char('q') | KeyCode::Esc => app.running = false,
                    KeyCode::Char('1') => app.active_tab = Tab::Overview,
                    KeyCode::Char('2') => app.active_tab = Tab::Workflows,
                    KeyCode::Char('3') => app.active_tab = Tab::Mesh,
                    KeyCode::Char('4') => app.active_tab = Tab::Brain,
                    KeyCode::Char('5') => app.active_tab = Tab::Logs,
                    KeyCode::Char('6') => app.active_tab = Tab::Usage,
                    KeyCode::Tab | KeyCode::Right => app.active_tab = app.active_tab.next(),
                    KeyCode::BackTab | KeyCode::Left => app.active_tab = app.active_tab.prev(),
                    KeyCode::Char('r') => {
                        app.refresh_all_telemetry();
                    }
                    _ => {}
                },
                AppEvent::Tick => {
                    tick_counter = tick_counter.wrapping_add(1);
                    if tick_counter.is_multiple_of(20) {
                        app.refresh_system_metrics();
                        app.refresh_organ_mesh();
                    }
                    if !app.nats_connected && tick_counter.is_multiple_of(40) {
                        app.refresh_local_cache_telemetry();
                    }
                }
                AppEvent::NatsMsg { subject, payload } => {
                    if subject == "system.info" {
                        app.nats_connected = true;
                    }
                    app.update_from_nats(&subject, &payload);
                }
            }
        }
    }

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn ui(f: &mut ratatui::Frame, app: &AppState) {
    let size = f.area();
    f.render_widget(Block::default().style(Style::default().bg(BG_DARK)), size);

    // Top Header (2 lines) -> Tab Bar (3 lines) -> Main Content -> Footer (1 line)
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(1),
        ])
        .split(size);

    render_header(f, layout[0], app);
    render_tabs(f, layout[1], app);

    match app.active_tab {
        Tab::Overview => render_overview_tab(f, layout[2], app),
        Tab::Workflows => render_workflows_tab(f, layout[2], app),
        Tab::Mesh => render_mesh_tab(f, layout[2], app),
        Tab::Brain => render_brain_tab(f, layout[2], app),
        Tab::Logs => render_logs_tab(f, layout[2], app),
        Tab::Usage => render_usage_tab(f, layout[2], &app.usage_data),
    }

    render_footer(f, layout[3]);
}

fn render_header(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(34),
            Constraint::Min(20),
            Constraint::Length(36),
        ])
        .split(area);

    let logo = Paragraph::new(Line::from(vec![
        Span::styled(
            "⚡ AUTONOMIC TUI ",
            Style::default()
                .fg(Color::Black)
                .bg(CYBER_BLUE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            concat!(" v", env!("CARGO_PKG_VERSION")),
            Style::default().fg(CYBER_BLUE),
        ),
    ]));
    f.render_widget(logo, chunks[0]);

    let (nats_status, nats_color) = if app.nats_connected {
        (format!("NATS CONNECTED [{}]", app.nats_url), CYBER_GREEN)
    } else {
        (format!("NATS STANDALONE [{}]", app.nats_url), CYBER_YELLOW)
    };
    let online = app
        .organs
        .iter()
        .filter(|o| o.status == "OPERATIONAL")
        .count();
    let total = app.organs.len();
    let mesh_color = if online == total {
        CYBER_GREEN
    } else {
        CYBER_YELLOW
    };
    let center = Paragraph::new(Line::from(vec![
        Span::styled("MESH: ", Style::default().fg(TEXT_MUTED)),
        Span::styled(
            format!("{online}/{total} OPERATIONAL "),
            Style::default().fg(mesh_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled("• ", Style::default().fg(TEXT_MUTED)),
        Span::styled(nats_status, Style::default().fg(nats_color)),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(center, chunks[1]);

    let right = Paragraph::new(Line::from(vec![
        Span::styled("ARCH: ", Style::default().fg(TEXT_MUTED)),
        Span::styled("arm64-darwin ", Style::default().fg(CYBER_BLUE)),
        Span::styled("EVENTS: ", Style::default().fg(TEXT_MUTED)),
        Span::styled(
            format!("{}", app.event_count),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .alignment(Alignment::Right);
    f.render_widget(right, chunks[2]);
}

fn render_tabs(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let titles = [
        Tab::Overview.title(),
        Tab::Workflows.title(),
        Tab::Mesh.title(),
        Tab::Brain.title(),
        Tab::Logs.title(),
        Tab::Usage.title(),
    ]
    .iter()
    .map(|t| Line::from(Span::raw(*t)))
    .collect::<Vec<_>>();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(50, 65, 85))),
        )
        .select(app.active_tab.to_index())
        .style(Style::default().fg(TEXT_MUTED))
        .highlight_style(
            Style::default()
                .fg(CYBER_BLUE)
                .bg(Color::Rgb(22, 27, 34))
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, area);
}

fn render_overview_tab(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let main_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Min(6),
        ])
        .split(main_split[0]);

    // Gauges
    let cpu_color = if app.cpu > 80 { CYBER_RED } else { CYBER_BLUE };
    let cpu_gauge = Gauge::default()
        .block(
            Block::default()
                .title(" CPU Load ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(cpu_color)),
        )
        .gauge_style(Style::default().fg(cpu_color).bg(Color::Rgb(30, 35, 45)))
        .percent(app.cpu)
        .label(format!("{}%", app.cpu));
    f.render_widget(cpu_gauge, left[0]);

    let total_mem_mb = (app.total_mem_gb as f32 * 1024.0).max(1.0);
    let mem_percent = ((app.mem as f32 / total_mem_mb) * 100.0).min(100.0) as u16;
    let mem_gauge = Gauge::default()
        .block(
            Block::default()
                .title(" Memory (MB) ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(CYBER_PURPLE)),
        )
        .gauge_style(Style::default().fg(CYBER_PURPLE).bg(Color::Rgb(30, 35, 45)))
        .percent(mem_percent)
        .label(format!("{} MB / {} GB", app.mem, app.total_mem_gb));
    f.render_widget(mem_gauge, left[1]);

    let budget_color = if app.token_budget_used > 85 {
        CYBER_RED
    } else {
        CYBER_GREEN
    };
    let budget_label = format!(
        "{}% Budget Consumed (${:.2} / ${:.2})",
        app.token_budget_used, app.usage_data.today_spend_usd, app.usage_data.monthly_ceiling_usd
    );
    let budget_gauge = Gauge::default()
        .block(
            Block::default()
                .title(" Token Cost Guardrail (Heart) ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(budget_color)),
        )
        .gauge_style(Style::default().fg(budget_color).bg(Color::Rgb(30, 35, 45)))
        .percent(app.token_budget_used)
        .label(budget_label);
    f.render_widget(budget_gauge, left[2]);

    // Mesh Quick Status
    let online = app
        .organs
        .iter()
        .filter(|o| o.status == "OPERATIONAL")
        .count();
    let total_organs = app.organs.len();
    let faults = total_organs.saturating_sub(online);
    let (online_color, fault_text, fault_color) = if faults == 0 {
        (CYBER_GREEN, "0 faults (Nominal)".to_string(), CYBER_BLUE)
    } else {
        (
            CYBER_YELLOW,
            format!("{faults} offline (Standby)"),
            CYBER_YELLOW,
        )
    };
    let stream_text = if app.nats_connected {
        "autonomic.> & events.> (live)"
    } else {
        "local telemetry cache"
    };

    let mesh_summary = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Active Organs: ", Style::default().fg(TEXT_MUTED)),
            Span::styled(
                format!("{online} / {total_organs} Online"),
                Style::default()
                    .fg(online_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Fault Tolerance: ", Style::default().fg(TEXT_MUTED)),
            Span::styled(fault_text, Style::default().fg(fault_color)),
        ]),
        Line::from(vec![
            Span::styled("Nerves Stream: ", Style::default().fg(TEXT_MUTED)),
            Span::styled(stream_text, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Signing: ", Style::default().fg(TEXT_MUTED)),
            Span::styled(
                "Apple Developer ID [ENFORCED]",
                Style::default().fg(CYBER_GREEN),
            ),
        ]),
    ])
    .block(
        Block::default()
            .title(" Organ Mesh Status ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(50, 65, 85))),
    );
    f.render_widget(mesh_summary, left[3]);

    // Right Split: DAG Workflows + Recent Activity
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_split[1]);

    let wf_items: Vec<ListItem> = app
        .workflows
        .iter()
        .take(8)
        .enumerate()
        .map(|(i, w)| {
            let color = if i == 0 { CYBER_GREEN } else { Color::White };
            ListItem::new(w.as_str()).style(Style::default().fg(color))
        })
        .collect();
    let wf_list = List::new(wf_items).block(
        Block::default()
            .title(" Active Spine DAG Workflows ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CYBER_GREEN)),
    );
    f.render_widget(wf_list, right[0]);

    let log_items: Vec<ListItem> = app
        .logs
        .iter()
        .rev()
        .take(10)
        .map(|l| ListItem::new(l.as_str()).style(Style::default().fg(CYBER_BLUE)))
        .collect();
    let log_list = List::new(log_items).block(
        Block::default()
            .title(" Recent Event Telemetry ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CYBER_BLUE)),
    );
    f.render_widget(log_list, right[1]);
}

fn render_workflows_tab(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(8)])
        .split(area);

    let intro = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Deterministic YAML Workflow DAG Executor (agent-spine)", Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("Spine executes DAG workflows with topological sorting, approval gates, and state isolation."),
        Line::from("Supported commands: agent-spine run --meta <task> | agent-spine init --with @workflow"),
        Line::from(vec![
            Span::styled("Curated Packs: ", Style::default().fg(TEXT_MUTED)),
            Span::styled("@bugfix  @feature-from-spec  @release-notes  @stacked-pr  @mcp-health-check", Style::default().fg(CYBER_BLUE)),
        ]),
    ])
    .block(Block::default().title(" Spine DAG Engine ").borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(CYBER_GREEN)));
    f.render_widget(intro, chunks[0]);

    let rows = app.workflows.iter().map(|w| {
        let is_running = w.contains("RUNNING");
        let (status, color) = if is_running {
            ("RUNNING", CYBER_YELLOW)
        } else {
            ("SUCCESS", CYBER_GREEN)
        };
        Row::new(vec![
            Cell::from(w.as_str()).style(Style::default().fg(Color::White)),
            Cell::from(status).style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Cell::from("topological").style(Style::default().fg(TEXT_MUTED)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(60),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ],
    )
    .header(
        Row::new(vec!["Workflow Plan / Steps", "Status", "Execution Model"])
            .style(Style::default().fg(CYBER_BLUE).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .title(" Execution History & Active Pipeline DAGs ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(50, 65, 85))),
    );
    f.render_widget(table, chunks[1]);
}

fn render_mesh_tab(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let online = app
        .organs
        .iter()
        .filter(|o| o.status == "OPERATIONAL")
        .count();
    let total = app.organs.len();
    let border_color = if online == total {
        CYBER_GREEN
    } else {
        CYBER_YELLOW
    };

    let rows = app.organs.iter().map(|o| {
        let (status_color, status_text) = match o.status {
            "OPERATIONAL" => (CYBER_GREEN, "OPERATIONAL"),
            "STANDBY" => (TEXT_MUTED, "STANDBY"),
            _ => (CYBER_RED, "OFFLINE"),
        };
        let status_style = Style::default()
            .fg(status_color)
            .add_modifier(Modifier::BOLD);
        Row::new(vec![
            Cell::from(o.name).style(Style::default().fg(CYBER_BLUE).add_modifier(Modifier::BOLD)),
            Cell::from(o.role).style(Style::default().fg(Color::White)),
            Cell::from(o.category).style(Style::default().fg(CYBER_PURPLE)),
            Cell::from(o.port).style(Style::default().fg(CYBER_YELLOW)),
            Cell::from(status_text).style(status_style),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Percentage(38),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(16),
        ],
    )
    .header(
        Row::new(vec![
            "Organ Binary",
            "Responsibility / Role",
            "Domain",
            "Port",
            "Health",
        ])
        .style(Style::default().fg(CYBER_BLUE).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .title(format!(
                " Active Organ Mesh Matrix ({online} / {total} Online) "
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color)),
    );
    f.render_widget(table, area);
}

fn render_brain_tab(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(8)])
        .split(area);

    let brain_header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "agent-brain: ",
                Style::default()
                    .fg(CYBER_PURPLE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Unified MCP Router, Vector Memory & Skill Registry",
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from("SQLite durable memory store: ~/.autonomic/memory/brain.db"),
        Line::from(vec![
            Span::styled("Enforcement: ", Style::default().fg(TEXT_MUTED)),
            Span::styled(
                "route_task turn gate • store_memory task exit gate",
                Style::default().fg(CYBER_GREEN),
            ),
        ]),
    ])
    .block(
        Block::default()
            .title(" Neural Architecture & Policy ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CYBER_PURPLE)),
    );
    f.render_widget(brain_header, chunks[0]);

    let items: Vec<ListItem> = app
        .context
        .iter()
        .map(|c| ListItem::new(c.as_str()).style(Style::default().fg(Color::Rgb(224, 179, 255))))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Active Context Facts & Cross-Turn Memory ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(50, 65, 85))),
    );
    f.render_widget(list, chunks[1]);
}

fn render_logs_tab(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let items: Vec<ListItem> = app
        .logs
        .iter()
        .rev()
        .map(|l| {
            let color = if l.contains("ERR") || l.contains("fail") {
                CYBER_RED
            } else if l.contains("WARN") {
                CYBER_YELLOW
            } else if l.contains("CONNECTED") || l.contains("ready") {
                CYBER_GREEN
            } else {
                Color::Rgb(163, 190, 140)
            };
            ListItem::new(l.as_str()).style(Style::default().fg(color))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(format!(" Live NATS Event Stream ({}) ", app.logs.len()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CYBER_BLUE)),
    );
    f.render_widget(list, area);
}

fn render_footer(f: &mut ratatui::Frame, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            " [1-6] ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Switch Page  ", Style::default().fg(TEXT_MUTED)),
        Span::styled(
            " [Tab/Arrows] ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Cycle Tabs  ", Style::default().fg(TEXT_MUTED)),
        Span::styled(
            " [r] ",
            Style::default()
                .fg(Color::Black)
                .bg(CYBER_BLUE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Refresh  ", Style::default().fg(TEXT_MUTED)),
        Span::styled(
            " [q/Esc] ",
            Style::default()
                .fg(Color::Black)
                .bg(CYBER_RED)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Exit", Style::default().fg(TEXT_MUTED)),
    ]));
    f.render_widget(footer, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_cycling() {
        // Arrange
        let mut tab = Tab::Overview;

        // Act & Assert cycle forward
        tab = tab.next();
        assert_eq!(tab, Tab::Workflows);
        tab = tab.next();
        assert_eq!(tab, Tab::Mesh);
        tab = tab.next();
        assert_eq!(tab, Tab::Brain);
        tab = tab.next();
        assert_eq!(tab, Tab::Logs);
        tab = tab.next();
        assert_eq!(tab, Tab::Usage);
        tab = tab.next();
        assert_eq!(tab, Tab::Overview);

        // Act & Assert cycle backward
        tab = tab.prev();
        assert_eq!(tab, Tab::Usage);
        tab = tab.prev();
        assert_eq!(tab, Tab::Logs);
    }

    #[test]
    fn test_update_from_nats_usage_event() {
        // Arrange
        let mut state = AppState::new("nats://127.0.0.1:4222".into());
        let initial_spend = state.usage_data.today_spend_usd;
        let initial_calls = state.usage_data.today_calls;

        let usage_json = r#"{
            "id": "evt-test-1",
            "timestamp": 1700000000,
            "session_id": null,
            "organ": "agent-brain",
            "provider": "anthropic",
            "model": "claude-3-7-sonnet",
            "billing_type": "metered_api",
            "prompt_tokens": 10000,
            "completion_tokens": 2000,
            "cache_read_tokens": 5000,
            "cache_write_tokens": 0,
            "estimated_cost_usd": 0.0615,
            "latency_ms": 900
        }"#;

        // Act
        state.update_from_nats("events.heart.usage", usage_json);

        // Assert
        assert_eq!(state.usage_data.today_calls, initial_calls + 1);
        assert!((state.usage_data.today_spend_usd - (initial_spend + 0.0615)).abs() < 1e-4);
        assert!(
            state
                .usage_data
                .model_breakdowns
                .iter()
                .any(|m| m.model == "claude-3-7-sonnet")
        );
        assert_eq!(state.event_count, 1);
    }

    #[test]
    fn test_update_from_nats_quota_event() {
        // Arrange
        let mut state = AppState::new("nats://127.0.0.1:4222".into());

        let quota_json = r#"{
            "provider": "anthropic",
            "account_or_tier": "claude-code-sub",
            "window_type": "5h_rolling",
            "utilization_pct": 58.5,
            "reset_at": 1700007200,
            "rate_limit_remaining": 150000,
            "updated_at": 1700000000
        }"#;

        // Act
        state.update_from_nats("events.heart.quota", quota_json);

        // Assert
        assert_eq!(state.usage_data.sub_claude_quota_pct, 58);
    }

    #[test]
    fn test_derived_app_state_metrics() {
        let state = AppState::new("nats://127.0.0.1:4222".into());
        assert!(state.total_mem_gb >= 1);
        assert_eq!(state.organs.len(), 11);
        assert!(
            state
                .organs
                .iter()
                .any(|o| o.name == "agent-tui" && o.status == "OPERATIONAL")
        );
        assert!(!state.workflows.is_empty());
        assert!(!state.context.is_empty());
    }

    #[test]
    fn test_handle_cli_args() {
        let version_arg = vec!["agent-tui".to_string(), "--version".to_string()];
        let v_arg = vec!["agent-tui".to_string(), "-v".to_string()];
        let help_arg = vec!["agent-tui".to_string(), "--help".to_string()];
        let no_arg = vec!["agent-tui".to_string()];

        assert!(handle_cli_args(&version_arg).unwrap().contains("agent-tui"));
        assert!(handle_cli_args(&v_arg).unwrap().contains("agent-tui"));
        assert!(handle_cli_args(&help_arg).unwrap().contains("USAGE"));
        assert_eq!(handle_cli_args(&no_arg), None);
    }
}
