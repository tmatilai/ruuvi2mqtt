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

.PHONY: lint
lint: ## Check the version, format and lint
	script/check-version
	cargo fmt --all
	cargo clippy --tests --all-targets --all-features -- -D clippy::all -W clippy::pedantic

.PHONY: test
test: lint ## Lint and test
	cargo test

.PHONY: local
local: test ## Test and build locally
	cargo build

.PHONY: cross
cross: $(ARCHS) ## Build all non-local architectures

# `cross` is installed from git: the last crates.io release is from 2023.
.PHONY: setup
setup: ## Install the tools for the Linux app and the ESP32 firmware
	cargo install --git https://github.com/cross-rs/cross cross
	$(MAKE) -C ruuvi2mqtt-esp32 setup

.PHONY: $(ARCHS)
$(ARCHS): ## Build for the specified architecture
	cross build --target $(TARGET_ARCH_$@)

.PHONY: release
release: ## Prepare the release PR: make release VERSION=1.2.3
	script/release $(VERSION)
