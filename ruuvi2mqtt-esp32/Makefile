# --- Device configuration --------------------------------------------------
# Set DEVICE explicitly (e.g. `make flash DEVICE=kitchen`) or let Make
# auto-detect from the connected board's MAC address via devices/devices.conf.
# Set DEVICE to empty to skip auto-detection (e.g. `make flash DEVICE=`).
#
# Auto-detection runs `espflash board-info` only for targets that need it
# and only when devices/devices.conf exists.
DEVICE_TARGETS := all lint build flash monitor

ifneq ($(filter $(DEVICE_TARGETS),$(MAKECMDGOALS)),)
  # Auto-detect device when DEVICE is not set at all
  ifeq ($(origin DEVICE),undefined)
    ifneq ($(wildcard devices/devices.conf),)
      ifeq ($(shell command -v espflash 2>/dev/null),)
        $(error espflash not found — cannot auto-detect device. Run 'make setup' first, set DEVICE, or set DEVICE= to skip.)
      else
        $(info Auto-detecting device from connected board...)
        DEVICE := $(shell mac=$$(espflash board-info 2>&1 | sed -n 's/.*MAC address: *//p'); \
          [ -n "$$mac" ] && awk -v m="$$mac" '!/^[[:space:]]*\#/ && NF>=2 && $$1==m {print $$2; exit}' devices/devices.conf)
        ifeq ($(DEVICE),)
          $(error Device not recognised. Register it with 'make register DEVICE=<name>', set DEVICE, or set DEVICE= to skip.)
        endif
      endif
    endif
  endif

  # Load device config
  ifneq ($(DEVICE),)
    $(info Using device: $(DEVICE))
    ifeq ($(wildcard devices/$(DEVICE).mk),)
      $(error Device config 'devices/$(DEVICE).mk' not found. Register with 'make register DEVICE=$(DEVICE)' first.)
    endif
    include devices/$(DEVICE).mk
  endif
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
	@echo ''
	@echo 'Device config: auto-detected from connected board MAC (see devices/),'
	@echo '               or override: make <target> DEVICE=kitchen'
	@echo '               or skip:     make <target> DEVICE='

# --- Toolchain setup -------------------------------------------------------
.PHONY: setup
setup: ## Install required toolchain and tools
	@command -v ldproxy >/dev/null 2>&1 || { echo "Installing ldproxy..."; cargo install ldproxy; }
	@command -v espflash >/dev/null 2>&1 || { echo "Installing espflash..."; cargo install espflash; }
	@command -v espup >/dev/null 2>&1 || { echo "Installing espup..."; cargo install espup; }
	@rustup toolchain list | grep -q esp || { echo "Installing Xtensa toolchain..."; espup install; }

.PHONY: all
all: lint test build ## Lint, test, and build

.PHONY: lint
lint: setup ## Format and lint
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
build: setup ## Build the firmware (release)
	$(CARGO) build --release --target $(TARGET)

.PHONY: flash
flash: setup ## Build and flash the firmware, then open the serial monitor
	$(CARGO) run --release --target $(TARGET)

.PHONY: monitor
monitor: setup ## Open the serial monitor (no build/flash)
	espflash monitor

.PHONY: register
register: setup ## Register the connected board as DEVICE (e.g. make register DEVICE=kitchen)
ifndef DEVICE
	$(error DEVICE is required. Usage: make register DEVICE=kitchen)
endif
	@echo "Reading board info from connected device..."
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
