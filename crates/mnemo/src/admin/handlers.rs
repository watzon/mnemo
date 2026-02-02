use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::stream::StreamExt;
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;

use crate::admin::{AdminMemory, DaemonStats};
use crate::memory::types::StorageTier;
use crate::proxy::AppState;
use crate::storage::filter::MemoryFilter;

pub async fn events_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| async move {
        match result {
            Ok(event) => {
                let json = serde_json::to_string(&event).ok()?;
                Some(Ok(Event::default().data(json)))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

pub async fn stats_handler(State(state): State<Arc<AppState>>) -> Json<DaemonStats> {
    let store = state.store.lock().await;

    let hot_count = store.count_by_tier(StorageTier::Hot).await.unwrap_or(0) as u64;
    let warm_count = store.count_by_tier(StorageTier::Warm).await.unwrap_or(0) as u64;
    let cold_count = store.count_by_tier(StorageTier::Cold).await.unwrap_or(0) as u64;

    let stats = DaemonStats {
        total_memories: hot_count + warm_count + cold_count,
        hot_count,
        warm_count,
        cold_count,
        total_requests: 0,
        active_sessions: 0,
    };

    Json(stats)
}

#[derive(Debug, Deserialize, Default)]
pub struct MemoriesQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    pub tier: Option<String>,
    #[serde(rename = "type")]
    pub memory_type: Option<String>,
}

fn default_limit() -> usize {
    50
}

#[derive(serde::Serialize)]
pub struct MemoriesResponse {
    pub memories: Vec<AdminMemory>,
    pub total: u64,
    pub limit: usize,
    pub offset: usize,
}

pub async fn memories_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<MemoriesQuery>,
) -> Json<MemoriesResponse> {
    let store = state.store.lock().await;

    let mut filter = MemoryFilter::default();

    if let Some(ref tier_str) = query.tier {
        if let Some(tier) = match tier_str.to_lowercase().as_str() {
            "hot" => Some(StorageTier::Hot),
            "warm" => Some(StorageTier::Warm),
            "cold" => Some(StorageTier::Cold),
            _ => None,
        } {
            filter = filter.with_tier(tier);
        }
    }

    let total = store.count_filtered(&filter).await.unwrap_or(0) as u64;

    let memories = store
        .list_filtered(&filter, query.limit, query.offset)
        .await
        .unwrap_or_default()
        .iter()
        .map(AdminMemory::from)
        .collect();

    Json(MemoriesResponse {
        memories,
        total,
        limit: query.limit,
        offset: query.offset,
    })
}
