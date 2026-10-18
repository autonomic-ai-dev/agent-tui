use chrono::{Datelike, NaiveDate};
use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;
use sysinfo::System;

use crate::usage_tab::{ModelUsageRow, UsageViewData};

#[derive(Debug, Clone)]
pub struct SystemMetrics {
    pub cpu_pct: u16,
    pub mem_mb: u16,
    pub total_mem_gb: u16,
}

pub fn fetch_system_metrics(sys: &mut System) -> SystemMetrics {
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu = sys.global_cpu_usage().round().clamp(0.0, 100.0) as u16;
    let mem_mb = (sys.used_memory() / 1024 / 1024) as u16;
    let total_gb = ((sys.total_memory() as f64) / 1024.0 / 1024.0 / 1024.0).ceil() as u16;

    SystemMetrics {
        cpu_pct: cpu,
        mem_mb,
        total_mem_gb: total_gb.max(1),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveOrganStatus {
    pub name: &'static str,
    pub role: &'static str,
    pub category: &'static str,
    pub port: &'static str,
    pub status: &'static str,
}

fn load_supervisor_status(autonomic_root: &Path) -> Vec<(String, bool)> {
    let status_path = autonomic_root
        .join("state")
        .join("supervisor")
        .join("status.json");
    let mut procs = Vec::new();
    if let Ok(content) = fs::read_to_string(status_path)
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(arr) = v.as_array()
    {
        for item in arr {
            let name = item
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let running = item
                .get("running")
                .and_then(|r| r.as_bool())
                .unwrap_or(false);
            if !name.is_empty() {
                procs.push((name, running));
            }
        }
    }
    procs
}

fn is_organ_in_supervisor(supervisor: &[(String, bool)], organ_name: &str) -> bool {
    let short_name = organ_name.trim_start_matches("agent-");
    for (name, running) in supervisor {
        if (name == short_name || name == organ_name) && *running {
            return true;
        }
        if short_name == "nerves" && name == "nats" && *running {
            return true;
        }
    }
    false
}

fn check_tcp_port(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(30)).is_ok()
}

fn is_binary_on_path(name: &str) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    let common_paths = [
        format!("/opt/homebrew/bin/{name}"),
        format!("/usr/local/bin/{name}"),
        format!("{home}/.cargo/bin/{name}"),
        format!("{home}/.local/bin/{name}"),
        format!("/usr/bin/{name}"),
    ];
    for p in &common_paths {
        if Path::new(p).is_file() {
            return true;
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if dir.join(name).is_file() {
                return true;
            }
        }
    }
    false
}

fn is_organ_operational(
    meta: &crate::OrganMeta,
    supervisor: &[(String, bool)],
    root: &Path,
) -> bool {
    if is_organ_in_supervisor(supervisor, meta.name) {
        return true;
    }
    if let Ok(port) = meta.port.parse::<u16>()
        && check_tcp_port(port)
    {
        return true;
    }
    if meta.name == "agent-nerves" && check_tcp_port(3102) {
        return true;
    }
    if meta.name == "agent-spine" && check_tcp_port(3001) {
        return true;
    }
    if meta.name == "agent-brain" && root.join("memory").join("serve.sock").exists() {
        return true;
    }
    if meta.name == "agent-ci-dashboard"
        && (check_tcp_port(5173) || root.join("agent-ui").join("package.json").exists())
    {
        return true;
    }
    if (meta.name.starts_with("agent-") || meta.name == "nats-server")
        && is_binary_on_path(meta.name)
    {
        return true;
    }
    false
}

