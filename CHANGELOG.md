# Changelog

## v0.5.1 — 2026-11-03

### Fixed
- A `base_url` off `https` is refused, and so is a form action, a bill link or a redirect
  from the portal that leaves `https`. Any of them would have put the password or the
  session cookies in the clear. Plain `http` to this machine still works.
- Built on cli-common v0.24.0.

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
