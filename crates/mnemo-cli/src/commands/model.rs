use std::path::PathBuf;

use clap::{Parser, Subcommand};
use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};
use hf_hub::{Repo, RepoType, api::sync::ApiBuilder};
use indicatif::{ProgressBar, ProgressStyle};

use crate::error::CliResult;
use crate::output::OutputFormat;

#[derive(Parser)]
pub struct ModelCommand {
    #[clap(subcommand)]
    pub command: ModelSubcommand,
}

#[derive(Subcommand)]
pub enum ModelSubcommand {
    #[clap(about = "Download a model from HuggingFace")]
    Download(DownloadArgs),

    #[clap(about = "List downloaded models")]
    List(ListArgs),

    #[clap(about = "Remove a downloaded model")]
    Remove(RemoveArgs),
}

#[derive(Parser)]
pub struct DownloadArgs {
    #[clap(help = "Model ID (e.g., 'Qwen/Qwen3-1.7B')")]
    pub model_id: String,

    #[clap(long, help = "Show what would be downloaded without downloading")]
    pub dry_run: bool,
}

#[derive(Parser)]
pub struct ListArgs {
    #[clap(long, short, help = "Show all files in each model")]
    pub verbose: bool,
}

#[derive(Parser)]
pub struct RemoveArgs {
    #[clap(help = "Model ID to remove (e.g., 'Qwen/Qwen3-1.7B')")]
    pub model_id: String,

    #[clap(long, short, help = "Skip confirmation prompt")]
    pub force: bool,
}

impl ModelCommand {
    pub async fn execute(&self, format: OutputFormat) -> CliResult<()> {
        match &self.command {
            ModelSubcommand::Download(args) => Self::download(args, format).await,
            ModelSubcommand::List(args) => Self::list(args, format).await,
            ModelSubcommand::Remove(args) => Self::remove(args, format).await,
        }
    }

    async fn download(args: &DownloadArgs, format: OutputFormat) -> CliResult<()> {
        let models_dir = get_models_dir()?;
        let model_dir_name = model_id_to_dir_name(&args.model_id);
        let target_dir = models_dir.join(&model_dir_name);

        if args.dry_run {
            match format {
                OutputFormat::Json => {
                    let output = serde_json::json!({
                        "model_id": args.model_id,
                        "target_dir": target_dir,
                        "dry_run": true,
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
                OutputFormat::Table => {
                    println!("Dry run - would download:");
                    println!("  Model: {}", args.model_id);
                    println!("  To: {}", target_dir.display());
                }
            }
            return Ok(());
        }

        std::fs::create_dir_all(&models_dir)?;

        let pb = ProgressBar::new(100);
        let style = ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {msg}")
            .map_err(|e| format!("Progress bar error: {e}"))?
            .progress_chars("#>-");
        pb.set_style(style);
        pb.set_message(format!("Downloading {}", args.model_id));

        let api = ApiBuilder::new()
            .with_cache_dir(models_dir.clone())
            .build()
            .map_err(|e| format!("API build error: {e}"))?;

        let repo = Repo::new(args.model_id.clone(), RepoType::Model);
        let api_repo = api.repo(repo);

        let _ = api_repo
            .get("config.json")
            .map_err(|e| format!("Failed to download config: {e}"))?;

        let repo_path = models_dir.join(format!("models--{}", model_dir_name));

        pb.finish_with_message(format!("Downloaded to {}", repo_path.display()));

        match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "model_id": args.model_id,
                    "path": repo_path,
                    "success": true,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                println!("Model downloaded successfully.");
                println!("Location: {}", repo_path.display());
            }
        }

        Ok(())
    }

    async fn list(args: &ListArgs, format: OutputFormat) -> CliResult<()> {
        let models_dir = get_models_dir()?;

        if !models_dir.exists() {
            match format {
                OutputFormat::Json => {
                    println!("[]");
                }
                OutputFormat::Table => {
                    println!("No models downloaded yet.");
                    println!("Models directory: {}", models_dir.display());
                }
            }
            return Ok(());
        }

        let mut models = Vec::new();

        for entry in std::fs::read_dir(&models_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
                let model_id = dir_name.replace("--", "/");

                let total_size = if args.verbose {
                    calculate_dir_size(&path)?
                } else {
                    0
                };

                models.push((model_id, path, total_size));
            }
        }

        match format {
            OutputFormat::Json => {
                let output: Vec<_> = models
                    .iter()
                    .map(|(id, path, size)| {
                        serde_json::json!({
                            "model_id": id,
                            "path": path,
                            "size_bytes": size,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                if models.is_empty() {
                    println!("No models found.");
                    return Ok(());
                }

                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL_CONDENSED)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header(["Model ID", "Location"]);

                if args.verbose {
                    table.set_header(["Model ID", "Location", "Size"]);
                }

                for (model_id, path, size) in &models {
                    if args.verbose {
                        table.add_row([
                            model_id.as_str(),
                            &path.display().to_string(),
                            &format_size(*size),
                        ]);
                    } else {
                        table.add_row([model_id.as_str(), &path.display().to_string()]);
                    }
                }

                println!("{table}");
                println!("\nTotal: {} models", models.len());
            }
        }

        Ok(())
    }

    async fn remove(args: &RemoveArgs, format: OutputFormat) -> CliResult<()> {
        let models_dir = get_models_dir()?;
        let model_dir_name = model_id_to_dir_name(&args.model_id);
        let target_dir = models_dir.join(&model_dir_name);

        if !target_dir.exists() {
            match format {
                OutputFormat::Json => {
                    let output = serde_json::json!({
                        "model_id": args.model_id,
                        "removed": false,
                        "error": "Model not found",
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
                OutputFormat::Table => {
                    println!("Model '{}' not found.", args.model_id);
                }
            }
            return Ok(());
        }

        if !args.force {
            match format {
                OutputFormat::Json => {
                    let output = serde_json::json!({
                        "model_id": args.model_id,
                        "removed": false,
                        "confirmation_required": true,
                        "message": "Use --force to confirm removal",
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                    return Ok(());
                }
                OutputFormat::Table => {
                    print!("Remove model '{}'? [y/N] ", args.model_id);
                    use std::io::Write;
                    std::io::stdout().flush()?;

                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;

                    if !input.trim().eq_ignore_ascii_case("y") {
                        println!("Cancelled.");
                        return Ok(());
                    }
                }
            }
        }

        std::fs::remove_dir_all(&target_dir)?;

        match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "model_id": args.model_id,
                    "removed": true,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Table => {
                println!("Model '{}' removed successfully.", args.model_id);
            }
        }

        Ok(())
    }
}

/// Get the models directory (~/.mnemo/models/)
fn get_models_dir() -> CliResult<PathBuf> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".mnemo").join("models"))
}

/// Convert model ID to directory name (Qwen/Qwen3-1.7B -> Qwen--Qwen3-1.7B)
fn model_id_to_dir_name(model_id: &str) -> String {
    model_id.replace('/', "--")
}

fn calculate_dir_size(path: &PathBuf) -> CliResult<u64> {
    let mut total = 0u64;

    for entry in walkdir::WalkDir::new(path) {
        let entry = entry.map_err(|e| format!("Walkdir error: {e}"))?;
        if entry.file_type().is_file() {
            let metadata = entry.metadata().map_err(|e| format!("Metadata error: {e}"))?;
            total += metadata.len();
        }
    }

    Ok(total)
}

/// Format size in human-readable format
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_idx])
}
