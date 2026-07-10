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
