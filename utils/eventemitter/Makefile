.PHONY: check fmt clippy unit-test integration-test test

check:
	cargo check

fmt:
	cargo fmt --check

clippy:
	cargo clippy -- -D warnings

unit-test:
	cargo test --lib
	cargo test --doc

integration-test:
	cargo test --tests

test: unit-test integration-test
