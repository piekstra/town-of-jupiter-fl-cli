# tojfl — internals

Command-line client for the Town of Jupiter, FL utility billing portal. This
file orients contributors (and coding agents) to the architecture and the
non-obvious constraints.

## Workspace layout

Two-layer design (SDK + CLI), mirroring the pattern used across these tools:

- **`crates/tojfl-sdk`** — the reusable client. Auth, HTTP, DNN/WebForms
  handling, HTML scraping, models, config/keychain, session persistence.
- **`crates/tojfl-cli`** — clap-derive CLI that produces the **`tojfl`** binary.
  Owns argument parsing and table/JSON rendering; contains no portal logic.

## What the portal is

- **Harris eCARE** utility billing on **DotNetNuke (DNN)** — ASP.NET WebForms.
- Every page is one big `<form>` that round-trips hidden fields
  (`__VIEWSTATE`, `__VIEWSTATEGENERATOR`, `__EVENTVALIDATION`, `__dnnVariable`).
  To submit anything you GET the page, scrape those fields, then POST them back
  plus your inputs and an `__EVENTTARGET`/`__EVENTARGUMENT` naming the "clicked"
  control. See `dnn.rs`.
- DNN control names carry volatile prefixes (`dnn$ctr1216$Login$Login_DNN$…`).
  We never hard-code them — we match on the stable suffix (`txtUsername`,
  `txtCust`, `GoButton`, `cmdLogin`). See `dnn::find_input_name_ending_with`.

## Endpoints (`pages.rs`)

Public: `Home`, `Login`, `ContactUs`, `Register`, `ForgotUsername`,
`SecurePasswordRecovery`, `OnlinePayment`.
Authenticated (redirect to `Login` until a session cookie exists):
`BillingHistory`, `UsageHistory`, `TransactionHistory`, `UserProfile`.

## Two hard constraints

1. **TLS must be native-tls, not rustls.** The server offers only CBC-mode
   TLS 1.2 ciphers (e.g. `ECDHE-RSA-AES256-SHA384`), no GCM, no TLS 1.3. rustls
   only does AEAD ciphers and cannot handshake. This is pinned in the root
   `Cargo.toml` with a comment; don't "modernize" it back to rustls.
2. **Manual redirect following.** `client.rs` sets `redirect::Policy::none()`
   and follows 3xx by hand so it can observe every `Set-Cookie` — that's how DNN
   delivers the forms-auth cookie during login, and we must capture it to
   persist the session. Letting reqwest auto-follow would hide those headers.

## Flows

- `auth.rs` — GET `Login.aspx`, fill username/password, post back with the login
  button as `__EVENTTARGET`. The button is a DNN LinkButton, so the target is the
  `$`-delimited UniqueID from its `__doPostBack` call (`find_postback_target`),
  NOT the element id. Success = have a DNN auth cookie and no longer see the
  login form. `verify()` re-checks by fetching a protected page.
- `scrape.rs` — `extract_tables()` reads every table (dropping DNN menu/script
  tables); parsers select the eCARE **GridView** by id (`is_data_grid`), then map
  columns by **header keyword matching**, stashing unrecognized columns in each
  model's `extra`. DNN renders menus as nested tables, so id-based grid selection
  is essential — "biggest table" grabs the nav menu.
- `usage.rs` — `UsageHistory.aspx` is form-first; submits the service dropdown
  via its ImageButton (posted as `name.x`/`name.y`), then scrapes the grid.
- `accounts.rs` — lists linked accounts and switches between them. Selecting an
  account posts back `Select$<rowIndex>` to the ListAccounts GridView, activating
  it for the session. `Portal::ready()` runs this before account-scoped reads
  when `--account`/`default_account` is set (`account list` itself is not scoped).
- `payment.rs` — public one-time-payment lookup. Validates digits, posts
  customer+account, reads the amount due and the hosted-page URL. Intentionally
  stops before card entry (that's on the processor's page).

## Credentials & state (never in the repo)

- `config.rs` resolves credentials: flags → env (`TOJFL_USERNAME`/
  `TOJFL_PASSWORD`) → OS keychain (`keyring`) → local `tojfl.toml`.
- `session.rs` persists cookies to the OS state dir at `0600`.
- Config/session paths come from `directories::ProjectDirs("us","piekstra",
  "tojfl")`.

## Testing

Unit tests are inline (`#[cfg(test)] mod tests`) and run on representative HTML
fixtures — no network. `Money`, DNN field extraction, each table parser, and the
payment helpers are covered. Public flows are additionally exercised against the
live site by hand (`tojfl contact`, `tojfl pay quote …`).

## Self-update & releases

`crates/tojfl-cli/src/selfupdate.rs` (`tojfl self-update`) uses the `self_update`
crate's GitHub backend to pull the platform binary from this repo's Releases and
swap it in place. It matches assets by Rust target triple, so the release
workflow (`.github/workflows/release.yml`, triggered by a `v*` tag) must name
assets `tojfl-<tag>-<target>.tar.gz`. `self_update` uses native-tls (same as the
portal client). `version.txt` mirrors the Cargo version for family-consistency.

## Commands

```bash
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```
