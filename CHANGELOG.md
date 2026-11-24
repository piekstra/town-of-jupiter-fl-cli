# Changelog

## v0.5.3 — 2026-11-04

- `jupiter_bills`: a bill's id is its date. It was the bill's count from the newest, which moved every bill when one arrived, so a bill arriving between pages could repeat a row. `bills get <N>` keeps its count.
- `auth logout --forget` speaks to a server on `$TOJFL_MCP_TOKEN` too, which never had a keychain item to remove.
- cli-common v0.29.0, up from v0.27.0.

## v0.5.2 — 2026-11-03

- `auth logout --forget` also removes the connector token `mcp-http-token`, and says what that
  does not end: a running `tojfl mcp --http` keeps the token it started with. Its `--json` answer
  gains `connector_token_removed`. Every step of a forget is attempted, and every failure reported.
- cli-common v0.27.0, up from v0.24.0.

## v0.5.1 — 2026-11-03

### Fixed
- A `base_url` off `https` is refused, and so is a form action, a bill link or a redirect
  from the portal that leaves `https`. Any of them would have put the password or the
  session cookies in the clear. Plain `http` to this machine still works for a
  `base_url`, not for a redirect from `https`.
- A form goes only to the portal's own host. A login page naming another host in its
  form action is refused with exit 5, with the password unsent.
- A network error no longer prints the URL. A statement link's query holds an
  account-specific token. Built on cli-common v0.24.0.

## v0.5.0 — 2026-11-03

### Added
- **`tojfl mcp`**: read-only tools for Claude, the `utility/v1` set from cli-common's
  `pk-cli-utility`: `jupiter_balance`, `jupiter_bills`, `jupiter_payments` and
  `jupiter_usage`. They run over stdio for Claude Code and Claude Desktop, and with
  `--http` on 127.0.0.1 behind a bearer token for Claude on the web and mobile through
  your own Cloudflare Worker and tunnel.
- Releases attach `tojfl-<version>.mcpb`, a Claude Desktop extension, and a Windows
  x86_64 tarball.
- `self-update` says to restart a running `tojfl mcp --http` and to install the new
  `.mcpb`.

### Changed
- cli-common v0.21.0, up from v0.15.1. Rust 1.89 or later.
