# Anvil development tasks. No CI/CD is configured by design.

CARGO ?= cargo

.PHONY: all build release test bench fuzz run clean examples fmt

all: build

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test
	$(CARGO) test --release

bench:
	$(CARGO) run --release -p anvil-bench

fuzz:
	$(CARGO) run --release -p anvil-fuzz -- 2000

run:
	$(CARGO) run --release

examples:
	$(CARGO) run --release -- generate-examples

clean:
	$(CARGO) clean

fmt:
	rustfmt --edition 2021 $$(find crates src tests bench fuzz -name '*.rs')
