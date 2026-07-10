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
