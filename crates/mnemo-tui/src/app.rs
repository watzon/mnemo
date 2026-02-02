//! Application state and logic

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use reqwest::Client;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

use crate::event::{ConnectionState, Event};
use crate::tui::Tui;
use crate::views::{
    MemoryBrowserView, MemoryDetailView, RequestDetailView, RequestLogView, StatsView,
};
use mnemo_server::admin::{DaemonStats, ProxyEvent};

/// The currently active main view
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActiveView {
    Stats,
    Requests,
    Memories,
}

impl ActiveView {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActiveView::Stats => "Stats",
            ActiveView::Requests => "Requests",
            ActiveView::Memories => "Memories",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            ActiveView::Stats => ActiveView::Requests,
            ActiveView::Requests => ActiveView::Memories,
            ActiveView::Memories => ActiveView::Stats,
        }
    }
}

/// Type of detail view being shown
#[derive(Debug, Clone, Copy)]
pub enum DetailType {
    Request,
    Memory,
}

pub struct App {
    pub should_quit: bool,
    pub daemon_url: String,
    pub connection_state: ConnectionState,

    // View state
    pub active_view: ActiveView,
    pub show_help: bool,
    pub showing_detail: Option<DetailType>,

    // View instances
    pub stats: DaemonStats,
    pub request_log: RequestLogView,
    pub memory_browser: MemoryBrowserView,
    pub request_detail: RequestDetailView,
    pub memory_detail: MemoryDetailView,

    /// Channel to signal immediate reconnection
    reconnect_tx: watch::Sender<()>,
}

impl App {
    pub fn new(daemon_url: String) -> Self {
        let (reconnect_tx, _) = watch::channel(());
        Self {
            should_quit: false,
            daemon_url,
            connection_state: ConnectionState::Disconnected,

            // View state
            active_view: ActiveView::Stats,
            show_help: false,
            showing_detail: None,

            // View instances
            stats: DaemonStats::default(),
            request_log: RequestLogView::new(),
            memory_browser: MemoryBrowserView::new(),
            request_detail: RequestDetailView::new(),
            memory_detail: MemoryDetailView::new(),

            reconnect_tx,
        }
    }

    pub async fn run(&mut self, mut tui: Tui) -> anyhow::Result<()> {
        tui.enter()?;

        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<Event>();

        let sse_tx = event_tx.clone();
        let daemon_url = self.daemon_url.clone();
        let reconnect_rx = self.reconnect_tx.subscribe();
        tokio::spawn(async move {
            Self::sse_listener(daemon_url, sse_tx, reconnect_rx).await;
        });

        // Spawn crossterm event reader
        let input_tx = event_tx.clone();
        tokio::spawn(async move {
            Self::input_listener(input_tx).await;
        });

        // Spawn tick timer
        let tick_tx = event_tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                interval.tick().await;
                if tick_tx.send(Event::Tick).is_err() {
                    break;
                }
            }
        });

        // Main event loop
        while !self.should_quit {
            tui.draw(|f| self.render(f))?;

            if let Some(event) = event_rx.recv().await {
                self.handle_event(event);
            }
        }

        tui.exit()?;
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // Main content
                Constraint::Length(1), // Help bar
                Constraint::Length(1), // Status bar
            ])
            .split(area);

        // Render main content based on current state
        if self.show_help {
            self.render_help_overlay(frame, chunks[0]);
        } else if let Some(detail) = self.showing_detail {
            match detail {
                DetailType::Request => self.request_detail.render(frame, chunks[0]),
                DetailType::Memory => self.memory_detail.render(frame, chunks[0]),
            }
        } else {
            match self.active_view {
                ActiveView::Stats => StatsView::render(frame, chunks[0], &self.stats),
                ActiveView::Requests => self.request_log.render(frame, chunks[0]),
                ActiveView::Memories => self.memory_browser.render(frame, chunks[0]),
            }
        }

        self.render_help_bar(frame, chunks[1]);
        self.render_status_bar(frame, chunks[2]);
    }

    fn render_help_bar(&self, frame: &mut Frame, area: Rect) {
        let help_text = if self.show_help {
            "Esc: close help"
        } else if self.showing_detail.is_some() {
            "j/k: scroll │ Esc: close"
        } else {
            match self.active_view {
                ActiveView::Stats => "1/2/3: views │ Tab: next │ r: reconnect │ ?: help │ q: quit",
                ActiveView::Requests => {
                    "j/k: navigate │ Enter: detail │ 1/2/3: views │ ?: help │ q: quit"
                }
                ActiveView::Memories => {
                    "j/k: navigate │ Enter: detail │ Tab: filter │ PgUp/Dn: page │ ?: help │ q: quit"
                }
            }
        };

        let help = Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(help, area);
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        // Split into three sections: connection state, daemon URL, current view
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(20), // Connection state
                Constraint::Min(10),    // Daemon URL
                Constraint::Length(15), // Current view
            ])
            .split(area);

        // Connection state indicator
        let (status_text, status_style) = match &self.connection_state {
            ConnectionState::Connected => ("● Connected", Style::default().fg(Color::Green)),
            ConnectionState::Connecting => ("◌ Connecting...", Style::default().fg(Color::Yellow)),
            ConnectionState::Disconnected => ("○ Disconnected", Style::default().fg(Color::DarkGray)),
            ConnectionState::Error(_) => ("✗ Error", Style::default().fg(Color::Red)),
        };
        frame.render_widget(
            Paragraph::new(status_text).style(status_style),
            chunks[0],
        );

        // Daemon URL
        let url_text = format!(" {} ", self.daemon_url);
        frame.render_widget(
            Paragraph::new(url_text).style(Style::default().fg(Color::DarkGray)),
            chunks[1],
        );

        // Current view name
        let view_name = if self.show_help {
            "[Help]"
        } else if self.showing_detail.is_some() {
            "[Detail]"
        } else {
            match self.active_view {
                ActiveView::Stats => "[Stats]",
                ActiveView::Requests => "[Requests]",
                ActiveView::Memories => "[Memories]",
            }
        };
        frame.render_widget(
            Paragraph::new(view_name)
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            chunks[2],
        );
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        // Clear the area first
        frame.render_widget(Clear, area);

        let help_text = "\
Mnemo TUI - Keyboard Shortcuts

