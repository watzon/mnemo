//! Application state and logic

/// Main application state
pub struct App {
    /// Whether the app should exit
    pub should_quit: bool,
    /// URL of the daemon to connect to
    pub daemon_url: String,
}

impl App {
    /// Create a new App instance
    pub fn new(daemon_url: String) -> Self {
        Self {
            should_quit: false,
            daemon_url,
        }
    }

    /// Mark the app as ready to quit
    pub fn quit(&mut self) {
        self.should_quit = true;
    }
}
