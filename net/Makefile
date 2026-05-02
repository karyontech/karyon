ALL_FEATURES = tcp,tls,ws,quic,udp,unix,proxy
SMOL_ALL  = smol,$(ALL_FEATURES)
TOKIO_ALL = tokio,$(ALL_FEATURES)

.PHONY: check fmt clippy unit-test integration-test test \
	test-codec test-builder test-proxy test-smol test-tokio

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

test-codec:
	cargo test --no-default-features --features $(SMOL_ALL) --test codec_test
	cargo test --no-default-features --features $(TOKIO_ALL) --test codec_test

test-builder:
	cargo test --no-default-features --features $(SMOL_ALL) --test builder_test
	cargo test --no-default-features --features $(TOKIO_ALL) --test builder_test

test-proxy:
	cargo test --no-default-features --features $(SMOL_ALL) --test proxy_test
	cargo test --no-default-features --features $(TOKIO_ALL) --test proxy_test

test-smol:
	cargo test --no-default-features --features $(SMOL_ALL) --tests

test-tokio:
	cargo test --no-default-features --features $(TOKIO_ALL) --tests

integration-test: test-smol test-tokio

test: unit-test integration-test
