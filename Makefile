CRATES = core net jsonrpc p2p swarm utils/eventemitter

.PHONY: all check fmt clippy unit-test integration-test test \
	test-core test-net test-jsonrpc test-p2p test-swarm test-eventemitter

all: check fmt clippy

check:
	cargo check --all

fmt:
	cargo fmt --all --check

clippy:
	cargo clippy --workspace -- -D warnings

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

test-eventemitter:
	$(MAKE) -C utils/eventemitter test

test: unit-test integration-test
