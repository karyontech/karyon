ALL_FEATURES = udp,tcp,tls,quic
SMOL_ALL  = smol,$(ALL_FEATURES)
TOKIO_ALL = tokio,$(ALL_FEATURES)
# Single transports. Guard the cfg-gated paths.
SMOL_UDP  = smol,udp
SMOL_TCP  = smol,tcp
SMOL_QUIC = smol,quic

.PHONY: check fmt clippy unit-test integration-test test

check:
	cargo check
	cargo check --no-default-features --features $(TOKIO_ALL)
	cargo check --no-default-features --features $(SMOL_UDP)
	cargo check --no-default-features --features $(SMOL_TCP)
	cargo check --no-default-features --features $(SMOL_QUIC)

fmt:
	cargo fmt --check

clippy:
	cargo clippy -- -D warnings
	cargo clippy --no-default-features --features $(TOKIO_ALL) -- -D warnings
	cargo clippy --no-default-features --features $(SMOL_UDP) -- -D warnings
	cargo clippy --no-default-features --features $(SMOL_TCP) -- -D warnings
	cargo clippy --no-default-features --features $(SMOL_QUIC) -- -D warnings

unit-test:
	cargo test --lib --no-default-features --features $(SMOL_ALL)
	cargo test --lib --no-default-features --features $(TOKIO_ALL)
	cargo test --doc --no-default-features --features $(SMOL_ALL)
	cargo test --doc --no-default-features --features $(TOKIO_ALL)

integration-test:
	cargo test --no-default-features --features $(SMOL_ALL) --tests
	cargo test --no-default-features --features $(TOKIO_ALL) --tests

test: unit-test integration-test
