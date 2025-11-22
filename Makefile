BIN := ruuvi2mqtt
ARCHS := aarch64 armv7 x86_64

TARGET_ARCH_aarch64 = aarch64-unknown-linux-gnu
TARGET_ARCH_x86_64 = x86_64-unknown-linux-gnu
TARGET_ARCH_armv7 = armv7-unknown-linux-gnueabihf

.DEFAULT_GOAL := help

.PHONY: help
help: ## Display this help
	@grep -hE '^[a-zA-Z-][a-zA-Z0-9_\.-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

.PHONY: all
all: local $(ARCHS) ## Lint and build all architectures

.PHONY: local
local: test ## Test and build locally
	cargo build

.PHONY: test
test: lint ## Lint and test
	cargo test

.PHONY: lint
lint: ## Format and lint
	cargo fmt --all
	cargo clippy --tests --all-targets --all-features -- -D clippy::all -W clippy::pedantic

.PHONY: cross
cross: $(ARCHS) ## Build all non-local architectures

.PHONY: $(ARCHS)
$(ARCHS): ## Build for the specified architecture
	cross build --target $(TARGET_ARCH_$@)
