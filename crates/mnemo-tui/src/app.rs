//! Application state and logic

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::Paragraph,
    Frame,
};
use reqwest::Client;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

use crate::event::{ConnectionState, Event};
use crate::tui::Tui;
use mnemo_server::admin::ProxyEvent;

pub struct App {
    pub should_quit: bool,
    pub daemon_url: String,
    pub connection_state: ConnectionState,
    pub last_events: Vec<ProxyEvent>,
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
            last_events: Vec::new(),
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

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        let text = "Mnemo TUI Dashboard\n\nPress 'q' to quit, 'r' to reconnect";
        frame.render_widget(Paragraph::new(text), chunks[0]);

        self.render_status_bar(frame, chunks[1]);
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let (status_text, status_style) = match &self.connection_state {
            ConnectionState::Connected => (
                "● Connected".to_string(),
                Style::default().fg(Color::Green),
            ),
            ConnectionState::Connecting => (
                "◌ Connecting...".to_string(),
                Style::default().fg(Color::Yellow),
            ),
            ConnectionState::Disconnected => (
                "○ Disconnected (press 'r' to reconnect)".to_string(),
                Style::default().fg(Color::DarkGray),
            ),
            ConnectionState::Error(e) => (
                format!("✗ Error: {e}"),
                Style::default().fg(Color::Red),
            ),
        };

        let status = Paragraph::new(status_text).style(status_style);
        frame.render_widget(status, area);
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Proxy(proxy_event) => {
                self.last_events.push(proxy_event);
                if self.last_events.len() > 100 {
                    self.last_events.remove(0);
                }
            }
            Event::ConnectionChanged(state) => {
                self.connection_state = state;
            }
            Event::Tick => {}
            Event::Resize(_, _) => {}
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('r') => {
                let _ = self.reconnect_tx.send(());
            }
            _ => {}
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
        let url = format!("{}/admin/events", daemon_url);
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
