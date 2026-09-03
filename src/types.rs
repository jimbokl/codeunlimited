//! Unified request record extracted from local agent logs.
//! Only token counts, models, timestamps and project names - never prompts.

#[derive(Debug, Clone)]
pub struct Request {
    pub source: &'static str, // "claude" | "codex"
    pub project: String,
    pub session: String,
    pub ts: Option<i64>, // unix seconds
    pub model: String,
    pub unc_in: u64,    // uncached input tokens
    pub cached_in: u64, // tokens served from cache
    pub w5: u64,        // cache writes, 5m TTL (claude only)
    pub w1h: u64,       // cache writes, 1h TTL (claude only)
    pub out: u64,       // output tokens
}

impl Request {
    pub fn prompt_total(&self) -> u64 {
        self.unc_in + self.cached_in + self.w5 + self.w1h
    }
    pub fn total(&self) -> u64 {
        self.prompt_total() + self.out
    }
}

pub fn parse_ts(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp())
}
