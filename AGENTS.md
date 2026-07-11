# AGENTS.md

Agent entrypoint for the **tojfl** repository. This file is a thin index — the
substance lives in the documents it points to. Keep it and `CLAUDE.md` short.

## Start here

- **Development workflow & repo-local facts:** [`docs/development.md`](docs/development.md)
- **Architecture & internals:** [`CLAUDE.md`](CLAUDE.md) (peer of this file)
- **User-facing usage:** [`README.md`](README.md)
- **Portal field reference:** [`docs/portal.md`](docs/portal.md)

## Build, test, lint

Everything runs through the Makefile:

```bash
make check   # fmt-check + clippy (-D warnings) + test + build — run before pushing
```

## Conventions

- Rust, `clap` derive; `--json` on every command; human tables otherwise.
- **Credential-safe:** no secrets or PII in the repo — credentials resolve from
  flags → env → OS keychain → gitignored config; session state lives under the
  OS state dir at `0600`.
- Put repo-specific guidance in `docs/development.md`, not here.

## The CLI family & cli-common

This CLI conforms to **piekstra-cli/1** — the shared surface spec in
[piekstra/cli-common](https://github.com/piekstra/cli-common) (`DESIGN.md`):
standard `auth` / `config` / `self-update` / `completions` / `info` commands,
global `--json`, canonical DTOs (`auth-status/v1`, `self-update/v1`,
`cli-info/v1`), and frozen exit codes 0–6.

- **Don't fork shared behavior.** Error/exit-code handling, output rendering,
  keychain secrets, config storage, and self-update come from the `pk-cli-*`
  crates (tag-pinned git deps on cli-common). If you need a change there — or
  you're writing anything reusable across the family CLIs (fpl, xfin, lrfl,
  tojfl, …) — add it to cli-common, cut a tag, and bump the pin here. Never
  copy shared code into this repo.
- **Surface changes are spec changes.** A new standard command, flag, DTO
  field, or exit code belongs in cli-common's `DESIGN.md` first; update
  `conformance.md` alongside.
- **macOS dev signing.** Every plain `cargo build` gets a fresh ad-hoc code
  signature, so keychain "Always Allow" grants don't stick and every rebuild
  re-prompts. One-time: run cli-common's `scripts/setup-dev-signing.sh`. Then
  build with `make dev` (build + re-sign with the stable `pk-cli-codesign`
  identity) whenever you'll exercise keychain-touching commands.
