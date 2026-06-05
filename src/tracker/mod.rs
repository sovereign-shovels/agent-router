use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct RequestLog {
    pub id: i64,
    pub timestamp: u64,
    pub provider: String,
    pub model: String,
    pub complexity: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_cost: f64,
    pub actual_cost: f64,
    pub latency_ms: u64,
    pub status: String,
}

pub struct CostTracker {
    conn: Connection,
}

impl CostTracker {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;
        let tracker = Self { conn };
        tracker.init()?;
        Ok(tracker)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS requests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                complexity TEXT NOT NULL,
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                estimated_cost REAL DEFAULT 0.0,
                actual_cost REAL DEFAULT 0.0,
                latency_ms INTEGER DEFAULT 0,
                status TEXT NOT NULL
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_timestamp ON requests(timestamp)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_provider ON requests(provider)",
            [],
        )?;

        Ok(())
    }

    pub fn log_request(
        &self,
        provider: &str,
        model: &str,
        complexity: &str,
        input_tokens: u64,
        output_tokens: u64,
        estimated_cost: f64,
        actual_cost: f64,
        latency_ms: u64,
        status: &str,
    ) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.conn.execute(
            "INSERT INTO requests (
                timestamp, provider, model, complexity,
                input_tokens, output_tokens, estimated_cost, actual_cost,
                latency_ms, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                timestamp,
                provider,
                model,
                complexity,
                input_tokens as i64,
                output_tokens as i64,
                estimated_cost,
                actual_cost,
                latency_ms as i64,
                status,
            ],
        )?;

        Ok(())
    }

    pub fn total_cost(&self, since_hours: Option<u64>) -> Result<f64> {
        let query = match since_hours {
            Some(hours) => {
                let cutoff = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    - hours * 3600;
                self.conn.prepare(
                    "SELECT COALESCE(SUM(actual_cost), 0.0) FROM requests WHERE timestamp > ?1",
                )?
                .query_row([cutoff as i64], |row| row.get(0))?
            }
            None => self
                .conn
                .prepare("SELECT COALESCE(SUM(actual_cost), 0.0) FROM requests")?
                .query_row([], |row| row.get(0))?,
        };
        Ok(query)
    }

    pub fn total_tokens(&self, since_hours: Option<u64>) -> Result<(u64, u64)> {
        let query = match since_hours {
            Some(hours) => {
                let cutoff = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    - hours * 3600;
                self.conn.prepare(
                    "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0) FROM requests WHERE timestamp > ?1",
                )?
                .query_row([cutoff as i64], |row| {
                    Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64))
                })?
            }
            None => self
                .conn
                .prepare("SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0) FROM requests")?
                .query_row([], |row| {
                    Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64))
                })?,
        };
        Ok(query)
    }

    pub fn provider_breakdown(&self) -> Result<Vec<(String, f64, u64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT provider, SUM(actual_cost), COUNT(*) FROM requests GROUP BY provider ORDER BY SUM(actual_cost) DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, i64>(2)? as u64,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn recent_requests(&self, limit: usize) -> Result<Vec<RequestLog>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, provider, model, complexity, input_tokens, output_tokens, estimated_cost, actual_cost, latency_ms, status
             FROM requests ORDER BY timestamp DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(RequestLog {
                id: row.get(0)?,
                timestamp: row.get::<_, i64>(1)? as u64,
                provider: row.get(2)?,
                model: row.get(3)?,
                complexity: row.get(4)?,
                input_tokens: row.get::<_, i64>(5)? as u64,
                output_tokens: row.get::<_, i64>(6)? as u64,
                estimated_cost: row.get(7)?,
                actual_cost: row.get(8)?,
                latency_ms: row.get::<_, i64>(9)? as u64,
                status: row.get(10)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_log_and_query() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("tracker.db");
        let tracker = CostTracker::open(&db_path).unwrap();

        tracker
            .log_request("ollama", "llama3.1", "simple", 10, 50, 0.0, 0.0, 120, "success")
            .unwrap();
        tracker
            .log_request("deepseek", "deepseek-chat", "medium", 100, 200, 0.001, 0.0012, 450, "success")
            .unwrap();

        let total = tracker.total_cost(None).unwrap();
        assert!((total - 0.0012).abs() < 0.0001);

        let (input, output) = tracker.total_tokens(None).unwrap();
        assert_eq!(input, 110);
        assert_eq!(output, 250);

        let breakdown = tracker.provider_breakdown().unwrap();
        assert_eq!(breakdown.len(), 2);

        let recent = tracker.recent_requests(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].provider, "deepseek"); // Most recent first
    }
}
