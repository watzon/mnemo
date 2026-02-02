//! TUI view components

pub mod detail;
pub mod memories;
pub mod requests;
pub mod stats;

pub use detail::{MemoryDetailView, RequestDetailView};
pub use memories::{MemoryBrowserView, TierFilter};
pub use requests::RequestLogView;
pub use stats::StatsView;
