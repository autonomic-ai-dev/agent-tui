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

pub fn probe_organ_mesh(organs: &[crate::OrganMeta]) -> Vec<LiveOrganStatus> {
    organs
        .iter()
        .map(|meta| {
            let status = if meta.port == "LOCAL" {
                "OPERATIONAL"
            } else if let Ok(port) = meta.port.parse::<u16>() {
                let addr = SocketAddr::from(([127, 0, 0, 1], port));
                if TcpStream::connect_timeout(&addr, Duration::from_millis(25)).is_ok() {
                    "OPERATIONAL"
                } else {
                    "OFFLINE"
                }
            } else {
                "STANDBY"
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

    // Today's total spend and calls
    let start_of_today_ms = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0);

    let mut today_spend = 0.0;
    let mut today_calls = 0u64;

    if let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(SUM(estimated_cost_usd), 0.0), COUNT(*)
         FROM model_usage_events
         WHERE timestamp >= ?",
    ) && let Ok((spend, calls)) = stmt.query_row([start_of_today_ms], |row| {
        Ok((row.get::<_, f64>(0)?, row.get::<_, u64>(1)?))
    }) {
        today_spend = spend;
        today_calls = calls;
    }

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
         ORDER BY SUM(estimated_cost_usd) DESC",
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

    Some(UsageViewData {
        today_spend_usd: today_spend,
        today_calls,
        monthly_ceiling_usd: monthly_ceiling,
        burn_rate_usd: today_spend, // daily burn rate
        sub_claude_quota_pct: 0,
        sub_claude_reset_str: "Standby".to_string(),
        sub_openai_tpm_pct: 0,
        model_breakdowns: breakdowns,
        cache_savings_pct: 0.0,
        cache_dollars_saved: 0.0,
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
