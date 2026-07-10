# Development

Repo-local facts for working on `tojfl`. Architecture rationale lives in
[`CLAUDE.md`](../CLAUDE.md); this file is the how-to-develop source of truth.

## Prerequisites

- Rust 1.82+ (`rustup`).
- On Linux, OpenSSL dev headers (`pkg-config`, `libssl-dev`) — the HTTP client
  links **native-tls** (required; see below).

## Layout

```
crates/tojfl-sdk/    # the portal client: HTTP, DNN/WebForms, auth, scrapers, models
crates/tojfl-cli/    # clap CLI producing the `tojfl` binary; rendering + glue
docs/                # portal.md (field reference), development.md (this file)
```

## Common tasks

```bash
make build     # cargo build --workspace
make test      # cargo test --workspace
make lint      # cargo clippy --workspace --all-targets -- -D warnings
make fmt       # cargo fmt --all
make check     # fmt-check + lint + test + build (run before pushing)
make install   # cargo install --path crates/tojfl-cli
```

## Invariants (don't regress these)

- **native-tls, not rustls.** The portal offers only CBC-mode TLS 1.2 ciphers;
  rustls can't negotiate them. Pinned in the root `Cargo.toml`.
- **Manual redirect following** in `client.rs` so `Set-Cookie` is observed during
  the DNN login postback (that's how the session cookie is captured).
- **Auth gating** (`Portal::has_session`): authenticated commands must reject
  when no session is loaded rather than scrape a public page.

## Testing

Unit tests are inline (`#[cfg(test)] mod tests`) and run offline against HTML
fixtures — no network. Public flows (`contact`, `pay quote`) are verified live;
authenticated scrapers are validated against representative markup and should be
re-checked on a real login.

## Releasing

Version lives in `Cargo.toml` (mirrored in `version.txt`). Tag to release:

```bash
git tag v0.2.0 && git push origin v0.2.0
```

`.github/workflows/release.yml` builds macOS (arm64/x86_64) + Linux binaries and
attaches them to the GitHub Release; `tojfl self-update` pulls from there.