pub fn probe_organ_mesh(organs: &[crate::OrganMeta]) -> Vec<LiveOrganStatus> {
    let root = crate::nats_config::autonomic_root();
    let supervisor = load_supervisor_status(&root);

    organs
        .iter()
        .map(|meta| {
            let status = if meta.port == "LOCAL" {
                "OPERATIONAL"
            } else if meta.port == "STANDBY" {
                "STANDBY"
            } else if is_organ_operational(meta, &supervisor, &root) {
                "OPERATIONAL"
            } else {
                "OFFLINE"
            };

            LiveOrganStatus {
                name: meta.name,
                role: meta.role,
                category: meta.category,
                port: meta.port,
                status,
            }
        })
        .collect()
}

pub fn record_usage_event_to_heart_db(
    autonomic_root: &Path,
    payload: &serde_json::Value,
) -> rusqlite::Result<()> {
    let db_path = autonomic_root.join("state").join("heart").join("heart.db");
    if !db_path.exists() {
        return Ok(());
    }
    let conn = rusqlite::Connection::open(&db_path)?;
    let id = payload
        .get("id")
        .and_then(|i| i.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            format!(
                "evt-{}",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
            )
        });
    let timestamp = payload
        .get("timestamp")
        .and_then(|t| t.as_i64())
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let session_id = payload.get("session_id").and_then(|s| s.as_str());
    let organ = payload
        .get("organ")
        .and_then(|o| o.as_str())
        .unwrap_or("agent-tui");
    let provider = payload
        .get("provider")
        .and_then(|p| p.as_str())
        .unwrap_or("unknown");
    let model = payload
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown");
    let billing = payload
        .get("billing_type")
        .and_then(|b| b.as_str())
        .unwrap_or("metered_api");
    let prompt = payload
        .get("prompt_tokens")
        .and_then(|p| p.as_i64())
        .unwrap_or(0);
    let compl = payload
        .get("completion_tokens")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    let cache_r = payload
        .get("cache_read_tokens")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    let cache_w = payload
        .get("cache_write_tokens")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    let cost = payload
        .get("estimated_cost_usd")
        .and_then(|c| c.as_f64())
        .unwrap_or(0.0);
    let latency = payload
        .get("latency_ms")
        .and_then(|l| l.as_i64())
        .unwrap_or(0);

    conn.execute(
        "INSERT OR REPLACE INTO model_usage_events (
            id, timestamp, session_id, organ, provider, model, billing_type,
            prompt_tokens, completion_tokens, cache_read_tokens, cache_write_tokens,
            estimated_cost_usd, latency_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        rusqlite::params![
            id, timestamp, session_id, organ, provider, model, billing, prompt, compl, cache_r,
            cache_w, cost, latency
        ],
    )?;
    Ok(())
}

pub fn record_quota_event_to_heart_db(
    autonomic_root: &Path,
    payload: &serde_json::Value,
) -> rusqlite::Result<()> {
    let db_path = autonomic_root.join("state").join("heart").join("heart.db");
    if !db_path.exists() {
        return Ok(());
    }
    let conn = rusqlite::Connection::open(&db_path)?;
    let provider = payload
        .get("provider")
        .and_then(|p| p.as_str())
        .unwrap_or("unknown");
    let window_type = payload
        .get("window_type")
        .and_then(|w| w.as_str())
        .unwrap_or("5h");
    let account = payload
        .get("account_or_tier")
        .and_then(|a| a.as_str())
        .unwrap_or("default");
    let util_pct = payload
        .get("utilization_pct")
        .and_then(|u| u.as_f64())
        .unwrap_or(0.0);
    let reset_at = payload.get("reset_at").and_then(|r| r.as_i64());
    let rate_limit = payload.get("rate_limit_remaining").and_then(|r| r.as_i64());
    let updated_at = payload
        .get("updated_at")
        .and_then(|u| u.as_i64())
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

    conn.execute(
        "INSERT INTO provider_quotas (
            provider, window_type, account_or_tier, utilization_pct, reset_at,
            rate_limit_remaining, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(provider, window_type) DO UPDATE SET
            account_or_tier = excluded.account_or_tier,
            utilization_pct = excluded.utilization_pct,
            reset_at = excluded.reset_at,
            rate_limit_remaining = excluded.rate_limit_remaining,
            updated_at = excluded.updated_at",
        rusqlite::params![
            provider,
            window_type,
            account,
            util_pct,
            reset_at,
            rate_limit,
            updated_at
        ],
    )?;
    Ok(())
}

