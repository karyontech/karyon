ALL_FEATURES = quic,aws-lc-rs
SMOL_ALL  = smol,$(ALL_FEATURES)
TOKIO_ALL = tokio,$(ALL_FEATURES)

.PHONY: check check-examples fmt clippy unit-test integration-test test \
	test-discovery test-smol test-tokio

check:
	cargo check

check-examples:
	cargo check --examples

fmt:
	cargo fmt --check

clippy:
	cargo clippy -- -D warnings

unit-test:
	cargo test --lib --no-default-features --features $(SMOL_ALL)
	cargo test --lib --no-default-features --features $(TOKIO_ALL)
	cargo test --doc --no-default-features --features $(SMOL_ALL)
	cargo test --doc --no-default-features --features $(TOKIO_ALL)

test-discovery:
	cargo test --no-default-features --features $(SMOL_ALL) --test discovery
	cargo test --no-default-features --features $(TOKIO_ALL) --test discovery

test-smol:
	cargo test --no-default-features --features $(SMOL_ALL) --tests

test-tokio:
	cargo test --no-default-features --features $(TOKIO_ALL) --tests

integration-test: test-smol test-tokio

test: unit-test integration-test
