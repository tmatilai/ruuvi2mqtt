# Target chip (default: esp32c6).
# Override: make build CHIP=esp32
# Supported: esp32c6, esp32c3, esp32, esp32s3
CHIP ?= esp32c6

# --- Derive target triple and toolchain from CHIP ---
ifeq ($(CHIP),esp32c6)
  TARGET := riscv32imac-esp-espidf
  TOOLCHAIN :=
else ifeq ($(CHIP),esp32c3)
  TARGET := riscv32imc-esp-espidf
  TOOLCHAIN :=
else ifeq ($(CHIP),esp32)
  TARGET := xtensa-esp32-espidf
  TOOLCHAIN := +esp
else ifeq ($(CHIP),esp32s3)
  TARGET := xtensa-esp32s3-espidf
  TOOLCHAIN := +esp
else
  $(error Unsupported CHIP=$(CHIP). Use: esp32c6, esp32c3, esp32, esp32s3)
endif

export MCU := $(CHIP)
CARGO := cargo $(TOOLCHAIN)

# Pure-Rust source files with #[cfg(test)] unit tests.
# These have no ESP-IDF dependencies and can be tested on the host.
TEST_SRCS := src/mac.rs

.DEFAULT_GOAL := help

.PHONY: help
help: ## Show this help
	@grep -hE '^[a-zA-Z-][a-zA-Z0-9_\.-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'
	@echo ''
	@echo 'Override target chip: make <target> CHIP=esp32  (default: esp32c6)'
	@echo 'Xtensa chips (esp32, esp32s3) require the "esp" toolchain (espup install).'

.PHONY: all
all: lint test build ## Lint, test, and build

.PHONY: lint
lint: ## Format and lint
	$(CARGO) fmt
	$(CARGO) clippy --target $(TARGET) -- -D clippy::all -W clippy::pedantic

.PHONY: test
test: ## Run host-side unit tests (pure-Rust modules only)
	@mkdir -p target
	@for f in $(TEST_SRCS); do \
	  echo "Testing $$f..."; \
	  rustup run stable rustc --test --edition 2021 $$f -o target/test_$$(basename $$f .rs) && \
	  target/test_$$(basename $$f .rs) || exit 1; \
	done

.PHONY: build
build: ## Build the firmware (release)
	$(CARGO) build --release --target $(TARGET)

.PHONY: flash
flash: ## Build and flash the firmware, then open the serial monitor
	$(CARGO) run --release --target $(TARGET)

.PHONY: monitor
monitor: ## Open the serial monitor (no build/flash)
	espflash monitor

.PHONY: clean
clean: ## Remove build artefacts
	$(CARGO) clean
