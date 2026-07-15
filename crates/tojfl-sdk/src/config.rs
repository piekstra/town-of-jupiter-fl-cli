//! Configuration and credential resolution.
//!
//! Nothing sensitive is ever written to the repo. At runtime, config lives in
//! the OS config dir (see [`Config::default_path`]) or a local `tojfl.toml`,
//! and the password is preferably kept in the OS keychain (see [`credentials`]).

use crate::error::{Error, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "tojfl";
const KEYRING_USER: &str = "portal-password";

/// On-disk configuration (TOML). All fields optional so an empty file works.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Portal login username.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Password. Discouraged here — prefer the keychain. Kept for parity/tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Default account number to operate on when several are linked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_account: Option<String>,
    /// Override the portal base URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Default output format ("table" | "json" | "csv").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// HTTP timeout in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

impl Config {
    /// The default OS-specific config file path (created lazily on save).
    pub fn default_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("us", "piekstra", "tojfl")
            .ok_or_else(|| Error::Config("could not determine config directory".into()))?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// The OS-specific state dir for sessions/cache.
    pub fn state_dir() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("us", "piekstra", "tojfl")
            .ok_or_else(|| Error::Config("could not determine state directory".into()))?;
        // data_local_dir is the most broadly available "state" location.
        Ok(dirs.data_local_dir().to_path_buf())
    }

    /// Load config, checking `./tojfl.toml` first, then the OS config path.
    /// A missing file is not an error — you get defaults.
    pub fn load() -> Result<Config> {
        let local = Path::new("tojfl.toml");
        if local.exists() {
            return Self::load_from(local);
        }
        let path = Self::default_path()?;
        if path.exists() {
            return Self::load_from(&path);
        }
        Ok(Config::default())
    }

    /// Load config from a specific file.
    pub fn load_from(path: &Path) -> Result<Config> {
        let raw = std::fs::read_to_string(path)?;
        toml::from_str(&raw).map_err(|e| Error::Config(format!("{}: {e}", path.display())))
    }

    /// Persist config to the default path, creating parent dirs.
    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::default_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let toml = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("serialize config: {e}")))?;
        std::fs::write(&path, toml)?;
        Ok(path)
    }
}

/// A resolved credential pair ready to authenticate with.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Resolve credentials from (in priority order): explicit args, environment
/// (`TOJFL_USERNAME`/`TOJFL_PASSWORD`), the OS keychain, then config file.
///
/// The username may come from one source and the password from another (e.g.
/// username in config, password in keychain) — that's the common setup.
pub fn credentials(
    cfg: &Config,
    arg_username: Option<&str>,
    arg_password: Option<&str>,
) -> Result<Credentials> {
    let username = arg_username
        .map(str::to_string)
        .or_else(|| std::env::var("TOJFL_USERNAME").ok())
        .or_else(|| cfg.username.clone())
        .ok_or_else(|| {
            Error::Config(
                "no username found (set TOJFL_USERNAME, config `username`, or pass --username)"
                    .into(),
            )
        })?;

    let password = arg_password
        .map(str::to_string)
        .or_else(|| std::env::var("TOJFL_PASSWORD").ok())
        .or_else(|| keychain_get().ok().flatten())
        .or_else(|| cfg.password.clone())
        .ok_or_else(|| {
            Error::Config(
                "no password found (run `tojfl auth login --save`, or set TOJFL_PASSWORD)".into(),
            )
        })?;

    Ok(Credentials { username, password })
}

/// Store the password in the OS keychain.
pub fn keychain_set(password: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| Error::Keychain(e.to_string()))?;
    entry
        .set_password(password)
        .map_err(|e| Error::Keychain(e.to_string()))
}

/// Fetch the password from the OS keychain, if present.
pub fn keychain_get() -> Result<Option<String>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| Error::Keychain(e.to_string()))?;
    match entry.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(Error::Keychain(e.to_string())),
    }
}

/// Remove the stored password from the OS keychain (no error if absent).
pub fn keychain_clear() -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| Error::Keychain(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Error::Keychain(e.to_string())),
    }
}
