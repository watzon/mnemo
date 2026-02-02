//! Event types for the TUI

use crossterm::event::KeyEvent;
use mnemo_server::admin::ProxyEvent;

/// Events that can occur in the TUI
#[derive(Debug)]
pub enum Event {
    /// A key was pressed
    Key(KeyEvent),
    /// Terminal was resized
    Resize(u16, u16),
    /// A tick occurred (for UI refresh)
    Tick,
    /// A proxy event was received from the daemon
    Proxy(ProxyEvent),
    /// Connection state changed
    ConnectionChanged(ConnectionState),
}

/// State of the connection to the daemon
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    /// Not connected
    Disconnected,
    /// Attempting to connect
    Connecting,
    /// Connected and receiving events
    Connected,
    /// Connection error occurred
    Error(String),
}
