use rusqlite::{params, Connection, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct RequestRecord {
    pub id: i64,
    pub session_id: Option<String>,
    pub timestamp: String,
    pub tier: String,
    pub model: String,
    pub provider: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost: f64,
    pub success: bool,
    pub error_message: Option<String>,
}

#[derive(Clone)]
pub struct CostTracker {
    conn: Arc<Mutex<Connection>>,
}

impl CostTracker {
    pub fn new(db_path: Option<PathBuf>) -> Result<Self> {
        let path = db_path.unwrap_or_else(|| {
            let base = dirs::data_dir()
                .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("."));
            base.join("agent-router").join("tracker.db")
        });
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(&path)?;
        let tracker = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        tracker.init_tables()?;
        Ok(tracker)
    }

    pub fn init_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS requests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                tier TEXT NOT NULL,
                model TEXT NOT NULL,
                provider TEXT NOT NULL,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cost REAL NOT NULL DEFAULT 0.0,
                success INTEGER NOT NULL DEFAULT 1,
                error_message TEXT
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session ON requests(session_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_timestamp ON requests(timestamp)",
            [],
        )?;
        Ok(())
    }

    pub fn record_request(
        &self,
        session_id: Option<&str>,
        tier: &str,
        model: &str,
        provider: &str,
        input_tokens: u32,
        output_tokens: u32,
        cost: f64,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO requests (session_id, tier, model, provider, input_tokens, output_tokens, cost, success, error_message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session_id,
                tier,
                model,
                provider,
                input_tokens,
                output_tokens,
                cost,
                if success { 1 } else { 0 },
                error_message,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn calculate_cost(input_tokens: u32, output_tokens: u32, cost_input_per_1m: f64, cost_output_per_1m: f64) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * cost_input_per_1m;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * cost_output_per_1m;
        input_cost + output_cost
    }

    pub fn session_total_cost(&self, session_id: &str) -> Result<f64> {
        let conn = self.conn.lock().unwrap();
        let cost: f64 = conn.query_row(
            "SELECT COALESCE(SUM(cost), 0.0) FROM requests WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(cost)
    }

    pub fn total_requests(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM requests",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn total_cost(&self) -> Result<f64> {
        let conn = self.conn.lock().unwrap();
        let cost: f64 = conn.query_row(
            "SELECT COALESCE(SUM(cost), 0.0) FROM requests",
            [],
            |row| row.get(0),
        )?;
        Ok(cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_tracker() -> CostTracker {
        let conn = Connection::open_in_memory().unwrap();
        let tracker = CostTracker {
            conn: Arc::new(Mutex::new(conn)),
        };
        tracker.init_tables().unwrap();
        tracker
    }

    #[test]
    fn test_record_and_retrieve() {
        let t = in_memory_tracker();
        let id = t
            .record_request(Some("sess1"), "local", "llama3.2", "ollama", 10, 20, 0.0, true, None)
            .unwrap();
        assert!(id > 0);
        assert_eq!(t.total_requests().unwrap(), 1);
    }

    #[test]
    fn test_calculate_cost() {
        let cost = CostTracker::calculate_cost(1_000_000, 500_000, 0.14, 0.28);
        assert!((cost - 0.28).abs() < 0.001, "expected ~0.28, got {}", cost);
    }

    #[test]
    fn test_session_total_cost() {
        let t = in_memory_tracker();
        t.record_request(Some("sess1"), "cheap", "ds", "deepseek", 1000, 500, 0.00014 + 0.00014, true, None)
            .unwrap();
        t.record_request(Some("sess1"), "cheap", "ds", "deepseek", 2000, 1000, 0.00028 + 0.00028, true, None)
            .unwrap();
        let total = t.session_total_cost("sess1").unwrap();
        assert!(total > 0.0);
    }

    #[test]
    fn test_total_cost_across_sessions() {
        let t = in_memory_tracker();
        t.record_request(None, "local", "llama", "ollama", 10, 10, 0.0, true, None)
            .unwrap();
        t.record_request(None, "cheap", "ds", "deepseek", 1_000_000, 0, 0.14, true, None)
            .unwrap();
        let total = t.total_cost().unwrap();
        assert!((total - 0.14).abs() < 0.0001);
    }

    #[test]
    fn test_error_tracking() {
        let t = in_memory_tracker();
        t.record_request(
            None,
            "local",
            "llama",
            "ollama",
            0,
            0,
            0.0,
            false,
            Some("connection refused"),
        )
        .unwrap();
        assert_eq!(t.total_requests().unwrap(), 1);
    }
}
