# --- Device configuration --------------------------------------------------
# Set DEVICE explicitly (e.g. `make flash DEVICE=kitchen`) or let Make
# auto-detect the only attached board from its MAC address via
# devices/devices.conf. Set DEVICE to empty to skip auto-detection (e.g.
# `make flash DEVICE=`). boards.sh finds the boards and their ports.
#
# Auto-detection runs only for targets that need it and only when
# devices/devices.conf exists. ESPFLASH_PORT limits it to one port.
DEVICE_TARGETS := all lint build flash monitor log
# The targets that talk to the board. They get its port in ESPFLASH_PORT.
PORT_TARGETS := flash monitor log
BOARDS = ESPFLASH_PORT='$(ESPFLASH_PORT)' WAIT='$(WAIT)' ./boards.sh

# Seconds to wait for a board's port. In deep sleep, the chip's own USB
# port is off.
WAIT ?= 75

ESPFLASH_MISSING = $(if $(shell command -v espflash 2>/dev/null),,$(error espflash not found. Run 'make setup' first))

ifneq ($(filter $(DEVICE_TARGETS),$(MAKECMDGOALS)),)
  # Auto-detect device when DEVICE is not set at all
  ifeq ($(origin DEVICE),undefined)
    ifneq ($(wildcard devices/devices.conf),)
      $(ESPFLASH_MISSING)
      $(info Auto-detecting device from the attached board...)
      DETECTED := $(shell $(BOARDS) detect)
      ifeq ($(DETECTED),)
        $(error Auto-detection failed, see above)
      endif
      DEVICE := $(word 1,$(DETECTED))
      ESPFLASH_PORT := $(word 2,$(DETECTED))
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

  # Find the board's port
  ifneq ($(filter $(PORT_TARGETS),$(MAKECMDGOALS)),)
    $(ESPFLASH_MISSING)
    ifeq ($(ESPFLASH_PORT),)
      # A registered DEVICE never falls back to another board.
      ifneq ($(and $(DEVICE),$(wildcard devices/devices.conf)),)
        ESPFLASH_PORT := $(shell $(BOARDS) port $(DEVICE))
      else
        ESPFLASH_PORT := $(shell $(BOARDS) only-port)
      endif
      ifeq ($(ESPFLASH_PORT),)
        $(error No port for the board, see above)
      endif
    endif
    $(info Using port: $(ESPFLASH_PORT))
  endif
endif
ifneq ($(ESPFLASH_PORT),)
  export ESPFLASH_PORT
endif

# Target chip (default: esp32c6).
# Override: make build CHIP=esp32  or set CHIP in a device .mk file.
# Supported: esp32, esp32c3, esp32c5, esp32c6, esp32s3
CHIP ?= esp32c6

# --- Derive target triple and toolchain from CHIP ---
ifeq ($(CHIP),esp32)
  TARGET := xtensa-esp32-espidf
  TOOLCHAIN := +esp
else ifeq ($(CHIP),esp32c3)
  TARGET := riscv32imc-esp-espidf
  TOOLCHAIN :=
else ifeq ($(CHIP),esp32c5)
  TARGET := riscv32imac-esp-espidf
  TOOLCHAIN :=
else ifeq ($(CHIP),esp32c6)
  TARGET := riscv32imac-esp-espidf
  TOOLCHAIN :=
else ifeq ($(CHIP),esp32s3)
  TARGET := xtensa-esp32s3-espidf
  TOOLCHAIN := +esp
else
  $(error Unsupported CHIP=$(CHIP). Use: esp32, esp32c3, esp32c5, esp32c6, esp32s3)
endif

export MCU := $(CHIP)
# A target directory per chip: the C6 and C5 share a Rust target, but
# ESP-IDF's build can't switch chips in place. A directory set in the
# environment wins, e.g. the test bed's. esp-idf-sys looks for the crate
# (sdkconfig.defaults) next to the target directory unless
# CARGO_WORKSPACE_DIR says where it is.
export CARGO_TARGET_DIR ?= target/$(CHIP)
export CARGO_WORKSPACE_DIR ?= $(CURDIR)
CARGO := cargo $(TOOLCHAIN)

