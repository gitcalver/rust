build:
	cargo build --release

test:
	cargo test

lint:
	cargo clippy

acceptance: build
	GITCALVER=$(CURDIR)/target/release/gitcalver ../sh/test/test.sh

coverage:
	cargo +nightly llvm-cov test

publish publish-dry-run:
	@set -e; \
	[ -z "$$(git status --porcelain)" ] || { echo "working tree is dirty; commit or stash before publishing" >&2; exit 1; }; \
	ver="0.$$(cargo run -q --)"; \
	$(if $(findstring dry,$@),echo "Would publish version $$ver";) \
	src=$$(pwd); \
	tmp=$$(mktemp -d); \
	trap 'rm -rf "$$tmp"' EXIT INT TERM; \
	git archive HEAD | tar -x -C "$$tmp"; \
	awk -v ver="$$ver" '/^\[package\]$$/ {print; print "version = \"" ver "\""; next} 1' "$$src/Cargo.toml" > "$$tmp/Cargo.toml"; \
	cd "$$tmp"; \
	CARGO_TARGET_DIR="$$src/target" cargo check --quiet; \
	CARGO_TARGET_DIR="$$src/target" cargo publish $(if $(findstring dry,$@),--dry-run,)

.PHONY: build test lint acceptance coverage publish publish-dry-run
