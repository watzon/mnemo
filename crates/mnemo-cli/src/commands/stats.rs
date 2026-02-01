use clap::Parser;
use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};
use mnemo_server::{memory::types::StorageTier, storage::LanceStore};

use crate::error::CliResult;
use crate::output::OutputFormat;

#[derive(Parser)]
pub struct StatsCommand {
    #[clap(long, help = "Filter statistics to a specific session/project")]
    pub session: Option<String>,

    #[clap(
        long,
        help = "Show only global memory statistics",
        conflicts_with = "session"
    )]
    pub global: bool,
}

impl StatsCommand {
    pub async fn execute(&self, store: &LanceStore, format: OutputFormat) -> CliResult<()> {
        let (hot_count, warm_count, cold_count, total_count) =
            if self.session.is_some() || self.global {
                let mut hot = Vec::new();
                let mut warm = Vec::new();
                let mut cold = Vec::new();

                for tier in [StorageTier::Hot, StorageTier::Warm, StorageTier::Cold] {
                    let memories = store.list_by_tier(tier).await?;
                    let filtered: Vec<_> = memories
                        .into_iter()
                        .filter(|m| {
                            if let Some(ref session_id) = self.session {
                                m.conversation_id.as_deref() == Some(session_id.as_str())
                            } else if self.global {
                                m.conversation_id.is_none()
                            } else {
                                true
                            }
                        })
                        .collect();

                    match tier {
                        StorageTier::Hot => hot = filtered,
                        StorageTier::Warm => warm = filtered,
                        StorageTier::Cold => cold = filtered,
                    }
                }

                (hot.len(), warm.len(), cold.len(), hot.len() + warm.len() + cold.len())
            } else {
                let hot_count = store.count_by_tier(StorageTier::Hot).await?;
                let warm_count = store.count_by_tier(StorageTier::Warm).await?;
                let cold_count = store.count_by_tier(StorageTier::Cold).await?;
                let total_count = store.total_count().await?;
                (hot_count, warm_count, cold_count, total_count)
            };

        let estimate_hot_size = estimate_memory_size(hot_count);
        let estimate_warm_size = estimate_memory_size(warm_count);
        let estimate_cold_size = estimate_memory_size(cold_count);
        let estimate_total_size = estimate_memory_size(total_count);

        match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "total_memories": total_count,
                    "by_tier": {
                        "hot": {
                            "count": hot_count,
                            "estimated_size_bytes": estimate_hot_size,
                        },
                        "warm": {
                            "count": warm_count,
                            "estimated_size_bytes": estimate_warm_size,
                        },
                        "cold": {
                            "count": cold_count,
                            "estimated_size_bytes": estimate_cold_size,
                        }
                    },
                    "total_estimated_size_bytes": estimate_total_size,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                println!("Mnemo Statistics");
                println!("======================\n");

                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL_CONDENSED)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header(["Tier", "Count", "Estimated Size"]);

                table.add_row([
                    "Hot",
                    &hot_count.to_string(),
                    &format_size(estimate_hot_size),
                ]);
                table.add_row([
                    "Warm",
                    &warm_count.to_string(),
                    &format_size(estimate_warm_size),
                ]);
                table.add_row([
                    "Cold",
                    &cold_count.to_string(),
                    &format_size(estimate_cold_size),
                ]);

                println!("{table}\n");

                println!(
                    "Total: {} memories ({} estimated)",
                    total_count,
                    format_size(estimate_total_size)
                );
            }
        }

        Ok(())
    }
}

fn estimate_memory_size(count: usize) -> u64 {
    const EMBEDDING_SIZE: u64 = 384 * 4;
    const AVG_CONTENT_SIZE: u64 = 500;
    const METADATA_OVERHEAD: u64 = 200;
    const PER_MEMORY_ESTIMATE: u64 = EMBEDDING_SIZE + AVG_CONTENT_SIZE + METADATA_OVERHEAD;

    count as u64 * PER_MEMORY_ESTIMATE
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
