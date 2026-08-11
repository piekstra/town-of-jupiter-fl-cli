# tojfl — build contract. `make check` is the pre-push gate.

CARGO ?= cargo
CLI_PATH := crates/tojfl-cli

.PHONY: all build test lint fmt fmt-check check install clean dev

all: check

build:
	$(CARGO) build --workspace

test:
	$(CARGO) test --workspace

lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

check: fmt-check lint test build

# `cargo install` ad-hoc signs, which gives the binary a *new* code identity
# every time. macOS scopes keychain "Always Allow" grants to that identity, so
# an unsigned reinstall silently revokes them and the next run prompts again.
# Re-signing with the stable shared identity keeps one grant valid across every
# future install.
install: SIGN_TARGET = $${CARGO_INSTALL_ROOT:-$$HOME/.cargo}/bin/tojfl
install:
	$(CARGO) install --path $(CLI_PATH)
	@$(SIGN)

clean:
	$(CARGO) clean

# Debug build re-signed with the same stable pk-cli-codesign identity, so the
# dev loop doesn't re-prompt either (see cli-common/scripts).
dev: SIGN_TARGET = target/debug/tojfl
dev:
	cargo build
	@$(SIGN)

# Shared re-signing step. No-ops with a note when the helper or identity is
# absent (CI, Linux, a fresh machine that hasn't run setup-dev-signing.sh).
define SIGN
if [ -x "$$HOME/Dev/cli-common/scripts/dev-sign.sh" ]; then \
	"$$HOME/Dev/cli-common/scripts/dev-sign.sh" $(SIGN_TARGET); \
else echo "cli-common/scripts/dev-sign.sh not found — $(SIGN_TARGET) left ad-hoc signed"; fi
endef
