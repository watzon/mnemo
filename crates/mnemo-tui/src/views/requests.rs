//! Request log view

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame,
};
use std::collections::VecDeque;

/// A single request entry for display
#[derive(Debug, Clone)]
pub struct RequestEntry {
    pub request_id: String,
    pub time: String,
    pub method: String,
    pub path: String,
    pub provider: String,
    pub status: Option<u16>,
    pub latency_ms: Option<u64>,
}

impl RequestEntry {
    pub fn from_started(
        request_id: String,
        method: String,
        path: String,
        provider: String,
        time: String,
    ) -> Self {
        Self {
            request_id,
            time,
            method,
            path,
            provider,
            status: None,
            latency_ms: None,
        }
    }

    pub fn complete(&mut self, status: u16, latency_ms: u64) {
        self.status = Some(status);
        self.latency_ms = Some(latency_ms);
    }
}

pub struct RequestLogView {
    pub entries: VecDeque<RequestEntry>,
    pub state: TableState,
    max_entries: usize,
}

impl RequestLogView {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            state: TableState::default(),
            max_entries: 1000,
        }
    }

    pub fn add_request(&mut self, entry: RequestEntry) {
        self.entries.push_front(entry);
        if self.entries.len() > self.max_entries {
            self.entries.pop_back();
        }
    }

    pub fn complete_request(&mut self, request_id: &str, status: u16, latency_ms: u64) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.request_id == request_id) {
            entry.complete(status, latency_ms);
        }
    }

    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.entries.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.entries.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let header = Row::new(vec![
            Cell::from("Time").style(Style::default().bold()),
            Cell::from("Method").style(Style::default().bold()),
            Cell::from("Path").style(Style::default().bold()),
            Cell::from("Provider").style(Style::default().bold()),
            Cell::from("Status").style(Style::default().bold()),
            Cell::from("Latency").style(Style::default().bold()),
        ])
        .height(1);

        let rows: Vec<Row> = self
            .entries
            .iter()
            .map(|entry| {
                let status_str = entry
                    .status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "...".to_string());
                let latency_str = entry
                    .latency_ms
                    .map(|l| format!("{}ms", l))
                    .unwrap_or_else(|| "...".to_string());

                let status_style = match entry.status {
                    Some(s) if s >= 200 && s < 300 => Style::default().fg(Color::Green),
                    Some(s) if s >= 400 => Style::default().fg(Color::Red),
                    _ => Style::default(),
                };

                Row::new(vec![
                    Cell::from(entry.time.clone()),
                    Cell::from(entry.method.clone()),
                    Cell::from(truncate_path(&entry.path, 40)),
                    Cell::from(entry.provider.clone()),
                    Cell::from(status_str).style(status_style),
                    Cell::from(latency_str),
                ])
            })
            .collect();

        let widths = [
            ratatui::layout::Constraint::Length(8),  // Time
            ratatui::layout::Constraint::Length(7),  // Method
            ratatui::layout::Constraint::Min(20),    // Path
            ratatui::layout::Constraint::Length(10), // Provider
            ratatui::layout::Constraint::Length(6),  // Status
            ratatui::layout::Constraint::Length(10), // Latency
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("📋 Request Log"),
            )
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        frame.render_stateful_widget(table, area, &mut self.state);
    }
}

fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len() - (max_len - 3)..])
    }
}

impl Default for RequestLogView {
    fn default() -> Self {
        Self::new()
    }
}
