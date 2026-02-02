//! Application state and logic

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyModifiers};
use futures::StreamExt;
use ratatui::widgets::Paragraph;
use reqwest::Client;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::event::{ConnectionState, Event};
use crate::tui::Tui;
use mnemo_server::admin::ProxyEvent;

pub struct App {
    pub should_quit: bool,
    pub daemon_url: String,
    pub connection_state: ConnectionState,
    pub last_events: Vec<ProxyEvent>,
}

impl App {
    pub fn new(daemon_url: String) -> Self {
        Self {
            should_quit: false,
            daemon_url,
            connection_state: ConnectionState::Disconnected,
            last_events: Vec::new(),
        }
    }

    pub async fn run(&mut self, mut tui: Tui) -> anyhow::Result<()> {
        tui.enter()?;

        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<Event>();

        // Spawn SSE listener
        let sse_tx = event_tx.clone();
        let daemon_url = self.daemon_url.clone();
        tokio::spawn(async move {
            Self::sse_listener(daemon_url, sse_tx).await;
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

    fn render(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let text = format!(
            "Mnemo TUI Dashboard\n\nConnection: {:?}\nPress 'q' to quit",
            self.connection_state
        );
        frame.render_widget(Paragraph::new(text), area);
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

    async fn sse_listener(daemon_url: String, tx: mpsc::UnboundedSender<Event>) {
        let _ = tx.send(Event::ConnectionChanged(ConnectionState::Connecting));

        let client = Client::new();
        let url = format!("{}/admin/events", daemon_url);

        match client.get(&url).send().await {
            Ok(response) => {
                let _ = tx.send(Event::ConnectionChanged(ConnectionState::Connected));

                let mut stream = response.bytes_stream();
                let mut buffer = String::new();

                while let Some(chunk) = stream.next().await {
                    if let Ok(bytes) = chunk {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));

                        // Parse SSE events from buffer (delimited by double newline)
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
                }
            }
            Err(e) => {
                let _ = tx.send(Event::ConnectionChanged(ConnectionState::Error(
                    e.to_string(),
                )));
            }
        }

        let _ = tx.send(Event::ConnectionChanged(ConnectionState::Disconnected));
    }
}
