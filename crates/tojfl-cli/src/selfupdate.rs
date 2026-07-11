//! `tojfl self-update` — replace the running binary with the latest GitHub
//! release build, via the family updater (`pk-cli-selfupdate`). Release
//! assets embed the Rust target triple, baked in by `build.rs`.

use anyhow::Result;
use pk_cli_selfupdate::{SelfUpdateArgs, Updater};

pub fn run(args: &SelfUpdateArgs, json: bool, quiet: bool) -> Result<()> {
    Updater {
        repo: "piekstra/town-of-jupiter-fl-cli".into(),
        binary: "tojfl".into(),
        target: env!("BUILD_TARGET").into(),
        current: env!("CARGO_PKG_VERSION").into(),
    }
    .run(args, json, quiet)
    .map_err(anyhow::Error::new)
}
