# Security policy

## Reporting

Open a private security advisory on the repository (Security → Advisories → New
draft). Please do not file a public issue for anything credential-related.

## What this tool holds

`tojfl` signs in to the Town of Jupiter's utility portal with your own username and
password. It has no telemetry, and no server component unless you run `tojfl mcp
--http` (see [The MCP server](#the-mcp-server)). Everything it stores is local, except
the copy of the `--http` bearer token you give your own Cloudflare Worker:

| What | Where | Notes |
| --- | --- | --- |
| Portal password | OS keychain, service `tojfl`, item `portal-password`, saved by `tojfl auth login --save`. `$TOJFL_PASSWORD` comes first; a `password` in the config file is the last resort. | Never logged or printed. |
| Portal session | A file in the OS state directory, owner-only, shared by the commands and every running server | The cookies of the last sign-in. `tojfl auth logout` removes it. A file that cannot be parsed counts as no session. |
| `--http` bearer token | OS keychain, service `piekstra.tojfl`, item `mcp-http-token`, or `TOJFL_MCP_TOKEN` | A copy lives in your Cloudflare Worker. |
| Non-secret settings | The config file in the OS config directory, or `./tojfl.toml` | Username, default account, base URL. `tojfl mcp` reads only the OS config path or `--config`: a client can start it in any directory, and a `tojfl.toml` there could name another host to send the password to. |

## The MCP server

`tojfl mcp` serves read-only tools to an MCP client (Claude Code, Claude Desktop) over
stdio; the server opens no port unless run with `--http` (below). **What the tools
return goes to Claude on every path**, as part of your conversation: your balance, due
date, statements, payments, water use, service address and, where a tool lists accounts
or the detailed format is asked for, the account number.

It uses the saved session and, when that has lapsed, signs in to the portal as the
commands' automatic sign-in does, which rewrites the session file. A tool never prompts
and never creates or changes a keychain item, and a missing or refused password is an
error asking the user to run `tojfl auth login --save` in a terminal. While
`$TOJFL_PASSWORD` is set, the keychain is not read, so the error asks the user to update
the password where it is set instead. With no saved session, a stored password signs in
only if `$TOJFL_PASSWORD` provides it, so `tojfl auth logout` stays logged out, and
`auto_login = false` in the config stops the server signing in at all. A password the
portal has refused is not sent again by a running server, so a stale one cannot lock
the login; the next call that finds a different password or username tries that.
Starting the server again sends it once more. A session saved for a different username
from the configured one is not used: the server signs in as the configured one, which
replaces it.

Which account the portal reports is state on that shared session. Every call selects
its account first, and if another process has switched it by the time the read is done,
the tool answers with an error rather than another account's rows.

The Claude Desktop extension (`.mcpb`) holds the username and password in Claude
Desktop's secure store (`user_config`, the password `sensitive`) and passes them to its
bundled binary as `$TOJFL_USERNAME` and `$TOJFL_PASSWORD`, so it reads no keychain item
for them. It writes the session file as the commands do.

`tojfl mcp --http --port <PORT>` serves the same tools over HTTP for the user's own
Cloudflare Worker, which reaches it through the user's own Cloudflare Tunnel
(cli-common's `connector/`). It binds 127.0.0.1 only, with no flag for another address,
and refuses to start without a bearer token of 32 or more characters, from
`TOJFL_MCP_TOKEN` or the keychain item `mcp-http-token` (`piekstra.tojfl`, a different
service from the password's), which `tojfl mcp --new-token` makes from 256 random bits
and replaces only with `--overwrite`. **A copy of the token lives in your Cloudflare
Worker**, as its `HOME_JUPITER_TOKEN` secret. Every request must carry the token
(compared in constant time) and a `Host` of `127.0.0.1` or `localhost`, and must not
carry an `Origin`; there are no CORS headers. Only the read-only tools are served (all
four are). The password stays on your computer: the Worker sends tool calls and receives
results, and never sees the password or a portal session.

**Tool results also cross Cloudflare on this path**, through your tunnel and your
Worker, on their way to Claude. Who holds what, and what a leaked token can do, is in
the connector's
[threat model](https://github.com/piekstra/cli-common/blob/main/connector/README.md#threat-model).

`tojfl auth logout --forget` removes the password and does not touch `mcp-http-token` or
stop `tojfl mcp --http`. To revoke the connector's access, run `tojfl mcp --new-token
--overwrite` and restart the server (`launchctl kickstart -k gui/$(id -u)/com.piekstra.tojfl-mcp-http`);
the old token works until the restart. To stop serving, run `launchctl bootout
gui/$(id -u)/com.piekstra.tojfl-mcp-http` and stop the tunnel.

## No personal data in the repo

Fixtures use synthetic data, and no raw capture is committed.
