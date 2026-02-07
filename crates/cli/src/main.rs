use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use fphoto_renamer_core::{
    app_paths, apply_plan, generate_plan, load_config, parse_template, undo_last, PlanOptions,
};

#[derive(Debug, Parser)]
#[command(name = "fphoto-renamer-cli")]
#[command(about = "JPG写真のファイル名をテンプレートで一括リネームします")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Rename(RenameArgs),
    Undo,
    Config(ConfigArgs),
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    action: ConfigAction,
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    Show,
}

#[derive(Debug, Args)]
struct RenameArgs {
    #[arg(long)]
    jpg_input: String,
    #[arg(long)]
    raw_input: Option<String>,
    #[arg(long, default_value_t = false)]
    recursive: bool,
    #[arg(long, default_value_t = false)]
    include_hidden: bool,
    #[arg(long, default_value_t = false)]
    apply: bool,
    #[arg(
        long,
        default_value = "{year}{month}{day}{hour}{minute}{second}_{camera_make}_{camera_model}_{lens_make}_{lens_model}_{film_sim}_{orig_name}"
    )]
    template: String,
    #[arg(long)]
    exclude: Vec<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    output: OutputFormat,
    #[arg(long)]
    tokens: Option<String>,
    #[arg(long)]
    delimiter: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Rename(args) => cmd_rename(args),
        Commands::Undo => cmd_undo(),
        Commands::Config(config) => match config.action {
            ConfigAction::Show => cmd_config_show(),
        },
    }
}

fn cmd_rename(args: RenameArgs) -> Result<()> {
    if args.tokens.is_some() || args.delimiter.is_some() {
        anyhow::bail!("--tokens / --delimiter は廃止されました。--template を使用してください。");
    }

    parse_template(&args.template)?;

    let options = PlanOptions {
        jpg_input: args.jpg_input.into(),
        raw_input: args.raw_input.map(Into::into),
        recursive: args.recursive,
        include_hidden: args.include_hidden,
        template: args.template,
        dedupe_same_maker: true,
        exclusions: args.exclude,
        max_filename_len: 240,
    };

    let plan = generate_plan(&options)?;

    match args.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        }
        OutputFormat::Table => {
            print_table(&plan);
        }
    }

    if args.apply {
        let result = apply_plan(&plan)?;
        eprintln!(
            "適用完了: {}件 (変更なし {}件)",
            result.applied, result.unchanged
        );
    } else {
        eprintln!("dry-runモード: 実ファイルは変更していません。適用するには --apply を指定してください。");
    }

    Ok(())
}

fn cmd_undo() -> Result<()> {
    let result = undo_last()?;
    println!("取り消し完了: {}件", result.restored);
    Ok(())
}

fn cmd_config_show() -> Result<()> {
    let config = load_config()?;
    let paths = app_paths()?;
    println!("設定ファイル: {}", paths.config_path.display());
    println!("{}", toml::to_string_pretty(&config)?);
    Ok(())
}

fn print_table(plan: &fphoto_renamer_core::RenamePlan) {
    println!("元ファイル -> 新ファイル (source)");
    for candidate in &plan.candidates {
        println!(
            "{} -> {} ({:?})",
            candidate.original_path.display(),
            candidate.target_path.display(),
            candidate.metadata_source
        );
    }

    println!(
        "\n集計: scanned={} jpg={} non_jpg_skip={} hidden_skip={} planned={} unchanged={}",
        plan.stats.scanned_files,
        plan.stats.jpg_files,
        plan.stats.skipped_non_jpg,
        plan.stats.skipped_hidden,
        plan.stats.planned,
        plan.stats.unchanged
    );
}
