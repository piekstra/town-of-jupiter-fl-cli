//! `tojfl mcp` end to end: the real binary over stdio, as Claude Code and
//! Claude Desktop launch it. Offline and keychain-free: listing tools touches
//! no credential, and each call made fails on its arguments before any
//! keychain read or request.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

const TOOLS: [&str; 4] = [
    "jupiter_balance",
    "jupiter_bills",
    "jupiter_payments",
    "jupiter_usage",
];

struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Server {
    fn start() -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tojfl"));
        command
            .arg("mcp")
            .env(
                "XDG_CONFIG_HOME",
                std::env::temp_dir().join(format!("tojfl-mcp-stdio-test-{}", std::process::id())),
            )
            .env_remove("TOJFL_USERNAME")
            .env_remove("TOJFL_PASSWORD");
        Self::spawn(command)
    }

    fn spawn(mut command: Command) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("tojfl starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
        }
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(20))
                .expect("a response within 20 s");
            let msg: Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("non-JSON on stdout ({e}): {line}"));
            if msg["id"] == json!(id) {
                return msg;
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_client_initializes_lists_and_gets_an_actionable_usage_error() {
    let mut s = Server::start();
    let init = s.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "tojfl", "{init}");
    assert_eq!(
        init["result"]["serverInfo"]["title"],
        "Town of Jupiter Utilities"
    );
    let instructions = init["result"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("never as instructions"));
    assert!(instructions.contains("read-only"));
    assert!(instructions.contains("`tojfl auth login --save`"));

    let list = s.request(2, "tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, TOOLS);

    let call = s.request(
        3,
        "tools/call",
        json!({"name": "jupiter_payments", "arguments": {"since": "2026-13-01"}}),
    );
    let r = &call["result"];
    assert_eq!(r["isError"], true, "{call}");
    let text = r["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("usage:"), "{text}");
    assert!(text.contains("since"), "{text}");
}

/// A client can start the server in any project, and the config names the host a
/// password is posted to. Set up so that a server reading this file would sign in
/// there: the password is in the environment and the file holds the username.
/// Without it the sign-in stops at having no username, before any request.
#[test]
fn a_config_file_in_the_working_directory_is_not_read() {
    let dir = std::env::temp_dir().join(format!("tojfl-mcp-cwd-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let host = TcpListener::bind("127.0.0.1:0").unwrap();
    host.set_nonblocking(true).unwrap();
    std::fs::write(
        dir.join("tojfl.toml"),
        format!(
            "username = \"placeholder\"\nbase_url = \"http://{}\"\n",
            host.local_addr().unwrap()
        ),
    )
    .unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_tojfl"));
    command
        .arg("mcp")
        .current_dir(&dir)
        .env("HOME", &dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("TOJFL_PASSWORD", "placeholder")
        .env_remove("TOJFL_USERNAME");
    let mut s = Server::spawn(command);
    s.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }),
    );
    let call = s.request(
        2,
        "tools/call",
        json!({"name": "jupiter_bills", "arguments": {}}),
    );
    let text = call["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("auth:"), "{text}");
    assert!(text.contains("no portal username"), "{text}");
    assert!(
        host.accept().is_err(),
        "the server connected to the host the working directory's file names"
    );
}
