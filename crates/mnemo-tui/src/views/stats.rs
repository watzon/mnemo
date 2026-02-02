//! Stats dashboard view

use mnemo_server::admin::DaemonStats;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};

pub struct StatsView;

impl StatsView {
    pub fn render(frame: &mut Frame, area: Rect, stats: &DaemonStats) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Length(3), // Total memories gauge
                Constraint::Min(6),    // Tier breakdown
                Constraint::Length(3), // Requests counter
            ])
            .split(area);

        // Title
        let title = Paragraph::new("📊 Stats Dashboard")
            .style(Style::default().bold())
            .block(Block::default());
        frame.render_widget(title, chunks[0]);

        // Total memories gauge
        let total = stats.total_memories;
        let max_memories: u64 = 10000; // Configurable max for gauge
        let ratio = if max_memories > 0 {
            (total as f64 / max_memories as f64).min(1.0)
        } else {
            0.0
        };

        let total_gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Total Memories"),
            )
            .gauge_style(Style::default().fg(Color::Cyan))
            .ratio(ratio)
            .label(format!("{total}"));
        frame.render_widget(total_gauge, chunks[1]);

        // Tier breakdown - three horizontal gauges
        let tier_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(34),
                Constraint::Percentage(33),
            ])
            .split(chunks[2]);

        let hot_gauge = Self::tier_gauge("🔥 Hot", stats.hot_count, total, Color::Red);
        let warm_gauge = Self::tier_gauge("🌡 Warm", stats.warm_count, total, Color::Yellow);
        let cold_gauge = Self::tier_gauge("❄ Cold", stats.cold_count, total, Color::Blue);

        frame.render_widget(hot_gauge, tier_chunks[0]);
        frame.render_widget(warm_gauge, tier_chunks[1]);
        frame.render_widget(cold_gauge, tier_chunks[2]);

        // Requests counter
        let requests = Paragraph::new(format!("Total Requests: {}", stats.total_requests))
            .block(Block::default().borders(Borders::ALL).title("Activity"));
        frame.render_widget(requests, chunks[3]);
    }

    fn tier_gauge(title: &str, count: u64, total: u64, color: Color) -> Gauge<'static> {
        let ratio = if total > 0 {
            count as f64 / total as f64
        } else {
            0.0
        };

        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title.to_string()),
            )
            .gauge_style(Style::default().fg(color))
            .ratio(ratio)
            .label(format!("{count}"))
    }
}
