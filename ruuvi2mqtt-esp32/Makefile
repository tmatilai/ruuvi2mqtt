# --- Board configuration ---------------------------------------------------
# Set BOARD explicitly, as in `make flash BOARD=kitchen`, or let Make
# auto-detect the only attached board from its MAC address via
# boards/boards.conf. Set BOARD to empty to skip auto-detection, as in
# `make flash BOARD=`. boards.sh finds the boards and their ports.
#
# Auto-detection runs only for targets that need it and only when
# boards/boards.conf exists. ESPFLASH_PORT limits it to one port.
BOARD_TARGETS := all lint build flash monitor log
# The targets that talk to the board. They get its port in ESPFLASH_PORT.
PORT_TARGETS := flash monitor log
BOARDS_SH = ESPFLASH_PORT='$(ESPFLASH_PORT)' WAIT='$(WAIT)' ./boards.sh

# Seconds to wait for a board's port. In deep sleep, the chip's own USB
# port is off.
WAIT ?= 75

ESPFLASH_MISSING = $(if $(shell command -v espflash 2>/dev/null),,$(error espflash not found. Run 'make setup' first))

# DEVICE and devices/ are the old names of BOARD and boards/.
ifneq ($(origin DEVICE),undefined)
  $(error DEVICE is now BOARD: use BOARD=$(DEVICE))
endif
ifneq ($(wildcard devices/devices.conf devices/*.mk),)
  $(error devices/ is now boards/. Move the files: mv devices/*.mk boards/; mv devices/devices.conf boards/boards.conf)
endif

ifneq ($(filter $(BOARD_TARGETS),$(MAKECMDGOALS)),)
  # Auto-detect the board when BOARD is not set at all
  ifeq ($(origin BOARD),undefined)
    ifneq ($(wildcard boards/boards.conf),)
      $(ESPFLASH_MISSING)
      $(info Auto-detecting the attached board...)
      DETECTED := $(shell $(BOARDS_SH) detect)
      ifeq ($(DETECTED),)
        $(error Auto-detection failed, see above)
      endif
      BOARD := $(word 1,$(DETECTED))
      ESPFLASH_PORT := $(word 2,$(DETECTED))
    endif
  endif

  ifneq ($(BOARD),)
    $(info Using board: $(BOARD))
    ifeq ($(wildcard boards/$(BOARD).mk),)
      $(error Board config 'boards/$(BOARD).mk' not found. Register with 'make register BOARD=$(BOARD)' first.)
    endif
    include boards/$(BOARD).mk
  endif

  ifneq ($(filter $(PORT_TARGETS),$(MAKECMDGOALS)),)
    $(ESPFLASH_MISSING)
    ifeq ($(ESPFLASH_PORT),)
      # A registered BOARD never falls back to another board.
      ifneq ($(and $(BOARD),$(wildcard boards/boards.conf)),)
        ESPFLASH_PORT := $(shell $(BOARDS_SH) port $(BOARD))
      else
        ESPFLASH_PORT := $(shell $(BOARDS_SH) only-port)
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

# Target chip. Override with CHIP=esp32 or in a board's .mk file.
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
# environment wins, such as the test bed's. esp-idf-sys looks for the crate
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
	@echo 'Board config:  auto-detected from the attached board (see boards/),'
	@echo '               or choose:   make <target> BOARD=kitchen'
	@echo '               or skip:     make <target> BOARD='
	@echo 'Serial port:   found from BOARD, or set ESPFLASH_PORT=<port>'
	@echo '               (`make boards` lists the attached boards)'

# --- Toolchain setup -------------------------------------------------------
# Tool versions are bumped by Renovate. Re-run `make setup` after pulling.
# renovate: datasource=crate depName=espflash
ESPFLASH_VERSION := 4.7.0
# renovate: datasource=crate depName=espup
ESPUP_VERSION := 0.19.0
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

# esp-idf-sys doesn't rebuild ESP-IDF when its config in Cargo.toml changes,
# so a new component would be missing. After a new ESP_IDF_VERSION, the
# bootloader's CMake cache still points at the old ESP-IDF, and the build
# fails.
ESP_IDF_SYS_STAMP = $(CARGO_TARGET_DIR)/$(TARGET)/esp-idf-sys.toml

.PHONY: esp-idf-sys-config
esp-idf-sys-config:
	@mkdir -p $(dir $(ESP_IDF_SYS_STAMP))
	@{ awk '/^\[/ { s = /^\[+package\.metadata\.esp-idf-sys/ } s && !/^[ \t]*(#|$$)/' Cargo.toml; \
	  grep -E '^ESP_IDF_VERSION *=' .cargo/config.toml; } >$(ESP_IDF_SYS_STAMP).new
	@if ! cmp -s $(ESP_IDF_SYS_STAMP).new $(ESP_IDF_SYS_STAMP); then \
	  echo "New esp-idf-sys config or ESP_IDF_VERSION: cleaning its build"; \
	  $(CARGO) clean -p esp-idf-sys --target $(TARGET) && \
	  $(CARGO) clean -p esp-idf-sys --target $(TARGET) --release; \
	fi
	@mv $(ESP_IDF_SYS_STAMP).new $(ESP_IDF_SYS_STAMP)

.PHONY: lint
lint: esp-idf-sys-config ## Format and lint
	$(CARGO) fmt
	$(CARGO) clippy --target $(TARGET) -- -D clippy::all -W clippy::pedantic
	$(HOST_CARGO) clippy $(HOST_TARGET) --lib --tests -- -D clippy::all -W clippy::pedantic

.PHONY: test
test: ## Run host-side unit tests (pure-Rust modules only)
	$(HOST_CARGO) test $(HOST_TARGET) --lib

.PHONY: build
build: esp-idf-sys-config ## Build the firmware (release)
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
	$(BOARDS_SH) log $(SECS)

.PHONY: boards
boards: ## List the attached boards (resets unknown ones behind a USB-UART bridge)
	@$(BOARDS_SH) list

.PHONY: register
register: ## Register the attached board as BOARD (make register BOARD=kitchen)
ifndef BOARD
	$(error BOARD is required. Usage: make register BOARD=kitchen)
endif
	@$(BOARDS_SH) register $(BOARD)

.PHONY: clean
clean: ## Remove build artifacts
	$(CARGO) clean
