# tojfl — build contract. `make check` is the pre-push gate.

CARGO ?= cargo
CLI_PATH := crates/tojfl-cli

.PHONY: all build test lint fmt fmt-check check install clean

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

install:
	$(CARGO) install --path $(CLI_PATH)

clean:
	$(CARGO) clean

# Debug build re-signed with the stable pk-cli-codesign identity so macOS
# keychain "Always Allow" grants survive rebuilds (see cli-common/scripts).
dev:
	cargo build
	@if [ -x "$$HOME/Dev/cli-common/scripts/dev-sign.sh" ]; then \
		"$$HOME/Dev/cli-common/scripts/dev-sign.sh" target/debug/tojfl; \
	else echo "cli-common/scripts/dev-sign.sh not found — binary left ad-hoc signed"; fi