fn utc_midnight_ms(date: NaiveDate) -> i64 {
    date.and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0)
}

fn days_in_month(date: NaiveDate) -> u32 {
    let first = date.with_day(1).unwrap_or(date);
    let next_first = if date.month() == 12 {
        NaiveDate::from_ymd_opt(date.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1)
    };
    next_first
        .map(|next| (next - first).num_days() as u32)
        .unwrap_or(30)
}

fn monthly_forecast(month_spend: f64, day_of_month: u32, days_in_month: u32) -> (f64, f64) {
    let burn_rate = month_spend / f64::from(day_of_month.max(1));
    let days_remaining = f64::from(days_in_month.saturating_sub(day_of_month));
    (burn_rate, month_spend + burn_rate * days_remaining)
}

fn spend_since(conn: &rusqlite::Connection, start_ms: i64) -> (f64, u64) {
    conn.query_row(
        "SELECT COALESCE(SUM(estimated_cost_usd), 0.0), COUNT(*)
         FROM model_usage_events
         WHERE timestamp >= ?1",
        [start_ms],
        |row| Ok((row.get::<_, f64>(0)?, row.get::<_, u64>(1)?)),
    )
    .unwrap_or((0.0, 0))
}

pub fn fetch_heart_telemetry(autonomic_root: &Path) -> Option<UsageViewData> {
    let db_path = autonomic_root.join("state").join("heart").join("heart.db");
    if !db_path.exists() {
        return None;
    }

    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let _ = conn.busy_timeout(Duration::from_millis(250));

    let mut monthly_ceiling = 100.0;
    if let Ok(mut stmt) = conn.prepare("SELECT monthly_ceiling_usd FROM budget_policies LIMIT 1")
        && let Ok(val) = stmt.query_row([], |row| row.get::<_, f64>(0))
    {
        monthly_ceiling = val;
    }

    let now = chrono::Utc::now().date_naive();
    let (today_spend, today_calls) = spend_since(&conn, utc_midnight_ms(now));
    let (month_spend, _) = spend_since(&conn, utc_midnight_ms(now.with_day(1).unwrap_or(now)));
    let days_in_month = days_in_month(now);
    let (burn_rate, projected_month_end) = monthly_forecast(month_spend, now.day(), days_in_month);

    // Model breakdown
    let mut breakdowns = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT provider, model, COUNT(*),
                COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(completion_tokens), 0),
                COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(estimated_cost_usd), 0.0),
                billing_type
         FROM model_usage_events
         GROUP BY provider, model
         ORDER BY SUM(estimated_cost_usd) DESC, COUNT(*) DESC",
    ) && let Ok(rows) = stmt.query_map([], |row| {
        Ok(ModelUsageRow {
            provider: row.get(0)?,
            model: row.get(1)?,
            calls: row.get(2)?,
            prompt_tokens: row.get(3)?,
            completion_tokens: row.get(4)?,
            cache_read: row.get(5)?,
            estimated_cost_usd: row.get(6)?,
            billing_type: row.get(7)?,
        })
    }) {
        for row in rows.flatten() {
            breakdowns.push(row);
        }
    }

    // Quotas from provider_quotas
    let mut sub_claude_quota_pct: u16 = 0;
    let mut sub_claude_reset_str = "Standby".to_string();
    let mut sub_openai_tpm_pct: u16 = 0;

    if let Ok(mut stmt) = conn.prepare(
        "SELECT provider, window_type, utilization_pct, reset_at
         FROM provider_quotas",
    ) && let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
            row.get::<_, Option<i64>>(3)?,
        ))
    }) {
        for row in rows.flatten() {
            let (provider, window_type, util_pct, reset_at) = row;
            let p_lower = provider.to_ascii_lowercase();
            let w_lower = window_type.to_ascii_lowercase();

            if p_lower.contains("anthropic") || p_lower.contains("claude") || w_lower.contains("5h")
            {
                sub_claude_quota_pct = (util_pct as u16).min(100);
                if let Some(mut reset_ts) = reset_at {
                    if reset_ts < 100_000_000_000 {
                        reset_ts *= 1000;
                    }
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    if reset_ts > now_ms {
                        let diff_secs = (reset_ts - now_ms) / 1000;
                        let hours = diff_secs / 3600;
                        let mins = (diff_secs % 3600) / 60;
                        sub_claude_reset_str = if hours > 0 {
                            format!("{hours}h {mins}m")
                        } else {
                            format!("{mins}m")
                        };
                    } else {
                        sub_claude_reset_str = "Resetting...".to_string();
                    }
                } else {
                    sub_claude_reset_str = "Active".to_string();
                }
            } else if p_lower.contains("openai")
                || w_lower.contains("tpm")
                || w_lower.contains("rpm")
            {
                sub_openai_tpm_pct = (util_pct as u16).min(100);
            }
        }
    }

    // Cache savings calculation
    let mut total_prompt_tokens = 0u64;
    let mut total_cache_read_tokens = 0u64;
    for row in &breakdowns {
        total_prompt_tokens += row.prompt_tokens;
        total_cache_read_tokens += row.cache_read;
    }
    let total_in = total_prompt_tokens + total_cache_read_tokens;
    let cache_savings_pct = if total_in > 0 {
        ((total_cache_read_tokens as f64) / (total_in as f64)) * 100.0
    } else {
        0.0
    };
    let cache_dollars_saved = (total_cache_read_tokens as f64 / 1_000_000.0) * 2.25;

    Some(UsageViewData {
        today_spend_usd: today_spend,
        today_calls,
        month_spend_usd: month_spend,
        monthly_ceiling_usd: monthly_ceiling,
        burn_rate_usd: burn_rate,
        projected_month_end_usd: projected_month_end,
        sub_claude_quota_pct,
        sub_claude_reset_str,
        sub_openai_tpm_pct,
        model_breakdowns: breakdowns,
        cache_savings_pct,
        cache_dollars_saved,
    })
}

