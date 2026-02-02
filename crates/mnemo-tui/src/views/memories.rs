//! Memory browser view

use mnemo_server::admin::AdminMemory;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TierFilter {
    All,
    Hot,
    Warm,
    Cold,
}

impl TierFilter {
    pub fn next(&self) -> Self {
        match self {
            TierFilter::All => TierFilter::Hot,
            TierFilter::Hot => TierFilter::Warm,
            TierFilter::Warm => TierFilter::Cold,
            TierFilter::Cold => TierFilter::All,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TierFilter::All => "All",
            TierFilter::Hot => "Hot",
            TierFilter::Warm => "Warm",
            TierFilter::Cold => "Cold",
        }
    }

    pub fn to_query_param(&self) -> Option<&'static str> {
        match self {
            TierFilter::All => None,
            TierFilter::Hot => Some("hot"),
            TierFilter::Warm => Some("warm"),
            TierFilter::Cold => Some("cold"),
        }
    }
}

pub struct MemoryBrowserView {
    pub memories: Vec<AdminMemory>,
    pub state: TableState,
    pub tier_filter: TierFilter,
    pub page: usize,
    pub page_size: usize,
    pub total: u64,
    pub loading: bool,
}

impl MemoryBrowserView {
    pub fn new() -> Self {
        Self {
            memories: Vec::new(),
            state: TableState::default(),
            tier_filter: TierFilter::All,
            page: 0,
            page_size: 50,
            total: 0,
            loading: false,
        }
    }

    pub fn set_memories(&mut self, memories: Vec<AdminMemory>, total: u64) {
        self.memories = memories;
        self.total = total;
        self.loading = false;
        if !self.memories.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    pub fn next_page(&mut self) -> bool {
        let max_page = if self.total == 0 {
            0
        } else {
            (self.total as usize - 1) / self.page_size
        };
        if self.page < max_page {
            self.page += 1;
            self.state.select(Some(0));
            self.loading = true;
            true // Needs data fetch
        } else {
            false
        }
    }

    pub fn prev_page(&mut self) -> bool {
        if self.page > 0 {
            self.page -= 1;
            self.state.select(Some(0));
            self.loading = true;
            true // Needs data fetch
        } else {
            false
        }
    }

    pub fn cycle_tier_filter(&mut self) -> bool {
        self.tier_filter = self.tier_filter.next();
        self.page = 0;
        self.state.select(None);
        self.loading = true;
        true // Needs data fetch
    }

    pub fn next(&mut self) {
        if self.memories.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.memories.len().saturating_sub(1) {
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
        if self.memories.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.memories.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn selected_memory(&self) -> Option<&AdminMemory> {
        self.state.selected().and_then(|i| self.memories.get(i))
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // Table
                Constraint::Length(1), // Footer
            ])
            .split(area);

        self.render_table(frame, chunks[0]);
        self.render_footer(frame, chunks[1]);
    }

    fn render_table(&mut self, frame: &mut Frame, area: Rect) {
        let header = Row::new(vec![
            Cell::from("ID").style(Style::default().bold()),
            Cell::from("Type").style(Style::default().bold()),
            Cell::from("Entities").style(Style::default().bold()),
            Cell::from("Weight").style(Style::default().bold()),
            Cell::from("Tier").style(Style::default().bold()),
            Cell::from("Created").style(Style::default().bold()),
        ])
        .height(1);

        let rows: Vec<Row> = self
            .memories
            .iter()
            .map(|memory| {
                let tier_style = match memory.tier.as_str() {
                    "Hot" => Style::default().fg(Color::Red),
                    "Warm" => Style::default().fg(Color::Yellow),
                    "Cold" => Style::default().fg(Color::Blue),
                    _ => Style::default(),
                };

                let entities = if memory.entities.is_empty() {
                    "-".to_string()
                } else {
                    memory
                        .entities
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                };

                Row::new(vec![
                    Cell::from(truncate_id(&memory.id, 8)),
                    Cell::from(memory.memory_type.clone()),
                    Cell::from(entities),
                    Cell::from(format!("{:.2}", memory.weight)),
                    Cell::from(memory.tier.clone()).style(tier_style),
                    Cell::from(memory.created_at.format("%m/%d %H:%M").to_string()),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(10), // ID
            Constraint::Length(10), // Type
            Constraint::Min(15),    // Entities
            Constraint::Length(6),  // Weight
            Constraint::Length(6),  // Tier
            Constraint::Length(12), // Created
        ];

        let title = format!("🧠 Memories [Filter: {}]", self.tier_filter.as_str());

        let table = Table::new(rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(title))
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        frame.render_stateful_widget(table, area, &mut self.state);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let max_page = if self.total == 0 {
            0
        } else {
            (self.total as usize - 1) / self.page_size
        };

        let footer_text = format!(
            " Page {}/{} | Total: {} | Tab: filter | PgUp/PgDn: navigate ",
            self.page + 1,
            max_page + 1,
            self.total
        );

        let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray));

        frame.render_widget(footer, area);
    }
}

fn truncate_id(id: &str, len: usize) -> String {
    if id.len() <= len {
        id.to_string()
    } else {
        format!("{}...", &id[..len])
    }
}

impl Default for MemoryBrowserView {
    fn default() -> Self {
        Self::new()
    }
}