Navigation
  1           Stats view
  2           Request log view
  3           Memory browser view
  Tab         Next view
  j / Down    Navigate down
  k / Up      Navigate up
  Enter       Open detail view
  Esc         Close detail/help

Memory Browser
  Tab         Cycle tier filter (All/Hot/Warm/Cold)
  PgUp/PgDn   Previous/next page

General
  r           Reconnect to daemon
  ?           Toggle this help
  q           Quit
  Ctrl+C      Force quit";

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Help ")
            .title_style(Style::default().add_modifier(Modifier::BOLD));

        let paragraph = Paragraph::new(help_text)
            .block(block)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::White));

        frame.render_widget(paragraph, area);
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Proxy(proxy_event) => self.handle_proxy_event(proxy_event),
            Event::ConnectionChanged(state) => {
                self.connection_state = state;
            }
            Event::Tick => {}
            Event::Resize(_, _) => {
                // Resize is handled automatically by ratatui's draw()
            }
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        // Global keys that always work
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                return;
            }
            KeyCode::Esc => {
                if self.show_help {
                    self.show_help = false;
                    return;
                } else if self.showing_detail.is_some() {
                    self.showing_detail = None;
                    return;
                }
            }
            _ => {}
        }

        // If showing help, only Esc works (handled above)
        if self.show_help {
            return;
        }

        // If showing detail, handle scroll
        if let Some(detail) = self.showing_detail {
            self.handle_detail_key(key, detail);
            return;
        }

        // View switching keys
        match key.code {
            KeyCode::Char('1') => {
                self.active_view = ActiveView::Stats;
                return;
            }
            KeyCode::Char('2') => {
                self.active_view = ActiveView::Requests;
                return;
            }
            KeyCode::Char('3') => {
                self.active_view = ActiveView::Memories;
                return;
            }
            KeyCode::Tab => {
                self.active_view = self.active_view.next();
                return;
            }
            KeyCode::Char('r') => {
                let _ = self.reconnect_tx.send(());
                return;
            }
            _ => {}
        }

        // View-specific keys
        match self.active_view {
            ActiveView::Stats => {
                // Stats view has no specific keys
            }
            ActiveView::Requests => self.handle_requests_key(key),
            ActiveView::Memories => self.handle_memories_key(key),
        }
    }

    fn handle_requests_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.request_log.next(),
            KeyCode::Char('k') | KeyCode::Up => self.request_log.previous(),
            KeyCode::Enter => {
                if let Some(i) = self.request_log.state.selected() {
                    if let Some(entry) = self.request_log.entries.get(i) {
                        self.request_detail.set_request(entry.clone());
                        self.showing_detail = Some(DetailType::Request);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_memories_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.memory_browser.next(),
            KeyCode::Char('k') | KeyCode::Up => self.memory_browser.previous(),
            KeyCode::Tab => {
                // Cycle tier filter - would trigger data fetch in real implementation
                self.memory_browser.cycle_tier_filter();
            }
            KeyCode::PageDown => {
                // Next page - would trigger data fetch
                self.memory_browser.next_page();
            }
            KeyCode::PageUp => {
                // Previous page - would trigger data fetch
                self.memory_browser.prev_page();
            }
            KeyCode::Enter => {
                if let Some(memory) = self.memory_browser.selected_memory() {
                    self.memory_detail.set_memory(memory.clone());
                    self.showing_detail = Some(DetailType::Memory);
                }
            }
            _ => {}
        }
    }

    fn handle_detail_key(&mut self, key: crossterm::event::KeyEvent, detail: DetailType) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => match detail {
                DetailType::Request => self.request_detail.scroll_down(),
                DetailType::Memory => self.memory_detail.scroll_down(),
            },
            KeyCode::Char('k') | KeyCode::Up => match detail {
                DetailType::Request => self.request_detail.scroll_up(),
                DetailType::Memory => self.memory_detail.scroll_up(),
            },
            _ => {}
        }
    }

    fn handle_proxy_event(&mut self, event: ProxyEvent) {
        match event {
            ProxyEvent::RequestStarted {
                request_id,
                method,
                path,
                provider,
                timestamp,
            } => {
                use crate::views::requests::RequestEntry;
                let entry = RequestEntry::from_started(
                    request_id,
                    method,
                    path,
                    provider,
                    timestamp.format("%H:%M:%S").to_string(),
                );
                self.request_log.add_request(entry);
            }
            ProxyEvent::RequestCompleted {
                request_id,
                status,
                latency_ms,
                ..
            } => {
                self.request_log.complete_request(&request_id, status, latency_ms);
            }
            ProxyEvent::Heartbeat { stats, .. } => {
                self.stats = stats;
            }
            ProxyEvent::MemoriesInjected { .. } => {
                // Could update request entry with memory count
            }
            ProxyEvent::MemoryIngested { .. } => {
                // Could trigger memory browser refresh
            }
        }
    }

    async fn input_listener(tx: mpsc::UnboundedSender<Event>) {
        loop {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                match event::read() {
                    Ok(CrosstermEvent::Key(key)) => {
                        if tx.send(Event::Key(key)).is_err() {
                            break;
                        }
                    }
                    Ok(CrosstermEvent::Resize(w, h)) => {
                        let _ = tx.send(Event::Resize(w, h));
                    }
                    _ => {}
                }
            }
        }
    }

    async fn sse_listener(
        daemon_url: String,
        tx: mpsc::UnboundedSender<Event>,
        mut reconnect_rx: watch::Receiver<()>,
    ) {
        let client = Client::new();
        let url = format!("{daemon_url}/admin/events");
        let mut backoff_secs = 1u64;
        const MAX_BACKOFF: u64 = 30;

        loop {
            let _ = tx.send(Event::ConnectionChanged(ConnectionState::Connecting));

            match client.get(&url).send().await {
                Ok(response) if response.status().is_success() => {
                    let _ = tx.send(Event::ConnectionChanged(ConnectionState::Connected));
                    backoff_secs = 1;

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));

                                while let Some(pos) = buffer.find("\n\n") {
                                    let event_str = buffer[..pos].to_string();
                                    buffer = buffer[pos + 2..].to_string();

                                    for line in event_str.lines() {
                                        if let Some(json) = line.strip_prefix("data:") {
                                            if let Ok(proxy_event) =
                                                serde_json::from_str::<ProxyEvent>(json.trim())
                                            {
                                                let _ = tx.send(Event::Proxy(proxy_event));
                                            }
                                        }
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
                Ok(response) => {
                    let status = response.status();
                    let _ = tx.send(Event::ConnectionChanged(ConnectionState::Error(format!(
                        "HTTP {status}"
                    ))));
                }
                Err(e) => {
                    let _ = tx.send(Event::ConnectionChanged(ConnectionState::Error(
                        e.to_string(),
                    )));
                }
            }

            let _ = tx.send(Event::ConnectionChanged(ConnectionState::Disconnected));

            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {
                    backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF);
                }
                _ = reconnect_rx.changed() => {
                    backoff_secs = 1;
                }
            }
        }
    }
}
