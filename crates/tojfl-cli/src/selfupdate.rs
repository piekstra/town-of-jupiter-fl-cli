//! `tojfl self-update` — replace the running binary with the latest GitHub
//! release build.
//!
//! Fetches releases from this repo, compares against the compiled-in version,
//! and (unless `--check`) downloads the asset for the current platform and
//! swaps the binary in place. If you installed via a package manager, prefer
//! that manager's upgrade path instead.

use crate::cli::SelfUpdateArgs;
use anyhow::{Context, Result};

const REPO_OWNER: &str = "piekstra";
const REPO_NAME: &str = "town-of-jupiter-fl";
const BIN_NAME: &str = "tojfl";

pub fn run(args: &SelfUpdateArgs) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    let updater = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .current_version(current)
        .show_download_progress(!args.check)
        .no_confirm(args.yes)
        .build()
        .context("preparing self-update")?;

    if args.check {
        let latest = updater
            .get_latest_release()
            .context("checking the latest release")?;
        let newer =
            self_update::version::bump_is_greater(current, &latest.version).unwrap_or(false);
        if newer {
            println!("Update available: {current} → {}", latest.version);
            println!("Run `tojfl self-update` to install it.");
        } else {
            println!("tojfl is up to date ({current}).");
        }
        return Ok(());
    }

    let status = updater.update().context("performing self-update")?;
    if status.updated() {
        println!("Updated tojfl {current} → {}.", status.version());
    } else {
        println!("tojfl is already up to date ({}).", status.version());
    }
    Ok(())
}
