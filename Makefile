CONFORMANCE_DIR ?= ../sh
# Pinned to sh's last 0.2 commit. `make acceptance` is expected to FAIL at
# this pin: rust already implements the 0.3 counting rule, and the 0.2
# suite's merge-count expectations differ by design. Nothing in CI runs this
# target; re-pin to sh's 0.3 commit once sh/spec 0.3 land (see the 0.3
# rollout plan), after which it must pass again.
CONFORMANCE_SHA := a7f5c0600057d05028467cda8c65b36d5aa1eaf5

build:
	cargo build --release

test:
	cargo test

lint:
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
