# --- Device auto-detection -------------------------------------------------
# Set DEVICE explicitly (e.g. `make flash DEVICE=xiao-office`) or let Make
# auto-detect from the connected board's MAC address via devices/devices.conf.
#
# Auto-detection runs `espflash board-info` only for targets that need it.
# When no board is connected or devices.conf is absent, it silently falls
# back to environment variables (e.g. from .envrc).
DEVICE_TARGETS := all lint build flash monitor

ifneq ($(filter $(DEVICE_TARGETS),$(MAKECMDGOALS)),)
  ifndef DEVICE
    ifneq ($(wildcard devices/devices.conf),)
      DEVICE := $(shell mac=$$(espflash board-info 2>&1 | sed -n 's/.*MAC address: *//p'); \
        [ -n "$$mac" ] && awk -v m="$$mac" '!/^[[:space:]]*\#/ && NF>=2 && $$1==m {print $$2; exit}' devices/devices.conf)
    endif
  endif
endif

ifneq ($(DEVICE),)
  -include devices/$(DEVICE).mk
  $(info Using device: $(DEVICE))
endif

# Target chip (default: esp32c6).
# Override: make build CHIP=esp32  or set CHIP in a device .mk file.
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
	@echo ''
	@echo 'Device config: auto-detected from connected board MAC (see devices/),'
	@echo '               or override: make <target> DEVICE=myboard-kitchen'

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

.PHONY: register
register: ## Register the connected board as DEVICE (e.g. make register DEVICE=myboard-kitchen)
ifndef DEVICE
	$(error DEVICE is required. Usage: make register DEVICE=myboard-kitchen)
endif
	@mac=$$(espflash board-info 2>&1 | sed -n 's/.*MAC address: *//p'); \
	if [ -z "$$mac" ]; then \
		echo "Error: could not detect MAC address. Is a board connected?" >&2; \
		exit 1; \
	fi; \
	if [ ! -f devices/devices.conf ]; then \
		printf '# MAC-to-device-name mapping for auto-detection.\n# Format: MAC_ADDRESS  device-name\n\n' > devices/devices.conf; \
	fi; \
	if grep -q "$$mac" devices/devices.conf 2>/dev/null; then \
		echo "MAC $$mac is already registered in devices/devices.conf"; \
	else \
		printf '%s  %s\n' "$$mac" "$(DEVICE)" >> devices/devices.conf; \
		echo "Registered $$mac as $(DEVICE) in devices/devices.conf"; \
	fi; \
	if [ ! -f devices/$(DEVICE).mk ]; then \
		cp devices/device.mk.example devices/$(DEVICE).mk; \
		echo "Created devices/$(DEVICE).mk from template — edit it to set IP, hostname, etc."; \
	else \
		echo "devices/$(DEVICE).mk already exists"; \
	fi

.PHONY: clean
clean: ## Remove build artefacts
	$(CARGO) clean
