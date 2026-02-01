use clap::Parser;
use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};
use mnemo_server::{
    memory::types::StorageTier,
    storage::{Compactor, LanceStore},
};

use crate::error::CliResult;
use crate::output::OutputFormat;

#[derive(Parser)]
pub struct CompactCommand {
    #[clap(
        long,
        short,
        help = "Storage tier to compact (hot, warm, cold). Defaults to all tiers."
    )]
    pub tier: Option<String>,
}

impl CompactCommand {
    pub async fn execute(&self, store: &LanceStore, format: OutputFormat) -> CliResult<()> {
        let tiers: Vec<StorageTier> = match self.tier.as_deref() {
            Some("hot") => vec![StorageTier::Hot],
            Some("warm") => vec![StorageTier::Warm],
            Some("cold") => vec![StorageTier::Cold],
            Some(t) => return Err(format!("Unknown tier: {t}. Use hot, warm, or cold.").into()),
            None => vec![StorageTier::Hot, StorageTier::Warm, StorageTier::Cold],
        };

        let compactor = Compactor::new(store);
        let mut total_compacted = 0u32;
        let mut total_skipped_high_weight = 0u32;
        let mut total_already_compressed = 0u32;
        let mut tier_results = Vec::new();

        for tier in &tiers {
            let result = compactor.compact(*tier).await?;
            total_compacted += result.compacted_count;
            total_skipped_high_weight += result.skipped_high_weight;
            total_already_compressed += result.already_compressed;

            tier_results.push((
                format!("{tier:?}"),
                result.compacted_count,
                result.skipped_high_weight,
                result.already_compressed,
            ));
        }

        match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "tiers": tier_results.iter().map(|(tier, compacted, skipped, already)| {
                        serde_json::json!({
                            "tier": tier,
                            "compacted": compacted,
                            "skipped_high_weight": skipped,
                            "already_compressed": already,
                        })
                    }).collect::<Vec<_>>(),
                    "totals": {
                        "compacted": total_compacted,
                        "skipped_high_weight": total_skipped_high_weight,
                        "already_compressed": total_already_compressed,
                    }
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                println!("Compaction Results");
                println!("==================\n");

                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL_CONDENSED)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header([
                        "Tier",
                        "Compacted",
                        "Skipped (High Weight)",
                        "Already Compressed",
                    ]);

                for (tier, compacted, skipped, already) in &tier_results {
                    table.add_row([
                        tier,
                        &compacted.to_string(),
                        &skipped.to_string(),
                        &already.to_string(),
                    ]);
                }

                println!("{table}\n");

                println!(
                    "Total: {total_compacted} compacted, {total_skipped_high_weight} skipped, {total_already_compressed} already compressed"
                );
            }
        }

        Ok(())
    }
}