pub fn fetch_spine_workflows(autonomic_root: &Path) -> Vec<String> {
    let mut workflows = Vec::new();

    // 1. Try reading executions directory
    let exec_dir = autonomic_root.join("logs").join("spine").join("executions");
    if let Ok(entries) = fs::read_dir(exec_dir) {
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().is_some_and(|ext| {
                    ext == "json" && !e.file_name().to_string_lossy().contains(".dag.")
                })
            })
            .collect();
        files.sort_by_key(|f| {
            f.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        files.reverse();

        for file in files.into_iter().take(15) {
            if let Ok(content) = fs::read_to_string(file.path())
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
            {
                let wf_name = v
                    .get("workflow_name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("spine-workflow");
                let count = v
                    .get("snapshot_count")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0);
                workflows.push(format!("▶ @{wf_name} (snapshots: {count}) [COMPLETED]"));
            }
        }
    }

    // 2. If empty, check state.db
    if workflows.is_empty() {
        let db_path = autonomic_root.join("logs").join("spine").join("state.db");
        if db_path.exists()
            && let Ok(conn) = rusqlite::Connection::open_with_flags(
                &db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
        {
            let _ = conn.busy_timeout(Duration::from_millis(250));
            if let Ok(mut stmt) = conn.prepare(
                "SELECT DISTINCT execution_id, created_at FROM snapshots ORDER BY id DESC LIMIT 10",
            ) && let Ok(rows) = stmt.query_map([], |row| {
                Ok(format!(
                    "▶ @execution-{} [{}]",
                    &row.get::<_, String>(0)?[..8.min(row.get::<_, String>(0)?.len())],
                    row.get::<_, String>(1)?
                ))
            }) {
                for item in rows.flatten() {
                    workflows.push(item);
                }
            }
        }
    }

    if workflows.is_empty() {
        workflows.push("▶ No active DAGs (awaiting spine workflow execution)".into());
    }

    workflows
}

pub fn fetch_brain_context(autonomic_root: &Path) -> Vec<String> {
    let db_path = autonomic_root.join("memory").join("brain.db");
    if !db_path.exists() {
        return vec!["■ No active brain.db database found".into()];
    }

    let Ok(conn) = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return vec!["■ Connecting to brain.db...".into()];
    };
    let _ = conn.busy_timeout(Duration::from_millis(250));

    let mut facts = Vec::new();
    if let Ok(mut stmt) = conn.prepare("SELECT fact FROM facts ORDER BY rowid DESC LIMIT 20")
        && let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0))
    {
        for fact in rows.flatten() {
            let single_line = fact.replace('\n', " ");
            let trimmed = single_line.trim();
            if !trimmed.is_empty() {
                let shortened = if trimmed.chars().count() > 97 {
                    let truncated: String = trimmed.chars().take(94).collect();
                    format!("■ {truncated}...")
                } else {
                    format!("■ {trimmed}")
                };
                facts.push(shortened);
            }
        }
    }

    if facts.is_empty() {
        facts.push("■ No active memory facts in brain.db (store_memory empty)".into());
    }

    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_system_metrics() {
        let mut sys = System::new();
        let metrics = fetch_system_metrics(&mut sys);
        assert!(metrics.total_mem_gb >= 1);
    }

    #[test]
    fn test_probe_organ_mesh_standby_and_offline() {
        let meta = vec![
            crate::OrganMeta {
                name: "agent-custom",
                role: "Custom",
                category: "Custom",
                status: "UNKNOWN",
                port: "STANDBY",
            },
            crate::OrganMeta {
                name: "agent-offline",
                role: "Offline",
                category: "Worker",
                status: "UNKNOWN",
                port: "65432",
            },
        ];
        let statuses = probe_organ_mesh(&meta);
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].status, "STANDBY");
        assert_eq!(statuses[1].status, "OFFLINE");
    }

    #[test]
    fn test_fetch_heart_telemetry_reads_sqlite() {
        let temp_dir = std::env::temp_dir().join(format!("test_tui_heart_{}", uuid_test_str()));
        let db_dir = temp_dir.join("state").join("heart");
        let _ = fs::create_dir_all(&db_dir);
        let db_path = db_dir.join("heart.db");

        let conn = rusqlite::Connection::open(&db_path).expect("open test db");
        conn.execute_batch(
            r#"
            CREATE TABLE budget_policies (monthly_ceiling_usd REAL);
            INSERT INTO budget_policies VALUES (150.0);

            CREATE TABLE model_usage_events (
                timestamp INTEGER,
                provider TEXT,
                model TEXT,
                prompt_tokens INTEGER,
                completion_tokens INTEGER,
                cache_read_tokens INTEGER,
                estimated_cost_usd REAL,
                billing_type TEXT
            );
            "#,
        )
        .expect("create tables");

        let now_ms = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO model_usage_events VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                now_ms,
                "anthropic",
                "claude-3-7-sonnet",
                100,
                50,
                20,
                0.005,
                "metered_api"
            ],
        )
        .expect("insert event");

        let telemetry = fetch_heart_telemetry(&temp_dir).expect("fetch telemetry");
        assert_eq!(telemetry.monthly_ceiling_usd, 150.0);
        assert_eq!(telemetry.today_calls, 1);
        assert!((telemetry.today_spend_usd - 0.005).abs() < f64::EPSILON);
        assert_eq!(telemetry.model_breakdowns.len(), 1);
        assert_eq!(telemetry.model_breakdowns[0].model, "claude-3-7-sonnet");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    fn seed_heart_usage_db(name: &str, rows: &[(i64, f64)]) -> std::path::PathBuf {
        let temp_dir = std::env::temp_dir().join(format!("test_tui_{name}_{}", uuid_test_str()));
        let db_dir = temp_dir.join("state").join("heart");
        let _ = fs::create_dir_all(&db_dir);
        let conn = rusqlite::Connection::open(db_dir.join("heart.db")).expect("open test db");
        conn.execute_batch(
            r#"
            CREATE TABLE budget_policies (monthly_ceiling_usd REAL);
            INSERT INTO budget_policies VALUES (150.0);
            CREATE TABLE model_usage_events (
                timestamp INTEGER,
                provider TEXT,
                model TEXT,
                prompt_tokens INTEGER,
                completion_tokens INTEGER,
                cache_read_tokens INTEGER,
                estimated_cost_usd REAL,
                billing_type TEXT
            );
            "#,
        )
        .expect("create tables");
        for (timestamp, cost) in rows {
            conn.execute(
                "INSERT INTO model_usage_events VALUES (?, 'anthropic', 'claude-3-7-sonnet', 100, 50, 0, ?, 'metered_api')",
                rusqlite::params![timestamp, cost],
            )
            .expect("insert event");
        }
        temp_dir
    }

    #[test]
    fn test_fetch_heart_telemetry_today_excludes_older_events() {
        let stale_ms = 1_579_046_400_000;
        let temp_dir = seed_heart_usage_db("stale_only", &[(stale_ms, 1.0)]);

        let telemetry = fetch_heart_telemetry(&temp_dir).expect("fetch telemetry");
        assert_eq!(telemetry.today_calls, 0);
        assert_eq!(telemetry.today_spend_usd, 0.0);
        assert_eq!(telemetry.month_spend_usd, 0.0);
        assert_eq!(telemetry.projected_month_end_usd, 0.0);
        assert_eq!(telemetry.model_breakdowns.len(), 1);
        assert!((telemetry.model_breakdowns[0].estimated_cost_usd - 1.0).abs() < 1e-9);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_fetch_heart_telemetry_splits_today_month_and_all_time() {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let stale_ms = 1_579_046_400_000;
        let temp_dir = seed_heart_usage_db("today_month", &[(now_ms, 0.5), (stale_ms, 1.0)]);

        let telemetry = fetch_heart_telemetry(&temp_dir).expect("fetch telemetry");
        assert_eq!(telemetry.today_calls, 1);
        assert!((telemetry.today_spend_usd - 0.5).abs() < 1e-9);
        assert!((telemetry.month_spend_usd - 0.5).abs() < 1e-9);
        assert!(telemetry.burn_rate_usd > 0.0);
        assert!(telemetry.projected_month_end_usd >= telemetry.month_spend_usd);
        let all_time: f64 = telemetry
            .model_breakdowns
            .iter()
            .map(|r| r.estimated_cost_usd)
            .sum();
        assert!((all_time - 1.5).abs() < 1e-9);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_monthly_forecast_projects_remaining_days_at_average_burn() {
        let (burn_rate, projected) = monthly_forecast(10.0, 5, 30);
        assert!((burn_rate - 2.0).abs() < 1e-9);
        assert!((projected - 60.0).abs() < 1e-9);
    }

    #[test]
    fn test_days_in_month_handles_leap_year_and_december() {
        assert_eq!(
            days_in_month(NaiveDate::from_ymd_opt(2024, 2, 10).unwrap()),
            29
        );
        assert_eq!(
            days_in_month(NaiveDate::from_ymd_opt(2026, 12, 18).unwrap()),
            31
        );
    }

    #[test]
    fn test_fetch_spine_workflows_reads_executions_dir() {
        let temp_dir = std::env::temp_dir().join(format!("test_tui_spine_{}", uuid_test_str()));
        let exec_dir = temp_dir.join("logs").join("spine").join("executions");
        let _ = fs::create_dir_all(&exec_dir);

        let json = r#"{"workflow_name":"deploy-infra","snapshot_count":5}"#;
        fs::write(exec_dir.join("exec-123.json"), json).expect("write exec json");

        let workflows = fetch_spine_workflows(&temp_dir);
        assert_eq!(workflows.len(), 1);
        assert!(workflows[0].contains("@deploy-infra"));
        assert!(workflows[0].contains("snapshots: 5"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_fetch_spine_workflows_reads_sqlite_snapshots() {
        let temp_dir = std::env::temp_dir().join(format!("test_tui_spine_db_{}", uuid_test_str()));
        let db_dir = temp_dir.join("logs").join("spine");
        let _ = fs::create_dir_all(&db_dir);
        let db_path = db_dir.join("state.db");

        let conn = rusqlite::Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            r#"
            CREATE TABLE snapshots (
                id INTEGER PRIMARY KEY,
                execution_id TEXT,
                created_at TEXT
            );
            INSERT INTO snapshots VALUES (1, "exec-abc-987", "2026-10-18T00:00:00Z");
            "#,
        )
        .expect("exec");

        let workflows = fetch_spine_workflows(&temp_dir);
        assert_eq!(workflows.len(), 1);
        assert!(workflows[0].contains("exec-abc"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_fetch_brain_context_reads_facts() {
        let temp_dir = std::env::temp_dir().join(format!("test_tui_brain_{}", uuid_test_str()));
        let mem_dir = temp_dir.join("memory");
        let _ = fs::create_dir_all(&mem_dir);
        let db_path = mem_dir.join("brain.db");

        let conn = rusqlite::Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            r#"
            CREATE TABLE facts (fact TEXT);
            INSERT INTO facts VALUES ("Port 4222 is used by NATS JetStream mesh");
            "#,
        )
        .expect("exec");

        let facts = fetch_brain_context(&temp_dir);
        assert_eq!(facts.len(), 1);
        assert!(facts[0].contains("Port 4222 is used by NATS JetStream mesh"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_fetch_heart_telemetry_reads_quotas_and_cache_savings() {
        let temp_dir = std::env::temp_dir().join(format!("test_tui_quotas_{}", uuid_test_str()));
        let db_dir = temp_dir.join("state").join("heart");
        let _ = fs::create_dir_all(&db_dir);
        let db_path = db_dir.join("heart.db");

        let conn = rusqlite::Connection::open(&db_path).expect("open test db");
        conn.execute_batch(
            r#"
            CREATE TABLE budget_policies (monthly_ceiling_usd REAL);
            INSERT INTO budget_policies VALUES (100.0);

            CREATE TABLE model_usage_events (
                id TEXT PRIMARY KEY,
                timestamp INTEGER,
                session_id TEXT,
                organ TEXT,
                provider TEXT,
                model TEXT,
                prompt_tokens INTEGER,
                completion_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                estimated_cost_usd REAL,
                billing_type TEXT,
                latency_ms INTEGER
            );

            CREATE TABLE provider_quotas (
                provider TEXT NOT NULL,
                window_type TEXT NOT NULL,
                account_or_tier TEXT NOT NULL,
                utilization_pct REAL NOT NULL,
                reset_at INTEGER,
                rate_limit_remaining INTEGER,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (provider, window_type)
            );
            "#,
        )
        .expect("create tables");

        let now_ms = chrono::Utc::now().timestamp_millis();
        let future_reset = now_ms + (2 * 3600 + 30 * 60) * 1000; // 2h 30m in future

        conn.execute(
            "INSERT INTO provider_quotas VALUES ('anthropic', '5h', 'Claude Pro', 45.0, ?, NULL, ?)",
            rusqlite::params![future_reset, now_ms],
        )
        .expect("insert anthropic quota");

        conn.execute(
            "INSERT INTO provider_quotas VALUES ('openai', 'tpm', 'Tier 4', 33.0, NULL, 500000, ?)",
            rusqlite::params![now_ms],
        )
        .expect("insert openai quota");

        conn.execute(
            "INSERT INTO model_usage_events VALUES ('evt-1', ?, NULL, 'agent-body', 'anthropic', 'claude-3-7-sonnet', 2000, 500, 8000, 0, 0.02, 'subscription_quota', 120)",
            rusqlite::params![now_ms],
        )
        .expect("insert usage event");

        let telemetry = fetch_heart_telemetry(&temp_dir).expect("fetch telemetry");
        assert_eq!(telemetry.sub_claude_quota_pct, 45);
        assert!(telemetry.sub_claude_reset_str.contains("2h"));
        assert_eq!(telemetry.sub_openai_tpm_pct, 33);
        // Total in = 2000 prompt + 8000 cache = 10000. Cache savings = 8000 / 10000 = 80.0%
        assert!((telemetry.cache_savings_pct - 80.0).abs() < 1e-3);
        assert!(telemetry.cache_dollars_saved > 0.0);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_record_usage_and_quota_to_heart_db() {
        let temp_dir = std::env::temp_dir().join(format!("test_tui_record_{}", uuid_test_str()));
        let db_dir = temp_dir.join("state").join("heart");
        let _ = fs::create_dir_all(&db_dir);
        let db_path = db_dir.join("heart.db");

        let conn = rusqlite::Connection::open(&db_path).expect("open test db");
        conn.execute_batch(
            r#"
            CREATE TABLE budget_policies (monthly_ceiling_usd REAL);
            CREATE TABLE model_usage_events (
                id TEXT PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                session_id TEXT,
                organ TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                billing_type TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                cache_read_tokens INTEGER NOT NULL,
                cache_write_tokens INTEGER NOT NULL,
                estimated_cost_usd REAL NOT NULL,
                latency_ms INTEGER NOT NULL
            );
            CREATE TABLE provider_quotas (
                provider TEXT NOT NULL,
                window_type TEXT NOT NULL,
                account_or_tier TEXT NOT NULL,
                utilization_pct REAL NOT NULL,
                reset_at INTEGER,
                rate_limit_remaining INTEGER,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (provider, window_type)
            );
            "#,
        )
        .expect("create tables");

        let usage_json = serde_json::json!({
            "id": "evt-rec-1",
            "timestamp": 1792300000000i64,
            "organ": "agent-brain",
            "provider": "google",
            "model": "gemini-3.8-flash",
            "billing_type": "Local Free",
            "prompt_tokens": 1500,
            "completion_tokens": 300,
            "cache_read_tokens": 6000,
            "cache_write_tokens": 0,
            "estimated_cost_usd": 0.0,
            "latency_ms": 140
        });
        record_usage_event_to_heart_db(&temp_dir, &usage_json).expect("record usage");

        let quota_json = serde_json::json!({
            "provider": "anthropic",
            "window_type": "5h",
            "account_or_tier": "Claude Pro",
            "utilization_pct": 52.0,
            "reset_at": 1792350000000i64,
            "rate_limit_remaining": 120000
        });
        record_quota_event_to_heart_db(&temp_dir, &quota_json).expect("record quota");

        let telemetry = fetch_heart_telemetry(&temp_dir).expect("fetch telemetry");
        assert_eq!(telemetry.model_breakdowns.len(), 1);
        assert_eq!(telemetry.model_breakdowns[0].model, "gemini-3.8-flash");
        assert_eq!(telemetry.sub_claude_quota_pct, 52);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    fn uuid_test_str() -> String {
        format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }
}
