//! `tojfl` — command-line client for the Town of Jupiter, FL utility portal.

mod cli;
mod commands;
mod output;
mod selfupdate;

use clap::Parser;
use cli::{Cli, Command};
use commands::Ctx;
use output::Format;
use std::path::Path;
use std::process::ExitCode;
use tojfl_sdk::Config;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json_mode = cli.global.json;
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let cli_err = to_cli_error(&err);
            if json_mode {
                pk_cli_core::output::json(&cli_err.to_json());
            }
            eprintln!("error: {err:#}");
            ExitCode::from(cli_err.exit_code() as u8)
        }
    }
}

/// Map SDK/anyhow errors onto the family exit-code contract (SPEC v1 §1.5).
fn to_cli_error(err: &anyhow::Error) -> pk_cli_core::CliError {
    use pk_cli_core::CliError;
    use tojfl_sdk::Error as E;
    if let Some(e) = err.downcast_ref::<CliError>() {
        // Re-key a family error (e.g. from the shared self-updater).
        return match e {
            CliError::Usage(m) => CliError::Usage(m.clone()),
            CliError::Auth(m) => CliError::Auth(m.clone()),
            CliError::NotFound(m) => CliError::NotFound(m.clone()),
            CliError::Upstream(m) => CliError::Upstream(m.clone()),
            CliError::ConfirmationRequired(m) => CliError::ConfirmationRequired(m.clone()),
            CliError::Keychain(m) => CliError::Keychain(m.clone()),
            CliError::Other(m) => CliError::Other(m.clone()),
        };
    }
    match err.downcast_ref::<E>() {
        Some(E::Auth(m)) => CliError::Auth(m.clone()),
        Some(E::NotAuthenticated) => CliError::Auth(E::NotAuthenticated.to_string()),
        Some(E::Http(e)) => CliError::Upstream(e.to_string()),
        Some(E::Portal(m)) | Some(E::Parse(m)) => CliError::Upstream(m.clone()),
        Some(E::MissingFormField(m)) => CliError::Upstream(m.clone()),
        Some(E::Invalid(m)) | Some(E::Config(m)) => CliError::Usage(m.clone()),
        Some(E::NotFound(m)) => CliError::NotFound(m.clone()),
        Some(E::Keychain(m)) => CliError::Keychain(m.clone()),
        _ => CliError::Other(format!("{err:#}")),
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

    // Resolve the output mode. Explicit flags win over the config `output`
    // setting; `--json` / `--csv` are mutually exclusive at the flag layer.
    let (json, csv) = if cli.global.csv {
        (false, true)
    } else if cli.global.json {
        (true, false)
    } else {
        match cfg.output.as_deref() {
            Some("csv") => (false, true),
            Some("json") => (true, false),
            _ => (false, false),
        }
    };

    let ctx = Ctx {
        fmt: Format::new(json, csv),
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
        Command::Meters {
            limit,
            since,
            until,
        } => commands::meters(&ctx, *limit, since, until),
        Command::Transactions(c) => commands::transactions(&ctx, c),
        Command::Pay(c) => commands::pay(&ctx, c),
        Command::Profile(c) => commands::profile(&ctx, c),
        Command::Ebill(c) => commands::ebill(&ctx, c),
        Command::Summary => commands::summary(&ctx),
        Command::Snapshot { all_accounts } => commands::snapshot(&ctx, *all_accounts),
        Command::Service => commands::service(&ctx),
        Command::Contact => commands::contact(&ctx),
        Command::Config(c) => commands::config_cmd(&ctx, c),
        Command::SelfUpdate(a) => selfupdate::run(a, cli.global.json, cli.global.quiet),
        Command::Completions { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(*shell, &mut Cli::command(), "tojfl", &mut std::io::stdout());
            Ok(())
        }
        Command::Info => commands::info(&ctx),
    }
}

#[cfg(test)]
mod tests {
    use super::to_cli_error;
    use tojfl_sdk::Error as E;

    fn code(e: E) -> i32 {
        to_cli_error(&anyhow::Error::from(e)).exit_code()
    }

    #[test]
    fn sdk_errors_map_to_family_exit_codes() {
        assert_eq!(code(E::NotFound("no account".into())), 4);
        assert_eq!(code(E::NotAuthenticated), 3);
        assert_eq!(code(E::Auth("bad creds".into())), 3);
        assert_eq!(code(E::Invalid("bad flag".into())), 2);
        assert_eq!(code(E::Config("bad file".into())), 2);
        assert_eq!(code(E::Portal("rejected".into())), 5);
        assert_eq!(code(E::Keychain("locked".into())), 1);
    }
}
