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
