use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Table},
};

const CYBER_BLUE: Color = Color::Rgb(0, 229, 255);
const CYBER_GREEN: Color = Color::Rgb(0, 255, 157);
const CYBER_PURPLE: Color = Color::Rgb(179, 0, 255);
const CYBER_RED: Color = Color::Rgb(255, 0, 85);
const CYBER_YELLOW: Color = Color::Rgb(255, 204, 0);
const TEXT_MUTED: Color = Color::Rgb(136, 136, 153);

#[derive(Debug, Clone, PartialEq)]
pub struct ModelUsageRow {
    pub provider: String,
    pub model: String,
    pub calls: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read: u64,
    pub estimated_cost_usd: f64,
    pub billing_type: String,
}

#[derive(Debug, Clone)]
pub struct UsageViewData {
    pub today_spend_usd: f64,
    pub today_calls: u64,
    pub monthly_ceiling_usd: f64,
    pub burn_rate_usd: f64,
    pub sub_claude_quota_pct: u16,
    pub sub_claude_reset_str: String,
    pub sub_openai_tpm_pct: u16,
    pub model_breakdowns: Vec<ModelUsageRow>,
    pub cache_savings_pct: f64,
    pub cache_dollars_saved: f64,
}

impl Default for UsageViewData {
    fn default() -> Self {
        Self {
            today_spend_usd: 8.42,
            today_calls: 142,
            monthly_ceiling_usd: 100.0,
            burn_rate_usd: 3.12,
            sub_claude_quota_pct: 42,
            sub_claude_reset_str: "2h 14m (18:20 UTC)".to_string(),
            sub_openai_tpm_pct: 21,
            model_breakdowns: vec![
                ModelUsageRow {
                    provider: "Anthropic".into(),
                    model: "claude-3-7-sonnet".into(),
                    calls: 84,
                    prompt_tokens: 842_120,
                    completion_tokens: 114_200,
                    cache_read: 620_100,
                    estimated_cost_usd: 6.18,
                    billing_type: "Metered API".into(),
                },
                ModelUsageRow {
                    provider: "OpenAI".into(),
                    model: "gpt-4o".into(),
                    calls: 38,
                    prompt_tokens: 210_400,
                    completion_tokens: 42_800,
                    cache_read: 0,
                    estimated_cost_usd: 1.42,
                    billing_type: "Metered API".into(),
                },
                ModelUsageRow {
                    provider: "Anthropic".into(),
                    model: "claude-code-sub".into(),
                    calls: 20,
                    prompt_tokens: 640_000,
                    completion_tokens: 78_000,
                    cache_read: 410_000,
                    estimated_cost_usd: 0.00,
                    billing_type: "Subscription".into(),
                },
                ModelUsageRow {
                    provider: "Local".into(),
                    model: "deepseek-r1:8b".into(),
                    calls: 45,
                    prompt_tokens: 124_000,
                    completion_tokens: 32_100,
                    cache_read: 0,
                    estimated_cost_usd: 0.00,
                    billing_type: "Local Free".into(),
                },
            ],
            cache_savings_pct: 41.2,
            cache_dollars_saved: 4.80,
        }
    }
}

pub fn render_usage_tab(f: &mut ratatui::Frame, area: Rect, data: &UsageViewData) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Top 3 Cards
            Constraint::Min(10),   // Table
            Constraint::Length(4), // Forecast & Cache Banner
        ])
        .split(area);

    render_top_cards(f, main_layout[0], data);
    render_model_table(f, main_layout[1], data);
    render_forecast_banner(f, main_layout[2], data);
}

fn render_top_cards(f: &mut ratatui::Frame, area: Rect, data: &UsageViewData) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(area);

    // Card 1: API Spend Today
    let avg_turn = if data.today_calls > 0 {
        data.today_spend_usd / (data.today_calls as f64)
    } else {
        0.0
    };
    let spend_text = vec![
        Line::from(vec![
            Span::styled(
                format!("${:.2} ", data.today_spend_usd),
                Style::default()
                    .fg(CYBER_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("USD", Style::default().fg(TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled(format!("Across {} turn calls", data.today_calls), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(format!("Avg: ${:.3} / turn", avg_turn), Style::default().fg(TEXT_MUTED)),
        ]),
    ];
    let spend_card = Paragraph::new(spend_text).block(
        Block::default()
            .title(" 1. API SPEND TODAY ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CYBER_BLUE)),
    );
    f.render_widget(spend_card, cols[0]);

    // Card 2: Monthly Budget
    let remaining = (data.monthly_ceiling_usd - data.today_spend_usd).max(0.0);
    let budget_pct = ((data.today_spend_usd / data.monthly_ceiling_usd) * 100.0).min(100.0) as u16;
    let budget_color = if budget_pct > 90 {
        CYBER_RED
    } else if budget_pct > 75 {
        CYBER_YELLOW
    } else {
        CYBER_GREEN
    };

    let budget_header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                format!("${:.2} REMAINING ", remaining),
                Style::default().fg(budget_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("of ${:.2}", data.monthly_ceiling_usd), Style::default().fg(TEXT_MUTED)),
        ]),
    ]);
    let budget_gauge = Gauge::default()
        .gauge_style(Style::default().fg(budget_color).bg(Color::Rgb(30, 40, 50)))
        .percent(budget_pct)
        .label(format!("{}% Used", budget_pct));
    let burn_text = Paragraph::new(Line::from(vec![
        Span::styled(format!("Burn Rate: ${:.2} / day", data.burn_rate_usd), Style::default().fg(TEXT_MUTED)),
    ]));

    let budget_card_block = Block::default()
        .title(" 2. MONTHLY BUDGET ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(budget_color));
    f.render_widget(budget_card_block, cols[1]);

    let inner_col = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(2), Constraint::Length(1), Constraint::Length(2)])
        .split(cols[1]);
    f.render_widget(budget_header, inner_col[0]);
    f.render_widget(budget_gauge, inner_col[1]);
    f.render_widget(burn_text, inner_col[2]);

    // Card 3: Subscription Quota Windows
    let quota_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(2), Constraint::Length(2)])
        .split(cols[2]);

    let claude_gauge = Gauge::default()
        .block(Block::default().title(format!("Claude Code (5h) • Resets in {}", data.sub_claude_reset_str)))
        .gauge_style(Style::default().fg(CYBER_PURPLE).bg(Color::Rgb(30, 40, 50)))
        .percent(data.sub_claude_quota_pct);

    let openai_gauge = Gauge::default()
        .block(Block::default().title("OpenAI Tier 4 TPM Limit"))
        .gauge_style(Style::default().fg(CYBER_BLUE).bg(Color::Rgb(30, 40, 50)))
        .percent(data.sub_openai_tpm_pct);

    let quota_block = Block::default()
        .title(" 3. SUBSCRIPTION QUOTA ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CYBER_PURPLE));
    f.render_widget(quota_block, cols[2]);
    f.render_widget(claude_gauge, quota_chunks[0]);
    f.render_widget(openai_gauge, quota_chunks[1]);
}

