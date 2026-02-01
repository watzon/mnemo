use clap::{Parser, Subcommand};
use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};
use mnemo_server::{
    memory::types::{Memory, MemorySource, MemoryType, StorageTier},
    storage::LanceStore,
};
use uuid::Uuid;

use crate::error::CliResult;
use crate::output::{OutputFormat, format_timestamp, truncate_string};

#[derive(Parser)]
pub struct MemoryCommand {
    #[clap(subcommand)]
    pub command: MemorySubcommand,
}

#[derive(Subcommand)]
pub enum MemorySubcommand {
    #[clap(about = "List memories")]
    List(ListArgs),

    #[clap(about = "Show memory details")]
    Show(ShowArgs),

    #[clap(about = "Delete a memory")]
    Delete(DeleteArgs),

    #[clap(about = "Convert a session memory to global (removes session association)")]
    Globalize(GlobalizeArgs),

    #[clap(about = "Manually add a memory")]
    Add(AddArgs),
}

#[derive(Parser)]
pub struct ListArgs {
    #[clap(
        long,
        short,
        default_value = "20",
        help = "Maximum number of memories to display"
    )]
    pub limit: usize,

    #[clap(
        long,
        short,
        help = "Filter by memory type (episodic, semantic, procedural)"
    )]
    pub r#type: Option<String>,

    #[clap(long, help = "Filter to memories in this session/project")]
    pub session: Option<String>,

    #[clap(
        long,
        help = "Show only global memories (no session)",
        conflicts_with = "session"
    )]
    pub global: bool,
}

#[derive(Parser)]
pub struct ShowArgs {
    #[clap(help = "Memory ID (UUID format)")]
    pub id: String,
}

#[derive(Parser)]
pub struct DeleteArgs {
    #[clap(help = "Memory ID to delete (UUID format)")]
    pub id: String,
}

#[derive(Parser)]
pub struct GlobalizeArgs {
    #[clap(help = "Memory ID to globalize (UUID format)")]
    pub id: String,
}

#[derive(Parser)]
pub struct AddArgs {
    #[clap(help = "Memory content text")]
    pub text: String,

    #[clap(
        long,
        default_value = "semantic",
        help = "Memory type (episodic, semantic, procedural)"
    )]
    pub r#type: String,
}

impl MemoryCommand {
    pub async fn execute(&self, store: &LanceStore, format: OutputFormat) -> CliResult<()> {
        match &self.command {
            MemorySubcommand::List(args) => Self::list(store, args, format).await,
            MemorySubcommand::Show(args) => Self::show(store, args, format).await,
            MemorySubcommand::Delete(args) => Self::delete(store, args, format).await,
            MemorySubcommand::Globalize(args) => Self::globalize(store, args, format).await,
            MemorySubcommand::Add(args) => Self::add(store, args, format).await,
        }
    }

    async fn list(store: &LanceStore, args: &ListArgs, format: OutputFormat) -> CliResult<()> {
        let type_filter: Option<MemoryType> = match args.r#type.as_deref() {
            Some("episodic") => Some(MemoryType::Episodic),
            Some("semantic") => Some(MemoryType::Semantic),
            Some("procedural") => Some(MemoryType::Procedural),
            Some(t) => {
                return Err(format!(
                    "Unknown memory type: {t}. Use episodic, semantic, or procedural."
                )
                .into());
            }
            None => None,
        };

        let mut memories = Vec::new();
        for tier in [StorageTier::Hot, StorageTier::Warm, StorageTier::Cold] {
            let tier_memories = store.list_by_tier(tier).await?;
            memories.extend(tier_memories);
        }

        if let Some(type_filter) = type_filter {
            memories.retain(|m| m.memory_type == type_filter);
        }

        // Session filtering
        if let Some(ref session_id) = args.session {
            memories.retain(|m| m.conversation_id.as_deref() == Some(session_id.as_str()));
        } else if args.global {
            memories.retain(|m| m.conversation_id.is_none());
        }

        memories.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        memories.truncate(args.limit);

