//! Persisted login session (cookies) so you don't re-authenticate every call.
//!
//! Stored under the OS state dir (NOT the repo) with owner-only permissions on
//! Unix. Contains portal auth cookies — treat it like a password.

use crate::config::Config;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A saved session: the raw cookies to replay and when we captured them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    /// Raw `name=value` cookie strings captured from the portal.
    pub cookies: Vec<String>,
    /// Base URL the cookies belong to.
    pub base_url: String,
    /// Username the session was created for (for display; not a secret).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Unix epoch seconds when saved.
    pub saved_at: i64,
}

impl Session {
    pub fn path() -> Result<PathBuf> {
        Ok(Config::state_dir()?.join("session.json"))
    }

    /// Load a saved session if one exists.
    pub fn load() -> Result<Option<Session>> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;
        Ok(Some(serde_json::from_str(&raw)?))
    }

    /// Persist the session with restrictive permissions.
    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        restrict_permissions(&path);
        Ok(path)
    }

    /// Delete the saved session (no error if absent).
    pub fn clear() -> Result<()> {
        let path = Self::path()?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    pub fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) {}
