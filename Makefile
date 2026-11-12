CONFORMANCE_DIR ?= ../sh
CONFORMANCE_SHA := 857287da052d1437703ead1f7d2adc76a95451ba

build:
	cargo build --release

test:
	cargo test

lint: actions-lock-verify-local
	cargo fmt --check
	cargo clippy --all-targets

fmt:
	cargo fmt

acceptance: build
	@test "$$(git -C "$(CONFORMANCE_DIR)" rev-parse "$(CONFORMANCE_SHA)^{commit}")" = "$(CONFORMANCE_SHA)"
	@set -e; \
	tmp="$$(mktemp)"; \
	trap 'rm -f "$$tmp"' EXIT HUP INT TERM; \
	git -C "$(CONFORMANCE_DIR)" show "$(CONFORMANCE_SHA):test/test.sh" >"$$tmp"; \
	GITCALVER="$(CURDIR)/target/release/gitcalver" sh "$$tmp"

coverage:
	cargo +nightly llvm-cov test \
		--fail-under-functions 100 \
		--fail-under-lines 100

publish publish-dry-run:
	@set -e; \
	src=$$(pwd); \
	tmp=$$(mktemp -d); \
	trap 'rm -rf "$$tmp"' EXIT INT TERM; \
	git archive HEAD | tar -x -C "$$tmp"; \
	ver=$$(cargo run -q -- prepare-publish --prefix 0. --manifest "$$tmp/Cargo.toml" --source-dir "$$src"); \
	$(if $(findstring dry,$@),echo "Would publish version $$ver";) \
	cd "$$tmp"; \
	CARGO_TARGET_DIR="$$src/target" cargo check --quiet; \
	CARGO_TARGET_DIR="$$src/target" cargo publish $(if $(findstring dry,$@),--dry-run,)

.PHONY: build test lint fmt acceptance coverage publish publish-dry-run

.PHONY: actions-lock-install actions-lock actions-lock-verify-local actions-lock-verify

actions-lock-install:
	gh extension install github/gh-actions-lock --pin v0.4.1 --force

actions-lock:
	gh actions-lock --no-interactive

actions-lock-verify-local:
	gh actions-lock --verify-local

actions-lock-verify:
	gh actions-lock --verify
