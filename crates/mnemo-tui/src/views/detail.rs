//! Detail views for requests and memories

use mnemo_server::admin::AdminMemory;
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::requests::RequestEntry;

/// Detail view for a single request
pub struct RequestDetailView {
    pub request: Option<RequestEntry>,
    pub scroll: u16,
}

impl RequestDetailView {
    pub fn new() -> Self {
        Self {
            request: None,
            scroll: 0,
        }
    }

    pub fn set_request(&mut self, request: RequestEntry) {
        self.request = Some(request);
        self.scroll = 0;
    }

    pub fn clear(&mut self) {
        self.request = None;
        self.scroll = 0;
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("📋 Request Details (Esc to close, j/k to scroll)");

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if let Some(ref req) = self.request {
            let content = format!(
                "Request ID: {}\n\n\
                 Time: {}\n\
                 Method: {}\n\
                 Path: {}\n\
                 Provider: {}\n\n\
                 Status: {}\n\
                 Latency: {}",
                req.request_id,
                req.time,
                req.method,
                req.path,
                req.provider,
                req.status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "Pending...".to_string()),
                req.latency_ms
                    .map(|l| format!("{}ms", l))
                    .unwrap_or_else(|| "...".to_string()),
            );

            let paragraph = Paragraph::new(content)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0));

            frame.render_widget(paragraph, inner);
        } else {
            let paragraph = Paragraph::new("No request selected");
            frame.render_widget(paragraph, inner);
        }
    }
}

impl Default for RequestDetailView {
    fn default() -> Self {
        Self::new()
    }
}

/// Detail view for a single memory
pub struct MemoryDetailView {
    pub memory: Option<AdminMemory>,
    pub scroll: u16,
}

impl MemoryDetailView {
    pub fn new() -> Self {
        Self {
            memory: None,
            scroll: 0,
        }
    }

    pub fn set_memory(&mut self, memory: AdminMemory) {
        self.memory = Some(memory);
        self.scroll = 0;
    }

    pub fn clear(&mut self) {
        self.memory = None;
        self.scroll = 0;
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("🧠 Memory Details (Esc to close, j/k to scroll)");

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if let Some(ref mem) = self.memory {
            let entities = if mem.entities.is_empty() {
                "None".to_string()
            } else {
                mem.entities.join(", ")
            };

            let content = format!(
                "ID: {}\n\n\
                 Type: {}\n\
                 Tier: {}\n\
                 Weight: {:.2}\n\
                 Access Count: {}\n\n\
                 Created: {}\n\
                 Last Accessed: {}\n\n\
                 Entities: {}\n\n\
                 Conversation ID: {}\n\n\
                 ─── Content ───\n\n\
                 {}",
                mem.id,
                mem.memory_type,
                mem.tier,
                mem.weight,
                mem.access_count,
                mem.created_at.format("%Y-%m-%d %H:%M:%S"),
                mem.last_accessed.format("%Y-%m-%d %H:%M:%S"),
                entities,
                mem.conversation_id.as_deref().unwrap_or("None"),
                mem.content,
            );

            let paragraph = Paragraph::new(content)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0));

            frame.render_widget(paragraph, inner);
        } else {
            let paragraph = Paragraph::new("No memory selected");
            frame.render_widget(paragraph, inner);
        }
    }
}

impl Default for MemoryDetailView {
    fn default() -> Self {
        Self::new()
    }
}
