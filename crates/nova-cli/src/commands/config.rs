use std::path::Path;

use clap::Parser;
use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};
use nova_memory::config::Config;

use crate::error::CliResult;
use crate::output::OutputFormat;

#[derive(Parser)]
pub struct ConfigCommand {
    #[clap(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Parser)]
pub enum ConfigSubcommand {
    #[clap(about = "Show current configuration")]
    Show,
}

impl ConfigCommand {
    pub async fn execute(&self, config_path: Option<&Path>, format: OutputFormat) -> CliResult<()> {
        match &self.command {
            ConfigSubcommand::Show => Self::show(config_path, format).await,
        }
    }

    async fn show(config_path: Option<&Path>, format: OutputFormat) -> CliResult<()> {
        let config = if let Some(path) = config_path {
            let content = std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read config file: {e}"))?;
            toml::from_str::<Config>(&content)
                .map_err(|e| format!("Failed to parse config file: {e}"))?
        } else {
            Config::default()
        };

        match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "storage": {
                        "hot_cache_gb": config.storage.hot_cache_gb,
                        "warm_storage_gb": config.storage.warm_storage_gb,
                        "cold_enabled": config.storage.cold_enabled,
                        "data_dir": config.storage.data_dir.display().to_string(),
                    },
                    "proxy": {
                        "listen_addr": config.proxy.listen_addr,
                        "upstream_url": config.proxy.upstream_url,
                        "timeout_secs": config.proxy.timeout_secs,
                        "max_injection_tokens": config.proxy.max_injection_tokens,
                    },
                    "router": {
                        "strategy": config.router.strategy,
                        "max_memories": config.router.max_memories,
                        "relevance_threshold": config.router.relevance_threshold,
                    },
                    "embedding": {
                        "provider": config.embedding.provider,
                        "model": config.embedding.model,
                        "dimension": config.embedding.dimension,
                        "batch_size": config.embedding.batch_size,
                    }
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                if config_path.is_some() {
                    println!("Configuration from: {}", config_path.unwrap().display());
                } else {
                    println!("Configuration: (using defaults)");
                }
                println!("==============================\n");

                println!("[Storage]");
                let mut storage_table = Table::new();
                storage_table
                    .load_preset(UTF8_FULL_CONDENSED)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header(["Setting", "Value"]);

                storage_table.add_row(["hot_cache_gb", &config.storage.hot_cache_gb.to_string()]);
                storage_table.add_row([
                    "warm_storage_gb",
                    &config.storage.warm_storage_gb.to_string(),
                ]);
                storage_table.add_row(["cold_enabled", &config.storage.cold_enabled.to_string()]);
                storage_table.add_row(["data_dir", &config.storage.data_dir.display().to_string()]);

                println!("{storage_table}\n");

                println!("[Proxy]");
                let mut proxy_table = Table::new();
                proxy_table
                    .load_preset(UTF8_FULL_CONDENSED)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header(["Setting", "Value"]);

                proxy_table.add_row(["listen_addr", &config.proxy.listen_addr]);
                proxy_table.add_row([
                    "upstream_url",
                    if config.proxy.upstream_url.is_empty() {
                        "(not set)"
                    } else {
                        &config.proxy.upstream_url
                    },
                ]);
                proxy_table.add_row(["timeout_secs", &config.proxy.timeout_secs.to_string()]);
                proxy_table.add_row([
                    "max_injection_tokens",
                    &config.proxy.max_injection_tokens.to_string(),
                ]);

                println!("{proxy_table}\n");

                println!("[Router]");
                let mut router_table = Table::new();
                router_table
                    .load_preset(UTF8_FULL_CONDENSED)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header(["Setting", "Value"]);

                router_table.add_row([
                    "strategy",
                    if config.router.strategy.is_empty() {
                        "(not set)"
                    } else {
                        &config.router.strategy
                    },
                ]);
                router_table.add_row(["max_memories", &config.router.max_memories.to_string()]);
                router_table.add_row([
                    "relevance_threshold",
                    &config.router.relevance_threshold.to_string(),
                ]);

                println!("{router_table}\n");

                println!("[Embedding]");
                let mut embedding_table = Table::new();
                embedding_table
                    .load_preset(UTF8_FULL_CONDENSED)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header(["Setting", "Value"]);

                embedding_table.add_row([
                    "provider",
                    if config.embedding.provider.is_empty() {
                        "(not set)"
                    } else {
                        &config.embedding.provider
                    },
                ]);
                embedding_table.add_row([
                    "model",
                    if config.embedding.model.is_empty() {
                        "(not set)"
                    } else {
                        &config.embedding.model
                    },
                ]);
                embedding_table.add_row(["dimension", &config.embedding.dimension.to_string()]);
                embedding_table.add_row(["batch_size", &config.embedding.batch_size.to_string()]);

                println!("{embedding_table}");
            }
        }

        Ok(())
    }
}
