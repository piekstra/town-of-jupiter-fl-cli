//! Persisted login session (cookies) so you don't re-authenticate every call.
//!
//! Stored under the OS state dir (NOT the repo) with owner-only permissions on
//! Unix. Contains portal auth cookies — treat it like a password.

use crate::config::Config;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

    /// Load a saved session if one exists. A file that cannot be parsed counts as
    /// no session, so signing in or logging out can still replace it.
    pub fn load() -> Result<Option<Session>> {
        Self::load_from(&Self::path()?)
    }

    fn load_from(path: &Path) -> Result<Option<Session>> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        match serde_json::from_str(&raw) {
            Ok(session) => Ok(Some(session)),
            // Not the parse error: it can quote the file, which holds cookies.
            Err(_) => {
                eprintln!(
                    "warning: ignoring {}, which could not be parsed; sign in again",
                    path.display()
                );
                Ok(None)
            }
        }
    }

    /// Persist the session with restrictive permissions.
    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::path()?;
        self.save_to(&path)?;
        Ok(path)
    }

    /// Through a temporary file and a rename: commands and running servers share
    /// this file, and none of them should read half a write.
    fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        let written = write_private(&temp, &json).and_then(|()| std::fs::rename(&temp, path));
        if written.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        Ok(written?)
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
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(contents.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new empty directory; these tests never reach the real state directory.
    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tojfl-session-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn session(username: &str) -> Session {
        Session {
            cookies: vec!["name=value".into()],
            base_url: "https://portal.example".into(),
            username: Some(username.into()),
            saved_at: 1,
        }
    }

    #[test]
    fn a_saved_session_loads_and_a_later_save_replaces_it_leaving_no_temporary_file() {
        let dir = dir("round-trip");
        let path = dir.join("session.json");
        session("first").save_to(&path).unwrap();
        session("second").save_to(&path).unwrap();
        let loaded = Session::load_from(&path).unwrap().unwrap();
        assert_eq!(loaded.username.as_deref(), Some("second"));
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(files.len(), 1, "{files:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_saved_session_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = dir("mode").join("session.json");
        session("user").save_to(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn no_file_and_a_damaged_file_are_both_no_session() {
        let path = dir("damaged").join("session.json");
        assert!(Session::load_from(&path).unwrap().is_none());
        std::fs::write(&path, "{\"cookies\": [\"name=val").unwrap();
        assert!(Session::load_from(&path).unwrap().is_none());
    }
}
