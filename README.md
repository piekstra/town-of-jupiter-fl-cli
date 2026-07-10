# tojfl

A command-line client for the **Town of Jupiter, FL** utility billing portal
([utilitybill.jupiter.fl.us](https://utilitybill.jupiter.fl.us)).

View your account summary and balance, review billing and usage history, list
ledger transactions, look at your profile, and drive the one-time payment
lookup — all from the terminal, with `--json` on every command for scripting.

```console
$ tojfl balance
Balance due: $84.21

$ tojfl usage compare --json | jq '.[0]'
{ "period": "Jun 2026", "quantity": 3120, "change": 240, "percent": 8.3 }
```

> **Not affiliated with the Town of Jupiter.** This is an independent, personal
> tool that automates the same web pages a browser would load. There is no
> public API; it authenticates and scrapes the portal on your behalf. Use it
> with your own account and at your own discretion.

## Why this exists

The portal is a [Harris **eCARE**](https://www.harriscomputer.com/) utility
billing application hosted on **DotNetNuke (DNN)** — an ASP.NET WebForms stack.
There's no API, so everything here is built on:

- **DNN forms authentication** — a `__VIEWSTATE`/`__EVENTVALIDATION` postback to
  `Login.aspx`, with the resulting session cookie persisted locally.
- **eCARE page scraping** — the billing, usage, transaction, and profile pages
  render HTML tables, which this tool parses into typed records.

## Install

Requires a Rust toolchain (1.82+).

```bash
git clone https://github.com/piekstra/town-of-jupiter-fl
cd town-of-jupiter-fl
cargo install --path crates/tojfl-cli   # installs the `tojfl` binary
# or just: cargo build --release  ->  ./target/release/tojfl
```

### A note on TLS

This project links **native-tls** (your OS's TLS stack) rather than rustls.
That's not a style choice: the portal server only offers CBC-mode TLS 1.2
cipher suites (e.g. `ECDHE-RSA-AES256-SHA384`) with no AEAD/GCM suites and no
TLS 1.3. rustls implements only AEAD ciphers and cannot complete the handshake;
native-tls (SecureTransport on macOS, SChannel on Windows, OpenSSL on Linux)
still supports CBC and connects fine. On Linux you'll need OpenSSL dev headers.

## Getting started

1. Register on the portal itself first — you need a **7-digit customer number**
   and **6-digit account number** from your paper bill.
2. Log in once; the session is cached so later commands are instant:

```bash
tojfl auth login --save     # prompts for password, stores it in your OS keychain
tojfl auth status
tojfl account show
```

`--save` writes your **username** to a local config file and your **password**
to the OS keychain (macOS Keychain / Windows Credential Manager / Secret
Service). Nothing sensitive is written into this repository — see
[Privacy & security](#privacy--security).

## Commands

| Command | What it does |
| --- | --- |
| `tojfl auth login [--save] [--password-stdin]` | Authenticate and cache a session |
| `tojfl auth logout [--forget]` | Clear the session (and optionally the keychain password) |
| `tojfl auth status` | Report whether a valid session exists |
| `tojfl account show` | Account summary: balance, due date, service address |
| `tojfl account list` | Accounts linked to your login |
| `tojfl balance` | Just the current balance due |
| `tojfl bills list [--limit N]` | Billing history (statements) |
| `tojfl bills latest` | Most recent statement |
| `tojfl usage list [--limit N]` | Metered water usage per period |
| `tojfl usage compare` | Period-over-period consumption change (Δ and %) |
| `tojfl transactions list [--limit N]` | Ledger: charges, payments, adjustments |
| `tojfl profile show` | Account holder profile |
| `tojfl ebill status` | Paperless / autopay enrollment status |
| `tojfl pay quote -c CUST -a ACCT` | Look up an account and report the amount due (no login) |
| `tojfl pay open  -c CUST -a ACCT [--open]` | Print / open the hosted payment page URL |
| `tojfl contact` | Utility contact & service info (offline) |
| `tojfl config path\|init\|show\|set-password\|clear-password` | Manage local config & credentials |

Add `--json` to any command for machine-readable output, `-v` for diagnostics.

### Paying a bill

`tojfl pay` automates the portal's public one-time-payment lookup: it validates
your customer/account pair and reads back the amount due, then points you at the
**hosted payment page** where card entry happens.

```bash
tojfl pay quote -c 0000000 -a 000000        # what do I owe?
tojfl pay open  -c 0000000 -a 000000 --open # open the secure payment page
```

This tool **never handles card data.** It stops at the hosted processor page,
by design — see below.

## Privacy & security

This is a public repository, and it is built to keep your data out of it:

- **No credentials in code or config.** They're resolved at runtime from, in
  order: `--username`, `TOJFL_USERNAME`/`TOJFL_PASSWORD` env vars, the OS
  keychain, then a local (git-ignored) config file. The recommended path,
  `tojfl auth login --save`, uses the keychain.
- **No account data committed.** Session cookies live under your OS state dir
  (`~/Library/Application Support/us.piekstra.tojfl/` on macOS) with `0600`
  permissions, never in the repo. The example config uses placeholder digits.
- **No card handling.** The payment commands hand off to the portal's hosted
  page; this tool does not see, store, or transmit card details.
- `.gitignore` additionally blocks `tojfl.toml`, `session.json`, `*.cookies`,
  `.env`, and saved HTML, as defense in depth.

## Scope & honesty about coverage

Because this scrapes a login-gated portal, the flows are validated to different
degrees:

- **Fully exercised against the live site:** the public paths — login form
  contract, one-time-payment lookup (`OnlinePayment.aspx`), and the request /
  `__VIEWSTATE` postback / response-parse pipeline they share.
- **Built to the eCARE/DNN page structure, validated by unit tests on
  representative markup:** the authenticated scrapers (billing, usage,
  transactions, profile, account summary). The column heuristics are
  intentionally forgiving and preserve any unrecognized columns in an `extra`
  map, so no data is dropped even if a deployment's table layout differs. If a
  page's markup has drifted, the parser degrades gracefully rather than
  crashing; open an issue with the (redacted) table and it's an easy fix.

## Development

```bash
cargo build --workspace
cargo test  --workspace          # unit tests on parsing/auth/DNN helpers
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Architecture and internals: [`CLAUDE.md`](CLAUDE.md) and
[`docs/portal.md`](docs/portal.md).

## License

MIT — see [LICENSE](LICENSE).
