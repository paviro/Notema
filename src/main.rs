use clap::Parser;
use journal::{AppResult, config, storage, tui};
use std::{
    io::{self, Read},
    path::PathBuf,
};

#[cfg(not(unix))]
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

#[derive(Debug, Parser)]
#[command(name = "journal")]
#[command(about = "Markdown terminal journal")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,

    #[arg(long, value_name = "NAME")]
    journal: Option<String>,

    #[arg(long, value_name = "NAME")]
    set_default: Option<String>,

    #[arg(value_name = "TEXT")]
    body: Vec<String>,
}

fn main() -> AppResult<()> {
    let cli = Cli::parse();

    if let Some(journal) = cli.set_default.as_deref() {
        return set_default_journal(&cli, journal);
    }

    let stdin_is_pipe = stdin_has_command_input();
    if !cli.body.is_empty() || stdin_is_pipe {
        return create_entry_from_command(cli, stdin_is_pipe);
    }
    if cli.journal.is_some() {
        return Err("--journal requires entry text or piped stdin".into());
    }

    let config = config::load_or_setup(cli.config.as_deref())?;
    storage::ensure_workspace(&config.journal_root)?;

    tui::run(config)
}

fn set_default_journal(cli: &Cli, journal: &str) -> AppResult<()> {
    if !cli.body.is_empty() {
        return Err("--set-default cannot be used with entry text".into());
    }
    if cli.journal.is_some() {
        return Err("--set-default cannot be used with --journal".into());
    }

    let (path, mut config) = config::load_existing(cli.config.as_deref())?;
    validate_existing_journal(&config.journal_root, journal)?;
    config.default_journal = Some(journal.to_string());
    config::save_config(&path, &config)?;
    println!("Default journal set to {journal}");
    Ok(())
}

fn create_entry_from_command(cli: Cli, stdin_is_pipe: bool) -> AppResult<()> {
    let body_from_args = !cli.body.is_empty();
    if body_from_args && stdin_is_pipe {
        return Err("entry text cannot be combined with piped stdin".into());
    }

    let (_, config) = config::load_existing(cli.config.as_deref())?;
    let journal = cli
        .journal
        .as_deref()
        .or(config.default_journal.as_deref())
        .ok_or(
            "no journal specified; pass --journal or set one with `journal --set-default <name>`",
        )?;
    validate_existing_journal(&config.journal_root, journal)?;

    let body = if body_from_args {
        cli.body.join(" ")
    } else {
        let mut body = String::new();
        io::stdin().read_to_string(&mut body)?;
        body
    };

    let path = storage::create_entry_with_body(&config.journal_root, journal, &body)?;
    println!("{}", path.display());
    Ok(())
}

#[cfg(unix)]
fn stdin_has_command_input() -> bool {
    std::fs::metadata("/dev/stdin")
        .map(|metadata| {
            let file_type = metadata.file_type();
            file_type.is_fifo() || file_type.is_socket() || file_type.is_file()
        })
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn stdin_has_command_input() -> bool {
    !io::stdin().is_terminal()
}

fn validate_existing_journal(root: &std::path::Path, journal: &str) -> AppResult<()> {
    let journal = storage::validate_journal_name(journal)?;
    let path = root.join(&journal);
    if !path.is_dir() {
        return Err(format!("journal '{journal}' does not exist").into());
    }
    Ok(())
}