fn format_num(n: u64) -> String {
    let s = n.to_string();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut out = String::new();
    for (i, c) in chars.into_iter().enumerate() {
        out.push(c);
        if (len - 1 - i) > 0 && (len - 1 - i).is_multiple_of(3) {
            out.push(',');
        }
    }
    out
}

fn render_model_table(f: &mut ratatui::Frame, area: Rect, data: &UsageViewData) {
    let header = Row::new(vec![
        Cell::from(Span::styled("PROVIDER", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))),
        Cell::from(Span::styled("MODEL", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))),
        Cell::from(Span::styled("CALLS", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))),
        Cell::from(Span::styled("PROMPT TOK", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))),
        Cell::from(Span::styled("COMPL TOK", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))),
        Cell::from(Span::styled("CACHE READ", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))),
        Cell::from(Span::styled("EST. COST", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))),
        Cell::from(Span::styled("BILLING TYPE", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))),
    ])
    .height(1)
    .bottom_margin(1);

    let rows: Vec<Row> = data
        .model_breakdowns
        .iter()
        .map(|m| {
            let cost_color = if m.estimated_cost_usd > 0.0 {
                CYBER_GREEN
            } else {
                TEXT_MUTED
            };
            let type_color = match m.billing_type.as_str() {
                "Subscription" => CYBER_PURPLE,
                "Local Free" => CYBER_BLUE,
                _ => Color::White,
            };
            Row::new(vec![
                Cell::from(m.provider.clone()),
                Cell::from(Span::styled(m.model.clone(), Style::default().fg(CYBER_BLUE))),
                Cell::from(format!("{}", m.calls)),
                Cell::from(format_num(m.prompt_tokens)),
                Cell::from(format_num(m.completion_tokens)),
                Cell::from(format_num(m.cache_read)),
                Cell::from(Span::styled(format!("${:.2}", m.estimated_cost_usd), Style::default().fg(cost_color))),
                Cell::from(Span::styled(m.billing_type.clone(), Style::default().fg(type_color))),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(12),
        Constraint::Length(20),
        Constraint::Length(8),
        Constraint::Length(14),
        Constraint::Length(14),
        Constraint::Length(14),
        Constraint::Length(12),
        Constraint::Min(14),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(" PROVIDER & MODEL BREAKDOWN ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(CYBER_BLUE)),
        );

    f.render_widget(table, area);
}

fn render_forecast_banner(f: &mut ratatui::Frame, area: Rect, data: &UsageViewData) {
    let projected_spend = data.today_spend_usd + (data.burn_rate_usd * 25.0);
    let status_str = if projected_spend <= data.monthly_ceiling_usd {
        format!("Projected Month-End: ${:.2} (WITHIN ${:.2} CEILING) │ Soft Alert Gate: 80% (${:.2})",
            projected_spend, data.monthly_ceiling_usd, data.monthly_ceiling_usd * 0.8)
    } else {
        format!("Projected Month-End: ${:.2} (OVER ${:.2} CEILING) │ CRITICAL ALERT TRIGGERED",
            projected_spend, data.monthly_ceiling_usd)
    };

    let text = vec![
        Line::from(vec![
            Span::styled(status_str, Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(" [▲ STABLE] ", Style::default().fg(Color::Black).bg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(" Token savings from brain cache deduplication: {:.1}% (estimated ~${:.2} saved this week)",
                    data.cache_savings_pct, data.cache_dollars_saved),
                Style::default().fg(Color::White),
            ),
        ]),
    ];

    let banner = Paragraph::new(text).block(
        Block::default()
            .title(" FORECAST & OPTIMIZATIONS ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CYBER_GREEN)),
    );
    f.render_widget(banner, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_usage_view_data() {
        // Arrange & Act
        let data = UsageViewData::default();

        // Assert
        assert_eq!(data.today_calls, 142);
        assert!((data.today_spend_usd - 8.42).abs() < 1e-4);
        assert_eq!(data.sub_claude_quota_pct, 42);
        assert_eq!(data.model_breakdowns.len(), 4);
    }
}
