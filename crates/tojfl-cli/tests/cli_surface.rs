//! Offline black-box tests of the `tojfl` binary. No network, no keychain,
//! no real config: each run gets an empty home and working directory, so
//! neither `./tojfl.toml` nor the OS config dir can be found.

use std::path::PathBuf;
use std::process::Command;

fn tojfl(home: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tojfl"));
    cmd.current_dir(home)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env_remove("TOJFL_USERNAME")
        .env_remove("TOJFL_PASSWORD")
        .env_remove("NO_COLOR");
    cmd
}

/// A fresh, empty directory per call; the counter keeps parallel tests apart.
fn tmp() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "tojfl-cli-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn no_color_in_the_environment_never_fails_a_command() {
    // Any non-empty NO_COLOR disables color, per no-color.org. Read as a
    // boolean flag, NO_COLOR=1 was a usage error on every command.
    let home = tmp();
    for v in ["1", "true", "0", ""] {
        let out = tojfl(&home)
            .env("NO_COLOR", v)
            .args(["completions", "bash"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "NO_COLOR={v:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
