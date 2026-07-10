//! `tojfl` — command-line client for the Town of Jupiter, FL utility portal.

mod cli;
mod commands;
mod output;

use clap::Parser;
use cli::{Cli, Command};
use commands::Ctx;
use output::Format;
use std::path::Path;
use std::process::ExitCode;
use tojfl_sdk::Config;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    // Load config (explicit path, else default discovery) and apply CLI overrides.
    let mut cfg = match &cli.global.config {
        Some(path) => Config::load_from(Path::new(path))?,
        None => Config::load()?,
    };
    if let Some(u) = &cli.global.username {
        cfg.username = Some(u.clone());
    }
    if let Some(b) = &cli.global.base_url {
        cfg.base_url = Some(b.clone());
    }
    if let Some(a) = &cli.global.account {
        cfg.default_account = Some(a.clone());
    }

    // JSON is on if --json is passed or config sets output = "json".
    let json = cli.global.json || cfg.output.as_deref() == Some("json");

    let ctx = Ctx {
        fmt: Format::new(json),
        username: cli.global.username.clone(),
        verbose: cli.global.verbose,
        cfg,
    };

    match &cli.command {
        Command::Auth(c) => commands::auth(&ctx, c),
        Command::Account(c) => commands::account(&ctx, c),
        Command::Balance => commands::balance(&ctx),
        Command::Bills(c) => commands::bills(&ctx, c),
        Command::Usage(c) => commands::usage(&ctx, c),
        Command::Transactions(c) => commands::transactions(&ctx, c),
        Command::Pay(c) => commands::pay(&ctx, c),
        Command::Profile(c) => commands::profile(&ctx, c),
        Command::Ebill(c) => commands::ebill(&ctx, c),
        Command::Contact => commands::contact(&ctx),
        Command::Config(c) => commands::config_cmd(&ctx, c),
    }
}
