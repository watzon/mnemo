use chrono::{DateTime, Utc};

#[derive(Clone, Copy, Debug, Default)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
}

pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

pub fn format_timestamp(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}
