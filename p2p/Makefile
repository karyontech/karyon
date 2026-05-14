ALL_FEATURES = quic
SMOL_ALL  = smol,$(ALL_FEATURES)
TOKIO_ALL = tokio,$(ALL_FEATURES)

.PHONY: check fmt clippy unit-test integration-test test \
	test-tcp test-tls test-quic test-multi-transport \
	test-smol test-tokio

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

test-tcp:
	cargo test --no-default-features --features $(SMOL_ALL) --test tcp
	cargo test --no-default-features --features $(TOKIO_ALL) --test tcp

test-tls:
	cargo test --no-default-features --features $(SMOL_ALL) --test tls
	cargo test --no-default-features --features $(TOKIO_ALL) --test tls

test-quic:
	cargo test --no-default-features --features $(SMOL_ALL) --test quic
	cargo test --no-default-features --features $(TOKIO_ALL) --test quic

test-multi-transport:
	cargo test --no-default-features --features $(SMOL_ALL) --test multi_transport
	cargo test --no-default-features --features $(TOKIO_ALL) --test multi_transport

test-smol:
	cargo test --no-default-features --features $(SMOL_ALL) --tests

test-tokio:
	cargo test --no-default-features --features $(TOKIO_ALL) --tests

integration-test: test-smol test-tokio

test: unit-test integration-test
