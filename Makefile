CRATES = core net jsonrpc p2p swarm stun utils/eventemitter

# The default features with smol instead of tokio, so check and clippy
# cover both runtimes.
SMOL_FEATURES = karyon_core/smol,karyon_async_rustls/smol,\
	karyon_net/smol,karyon_net/all-protocols,\
	karyon_jsonrpc/smol,karyon_jsonrpc/tcp,karyon_jsonrpc/unix,karyon_jsonrpc/quic,\
	karyon_jsonrpc/http,karyon_jsonrpc/http3,karyon_jsonrpc/tls,karyon_jsonrpc/ws,\
	karyon_jsonrpc_macro/smol,karyon_p2p/smol,karyon_p2p/quic,karyon_swarm/smol,\
	karyon_stun/smol,karyon_stun/udp,karyon_stun/tcp,karyon_stun/tls,karyon_stun/quic,\
	karyon_eventemitter/derive

.PHONY: all check fmt clippy unit-test integration-test test \
	test-core test-net test-jsonrpc test-p2p test-swarm test-stun test-eventemitter

all: check fmt clippy

check:
	cargo check --all
	cargo check --all --no-default-features --features "$(SMOL_FEATURES)"

fmt:
	cargo fmt --all --check

clippy:
	cargo clippy --workspace -- -D warnings
	cargo clippy --workspace --no-default-features --features "$(SMOL_FEATURES)" -- -D warnings

unit-test:
	for c in $(CRATES); do $(MAKE) -C $$c unit-test || exit 1; done

integration-test:
	for c in $(CRATES); do $(MAKE) -C $$c integration-test || exit 1; done

test-core:
	$(MAKE) -C core test

test-net:
	$(MAKE) -C net test

test-jsonrpc:
	$(MAKE) -C jsonrpc test

test-p2p:
	$(MAKE) -C p2p test

test-swarm:
	$(MAKE) -C swarm test

test-stun:
	$(MAKE) -C stun test

test-eventemitter:
	$(MAKE) -C utils/eventemitter test

test: unit-test integration-test
