# Contributing

Thanks for your interest. This is a personal project but PRs and issues are
welcome.

## Ground rules

- **Never commit secrets or personal data.** Credentials come from flags, env,
  the OS keychain, or a gitignored config — never the repo. No real account
  numbers, addresses, or portal responses in code, tests, or fixtures.
- Keep the working tree clean: `make check` (fmt + clippy `-D warnings` + tests +
  build) must pass before you push.

## Workflow

1. Branch off `main`.
2. Make the change with a test where it makes sense (parsers/auth/DNN helpers
   have inline `#[cfg(test)]` tests using HTML fixtures — no network).
3. Run `make check`.
4. Open a PR describing the change and how you verified it.

## Style

- Match the surrounding code; `cargo fmt` decides formatting.
- Comments explain *why* / non-obvious constraints, not *what*.
- See [`docs/development.md`](docs/development.md) for layout and invariants.

## The CLI family & cli-common

This repo is part of a family of CLIs (fpl, xfin, lrfl, tojfl, …) that share a
surface spec and library crates: [piekstra/cli-common](https://github.com/piekstra/cli-common)
(**piekstra-cli/1**). Before adding anything reusable — output rendering,
secret handling, config storage, self-update, DTO shapes — check whether it
belongs in cli-common's `pk-cli-*` crates instead. Contributions of shared,
reusable pieces to cli-common are encouraged and preferred over per-repo
copies; consume them here as tag-pinned git dependencies.

Surface changes (new standard commands/flags, DTO fields, exit codes) start as
a spec change in cli-common's `DESIGN.md`.

On macOS, run cli-common's `scripts/setup-dev-signing.sh` once and build with
`make dev` so keychain "Always Allow" grants survive rebuilds.
