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

Conforms to the [piekstra-cli spec v1](https://github.com/piekstra/cli-common)
including the **`utility/v1` domain profile**: `summary`/`balance --json` emit
the canonical `utility-summary/v1` DTO, and `bills`/`usage`/`transactions`
lists emit schema-tagged `Paged` envelopes (records under `items`, `Money` as
string-decimal dollars, ISO dates) — so drivers like
[utiman](https://github.com/piekstra/utiman) need no per-provider field
configuration. The full provider-shaped payload stays available via
`snapshot --json`.

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
git clone https://github.com/piekstra/town-of-jupiter-fl-cli
cd town-of-jupiter-fl-cli
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

Once credentials are saved, **sessions refresh themselves**: the portal expires
its login cookie after a short idle period, so when a command finds the cached
session stale, tojfl silently re-authenticates from the stored credentials and
continues. Long-lived callers (dashboards, cron) keep working without a manual
`auth login`. This only ever re-establishes a session you already had — after
`tojfl auth logout` you stay logged out. Set `auto_login = false` in the config
to disable it and require an explicit login after each expiry.

Requests also **retry transient failures** — connection resets, timeouts, and
429/502/503/504 responses — with a short exponential backoff (the portal is an
old, occasionally-flaky server), so a blip doesn't fail a scripted run.

For non-interactive login (no TTY to prompt at), `tojfl auth login` reads the
password from the keychain if present; pipe one explicitly with
`--password-stdin` (e.g. `op read … | tojfl auth login --password-stdin --save`).

## Commands

| Command | What it does |
| --- | --- |
| `tojfl auth login [--save] [--password-stdin]` | Authenticate and cache a session |
| `tojfl auth logout [--forget]` | Clear the session (and optionally the keychain password) |
| `tojfl auth status` | Report whether a valid session exists |
| `tojfl summary` | At-a-glance overview: balance, due, last read/bill/payment, paperless + autopay |
| `tojfl snapshot [--all-accounts]` | One-call dashboard payload: account name + service address, balance (plus pending payments / effective balance), due/past-due, last payment, usage stats, ledger totals (best with `--json`; `--all-accounts` returns one per linked account) |
| `tojfl account show` | Account summary: account #, **name, service address**, balance, due date (for the active account) |
| `tojfl account list` | All accounts linked to your login (#, name, service address, balances) |
| `tojfl balance` | Just the current balance due |
| `tojfl bills list [--limit N] [--since/--until DATE]` | Billing history: current charges, bill total, balance forward per statement; a `PDF` column shows which are downloadable |
| `tojfl bills latest` | Most recent statement |
| `tojfl bills get <N> [-o FILE]` | Download a statement PDF (1 = most recent; `-o -` writes to stdout) |
| `tojfl usage list [--limit N] [--since/--until DATE]` | Metered water usage per period |
| `tojfl usage compare [--against street\|region\|city]` | Consumption change period-over-period, or vs. a street/region/city average |
| `tojfl usage stats [--since/--until DATE]` | Summary over usage history: periods, total, average, min/max period |
| `tojfl meters [--limit N] [--since/--until DATE]` | Meter reading history: date, meter #, previous/current read, days, type, usage |
| `tojfl transactions list [--limit N] [--since/--until DATE]` | Ledger: charges, payments, adjustments |
| `tojfl transactions summary [--since/--until DATE]` | Totals: charges, payments/credits, and net over the ledger |
| `tojfl profile show` | Account holder profile |
| `tojfl ebill status` | Paperless / autopay enrollment status |
| `tojfl pay quote [-c CUST -a ACCT]` | Amount due for an account; omit `-c/-a` to use your logged-in account |
| `tojfl pay open [-c CUST -a ACCT] [--open]` | Print/open the hosted payment page URL (omit `-c/-a` for your logged-in account) |
| `tojfl service` | Service snapshot: last read date, last bill, last payment |
| `tojfl contact` | Utility contact & service info (offline) |
| `tojfl open [ACCT]` | Open the utility portal in your browser (log in there) |
| `tojfl config path\|init\|show\|set\|unset\|set-password\|clear-password` | Manage local config & credentials (`config set account 000000`) |
| `tojfl self-update [--check] [-y]` | Update the binary in place from the latest GitHub release |

Add `--json` to any command for machine-readable output, or `--csv` for a
spreadsheet-friendly sheet (row commands and single-record views alike); `-v`
adds diagnostics. The default mode can also be set with `output = "json"` /
`"csv"` in the config file.

The history commands (`bills list`, `usage list`, `meters`, `transactions list`)
accept `--since` / `--until` to bound the results by date — both inclusive, both
optional, and applied before `--limit`. Dates accept `YYYY-MM-DD`, `MM/DD/YYYY`,
or `Mon DD, YYYY`:

```bash
tojfl transactions list --since 2026-06-01           # this quarter's ledger
tojfl bills list --since 2026-01-01 --until 2026-06-30 --csv > h1-bills.csv
```

### Multiple accounts

If several utility accounts are linked to your login, `tojfl account list` shows
them all. Target a specific one with the global `--account <ACCOUNT#>` flag —
it activates that account for the session before the command runs, so
`account show`, `bills`, `usage`, `meters`, `transactions`, and `ebill status`
all report that account:

```bash
tojfl account list
tojfl --account 000000 bills list      # statements for account 000000
tojfl --account 000000 usage list
```

Run `tojfl config set account <ACCOUNT#>` to avoid repeating `--account`.

### Paying a bill

`tojfl pay` automates the portal's public one-time-payment lookup: it validates
your customer/account pair and reads back the amount due, then points you at the
**hosted payment page** where card entry happens.

```bash
tojfl pay quote                             # your logged-in account (name/address shown)
tojfl pay open --open                        # open the secure payment page for it
tojfl pay quote -c 0000000 -a 000000        # or an explicit account (no login needed)
```

When logged in, omit `-c/-a` and the customer/account come from your **active**
account (honoring `--account`); the quote also shows the account **name**
(`account_name` in `--json`) and, in the default table view, the **service
address**, so you can confirm you're paying the right premise.

This tool **never handles card data.** It stops at the hosted processor page,
by design — see below.

## Exit codes

Commands follow the shared CLI-family exit-code contract, so scripts and
dashboards can branch on failure kind:

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | Unexpected/other error (incl. keychain access) |
| `2` | Usage error (bad flags/arguments/input) |
| `3` | Authentication required or session invalid/expired |
| `4` | Not found (unknown account, no statement at that position) |
| `5` | Upstream/portal error (network, TLS, unparseable page) |
| `6` | Confirmation required |

With `--json`, failures also print a machine-readable error object to stdout.

## Updating

```bash
tojfl self-update --check   # is a newer release available?
tojfl self-update           # download it and replace the binary in place
```

`self-update` pulls the build for your platform from this repo's GitHub
Releases. New releases are produced by pushing a version tag (`git tag v0.2.0 &&
git push origin v0.2.0`), which triggers the release workflow. If you installed
via a package manager, use that manager's upgrade instead.

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

- **Validated end-to-end against a real logged-in account:** login (the DNN
  forms-auth postback), **account summary** (balance, due date, account
  number), **billing history** (incl. downloading a statement PDF via the grid's
  eBill link), **transaction history**, **usage** (service-selection form →
  consumption grid; plus `--against` street/region/city comparison), **meter
  reads** (`MeterReadingHistory.aspx` service-selection form → reads grid: 4
  reads returned with previous/current read, days, type, and consumption),
  **multi-account** (`account list` + `--account` switching), and **`ebill
  status`**
  (paperless + autopay enrollment, incl. the autopay plan/draw). These read the
  eCARE ASP.NET GridViews directly; unrecognized columns are preserved in an
  `extra` map so nothing is dropped.
- **Fully exercised against the live site (public paths):** the login-form
  contract and the one-time-payment lookup (`OnlinePayment.aspx`).
- **`profile`** reads the DNN ManageUsers page (`ChangeProfile.aspx`) and returns
  the account holder's name, username, and email. The parser deliberately
  extracts only those known profile properties — it never surfaces the password
  or security-question fields on that page.

If a page's markup drifts, the parser degrades gracefully rather than crashing.

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