# The lib holds the pure-Rust modules. ESP-IDF is an espidf-only dependency,
# so the lib builds and tests on the host with stable Rust.
HOST_CARGO := cargo +stable
HOST_TARGET := --target host-tuple

.DEFAULT_GOAL := help

.PHONY: help
help: ## Show this help
	@grep -hE '^[a-zA-Z-][a-zA-Z0-9_\.-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'
	@echo ''
	@echo 'Run `make setup` to install the toolchain, and again after pulling.'
	@echo ''
	@echo 'Override target chip: make <target> CHIP=esp32  (default: esp32c6)'
	@echo ''
	@echo 'Device config: auto-detected from the attached board (see devices/),'
	@echo '               or choose:   make <target> DEVICE=kitchen'
	@echo '               or skip:     make <target> DEVICE='
	@echo 'Serial port:   found from DEVICE, or set ESPFLASH_PORT=<port>'
	@echo '               (`make boards` lists the attached boards)'

# --- Toolchain setup -------------------------------------------------------
# Tool versions are bumped by Renovate. Re-run `make setup` after pulling.
# renovate: datasource=crate depName=espflash
ESPFLASH_VERSION := 4.6.0
# renovate: datasource=crate depName=espup
ESPUP_VERSION := 0.18.0
# renovate: datasource=crate depName=ldproxy
LDPROXY_VERSION := 0.3.5
# The Xtensa Rust toolchain (`+esp`). CI builds ESP32-S3 with it too.
# renovate: datasource=github-releases depName=esp-rs/rust-build
XTENSA_TOOLCHAIN_VERSION := 1.97.0.0

.PHONY: setup
setup: ## Install the pinned tools and Xtensa toolchain
	cargo install --locked espflash@$(ESPFLASH_VERSION) espup@$(ESPUP_VERSION) ldproxy@$(LDPROXY_VERSION)
	espup install --toolchain-version $(XTENSA_TOOLCHAIN_VERSION) --skip-version-parse

.PHONY: all
all: lint test build ## Lint, test, and build

.PHONY: lint
lint: ## Format and lint
	$(CARGO) fmt
	$(CARGO) clippy --target $(TARGET) -- -D clippy::all -W clippy::pedantic
	$(HOST_CARGO) clippy $(HOST_TARGET) --lib --tests -- -D clippy::all -W clippy::pedantic

.PHONY: test
test: ## Run host-side unit tests (pure-Rust modules only)
	$(HOST_CARGO) test $(HOST_TARGET) --lib

.PHONY: build
build: ## Build the firmware (release)
	$(CARGO) build --release --target $(TARGET)

.PHONY: flash
flash: build ## Build and flash the firmware (`make flash monitor` to watch it)
	espflash flash --non-interactive $(CARGO_TARGET_DIR)/$(TARGET)/release/ruuvi2mqtt-esp32

.PHONY: monitor
monitor: ## Open the interactive serial monitor
	espflash monitor

# Seconds of serial output for `make log`, a bit over one cycle by default
SECS ?= 70

.PHONY: log
log: ## Print the serial output for SECS seconds, without prompts
	$(BOARDS) log $(SECS)

.PHONY: boards
boards: ## List the attached boards (resets unknown ones behind a USB-UART bridge)
	@$(BOARDS) list

.PHONY: register
register: ## Register the attached board as DEVICE (e.g. make register DEVICE=kitchen)
ifndef DEVICE
	$(error DEVICE is required. Usage: make register DEVICE=kitchen)
endif
	@$(BOARDS) register $(DEVICE)

.PHONY: clean
clean: ## Remove build artefacts
	$(CARGO) clean