        match format {
            OutputFormat::Json => {
                let output: Vec<_> = memories
                    .iter()
                    .map(|m| {
                        serde_json::json!({
                            "id": m.id.to_string(),
                            "content": &m.content,
                            "type": format!("{:?}", m.memory_type),
                            "weight": m.weight,
                            "tier": format!("{:?}", m.tier),
                            "created_at": m.created_at.to_rfc3339(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                if memories.is_empty() {
                    println!("No memories found.");
                    return Ok(());
                }

                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL_CONDENSED)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header(["ID", "Content", "Type", "Weight", "Tier", "Created"]);

                for memory in &memories {
                    table.add_row([
                        truncate_string(&memory.id.to_string(), 8),
                        truncate_string(&memory.content, 50),
                        format!("{:?}", memory.memory_type),
                        format!("{:.2}", memory.weight),
                        format!("{:?}", memory.tier),
                        format_timestamp(&memory.created_at),
                    ]);
                }

                println!("{table}");
                println!("\nTotal: {} memories", memories.len());
            }
        }

        Ok(())
    }

    async fn show(store: &LanceStore, args: &ShowArgs, format: OutputFormat) -> CliResult<()> {
        let id = Uuid::parse_str(&args.id).map_err(|e| format!("Invalid UUID format: {e}"))?;

        let memory = store
            .get(id)
            .await?
            .ok_or_else(|| format!("Memory not found: {}", args.id))?;

        match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "id": memory.id.to_string(),
                    "content": &memory.content,
                    "embedding_size": memory.embedding.len(),
                    "type": format!("{:?}", memory.memory_type),
                    "weight": memory.weight,
                    "tier": format!("{:?}", memory.tier),
                    "compression": format!("{:?}", memory.compression),
                    "source": format!("{:?}", memory.source),
                    "created_at": memory.created_at.to_rfc3339(),
                    "last_accessed": memory.last_accessed.to_rfc3339(),
                    "access_count": memory.access_count,
                    "conversation_id": memory.conversation_id,
                    "entities": memory.entities,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL_CONDENSED)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header(["Property", "Value"]);

                table.add_row(["ID", &memory.id.to_string()]);
                table.add_row(["Content", &memory.content]);
                table.add_row(["Type", &format!("{:?}", memory.memory_type)]);
                table.add_row(["Weight", &format!("{:.4}", memory.weight)]);
                table.add_row(["Tier", &format!("{:?}", memory.tier)]);
                table.add_row(["Compression", &format!("{:?}", memory.compression)]);
                table.add_row(["Source", &format!("{:?}", memory.source)]);
                table.add_row(["Created", &memory.created_at.to_rfc3339()]);
                table.add_row(["Last Accessed", &memory.last_accessed.to_rfc3339()]);
                table.add_row(["Access Count", &memory.access_count.to_string()]);
                table.add_row([
                    "Conversation ID",
                    memory.conversation_id.as_deref().unwrap_or("-"),
                ]);
                table.add_row(["Entities", &memory.entities.join(", ")]);
                table.add_row(["Embedding Size", &memory.embedding.len().to_string()]);

                println!("{table}");
            }
        }

        Ok(())
    }

    async fn delete(store: &LanceStore, args: &DeleteArgs, format: OutputFormat) -> CliResult<()> {
        let id = Uuid::parse_str(&args.id).map_err(|e| format!("Invalid UUID format: {e}"))?;

        let deleted = store.delete(id).await?;

        match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "id": args.id,
                    "deleted": deleted,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                if deleted {
                    println!("Memory {} deleted successfully.", args.id);
                } else {
                    println!("Memory {} not found.", args.id);
                }
            }
        }

        Ok(())
    }

    async fn globalize(store: &LanceStore, args: &GlobalizeArgs, format: OutputFormat) -> CliResult<()> {
        let id = Uuid::parse_str(&args.id).map_err(|e| format!("Invalid UUID format: {e}"))?;

        let memory = store
            .get(id)
            .await?
            .ok_or_else(|| format!("Memory not found: {}", args.id))?;

        if memory.conversation_id.is_none() {
            match format {
                OutputFormat::Json => {
                    let output = serde_json::json!({
                        "id": args.id,
                        "already_global": true,
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
                OutputFormat::Table => {
                    println!("Memory {} is already global.", args.id);
                }
            }
            return Ok(());
        }

        store.update_conversation_id(id, None).await?;

        match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "id": args.id,
                    "globalized": true,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                println!("Memory {} has been globalized.", args.id);
            }
        }

        Ok(())
    }

    async fn add(store: &LanceStore, args: &AddArgs, format: OutputFormat) -> CliResult<()> {
        let memory_type = match args.r#type.as_str() {
            "episodic" => MemoryType::Episodic,
            "semantic" => MemoryType::Semantic,
            "procedural" => MemoryType::Procedural,
            t => {
                return Err(format!(
                    "Unknown memory type: {t}. Use episodic, semantic, or procedural."
                )
                .into());
            }
        };

        let embedding_model = mnemo_server::embedding::EmbeddingModel::new()?;
        let embedding = embedding_model.embed(&args.text)?;

        let memory = Memory::new(
            args.text.clone(),
            embedding,
            memory_type,
            MemorySource::Manual,
        );

        let id = memory.id;
        store.insert(&memory).await?;

        match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "id": id.to_string(),
                    "created": true,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                println!("Memory created successfully.");
                println!("ID: {id}");
            }
        }

        Ok(())
    }
}
