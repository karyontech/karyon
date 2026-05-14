SMOL_ALL  = smol
TOKIO_ALL = tokio

.PHONY: check fmt clippy unit-test integration-test test

check:
	cargo check

fmt:
	cargo fmt --check

clippy:
	cargo clippy -- -D warnings

unit-test:
	cargo test --lib --no-default-features --features $(SMOL_ALL)
	cargo test --lib --no-default-features --features $(TOKIO_ALL)
	cargo test --doc --no-default-features --features $(SMOL_ALL)
	cargo test --doc --no-default-features --features $(TOKIO_ALL)

integration-test:
	cargo test --no-default-features --features $(SMOL_ALL) --tests
	cargo test --no-default-features --features $(TOKIO_ALL) --tests

test: unit-test integration-test
