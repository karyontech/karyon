ALL_FEATURES = tcp,tls,ws,quic,http,http3,unix
SMOL_ALL  = smol,$(ALL_FEATURES)
TOKIO_ALL = tokio,$(ALL_FEATURES)

.PHONY: check fmt clippy unit-test integration-test test \
	test-tcp test-tls test-ws test-wss test-unix test-quic test-http test-http3 \
	test-macros test-custom-codec test-ws-custom-codec \
	test-smol test-tokio

check:
	cargo check
	cargo check --no-default-features --features $(SMOL_ALL)
	cargo check --no-default-features --features $(TOKIO_ALL)

fmt:
	cargo fmt --check

clippy:
	cargo clippy -- -D warnings
	cargo clippy --no-default-features --features $(SMOL_ALL) -- -D warnings
	cargo clippy --no-default-features --features $(TOKIO_ALL) -- -D warnings

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

test-ws:
	cargo test --no-default-features --features $(SMOL_ALL) --test ws
	cargo test --no-default-features --features $(TOKIO_ALL) --test ws

test-wss:
	cargo test --no-default-features --features $(SMOL_ALL) --test wss
	cargo test --no-default-features --features $(TOKIO_ALL) --test wss

test-unix:
	cargo test --no-default-features --features $(SMOL_ALL) --test unix
	cargo test --no-default-features --features $(TOKIO_ALL) --test unix

test-quic:
	cargo test --no-default-features --features $(SMOL_ALL) --test quic
	cargo test --no-default-features --features $(TOKIO_ALL) --test quic

test-http:
	cargo test --no-default-features --features $(SMOL_ALL) --test http
	cargo test --no-default-features --features $(TOKIO_ALL) --test http

test-http3:
	cargo test --no-default-features --features $(SMOL_ALL) --test http3
	cargo test --no-default-features --features $(TOKIO_ALL) --test http3

test-macros:
	cargo test --no-default-features --features $(SMOL_ALL) --test macros
	cargo test --no-default-features --features $(TOKIO_ALL) --test macros

test-custom-codec:
	cargo test --no-default-features --features $(SMOL_ALL) --test custom_codec
	cargo test --no-default-features --features $(TOKIO_ALL) --test custom_codec

test-ws-custom-codec:
	cargo test --no-default-features --features $(SMOL_ALL) --test ws_custom_codec
	cargo test --no-default-features --features $(TOKIO_ALL) --test ws_custom_codec

test-smol:
	cargo test --no-default-features --features $(SMOL_ALL) --tests

test-tokio:
	cargo test --no-default-features --features $(TOKIO_ALL) --tests

integration-test: test-smol test-tokio

test: unit-test integration-test
