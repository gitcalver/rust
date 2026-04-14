build:
	cargo build --release

test:
	cargo test

lint:
	cargo clippy

acceptance: build
	GITCALVER=$(CURDIR)/target/release/gitcalver ../sh/test/test.sh

coverage:
	cargo +nightly llvm-cov test --workspace

.PHONY: build test lint acceptance coverage
