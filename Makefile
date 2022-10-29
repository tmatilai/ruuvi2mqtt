BIN := ruuvi2mqtt
ARCHS := aarch64 armv7 x86_64

TARGET_ARCH_aarch64 = aarch64-unknown-linux-gnu
TARGET_ARCH_x86_64 = x86_64-unknown-linux-gnu
TARGET_ARCH_armv7 = armv7-unknown-linux-gnueabihf

CROSS_IMAGE := $(BIN)-dev

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
	cargo clippy --all --all-features

.PHONY: cross
cross: $(ARCHS) ## Build all non-local architectures

.PHONY: $(ARCHS)
$(ARCHS): ## Build for the specified architecture
	docker run --rm -ti -v $$PWD:/usr/src/app \
	    -e CARGO_HOME=/usr/src/app/target/.cargo \
	    $(CROSS_IMAGE) \
	    cargo build --target $(TARGET_ARCH_$@)

.PHONY: cross-image
cross-image: ## Build Docker image for cross compiling
	docker build -t $(CROSS_IMAGE) dev
