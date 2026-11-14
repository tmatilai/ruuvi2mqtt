# The board, as in ../ruuvi2mqtt-esp32: BOARD from the firmware's
# boards/boards.conf, or the only attached board if it's registered
# there. Its boards/<name>.mk sets CHIP and the LED. BOARD= skips the
# registry. The firmware's boards.sh finds the port, so no target prompts.
BOARDS_DIR := ../ruuvi2mqtt-esp32/boards
BOARDS_SH = BOARDS_CONF=$(BOARDS_DIR)/boards.conf ESPFLASH_PORT='$(ESPFLASH_PORT)' ../ruuvi2mqtt-esp32/boards.sh
BOARD_TARGETS := lint build flash monitor log

# DEVICE and devices/ are the old names of BOARD and boards/.
ifneq ($(origin DEVICE),undefined)
  $(error DEVICE is now BOARD: use BOARD=$(DEVICE))
endif
ifneq ($(wildcard ../ruuvi2mqtt-esp32/devices/devices.conf ../ruuvi2mqtt-esp32/devices/*.mk),)
  $(error ../ruuvi2mqtt-esp32/devices/ is now ../ruuvi2mqtt-esp32/boards/. Move the files: mv ../ruuvi2mqtt-esp32/devices/*.mk ../ruuvi2mqtt-esp32/boards/; mv ../ruuvi2mqtt-esp32/devices/devices.conf ../ruuvi2mqtt-esp32/boards/boards.conf)
endif
PORT_TARGETS := flash monitor log

ifneq ($(filter $(BOARD_TARGETS),$(MAKECMDGOALS)),)
  ifeq ($(origin BOARD),undefined)
    ifneq ($(wildcard $(BOARDS_DIR)/boards.conf),)
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
    ifeq ($(wildcard $(BOARDS_DIR)/$(BOARD).mk),)
      $(error Board config '$(BOARDS_DIR)/$(BOARD).mk' not found)
    endif
    include $(BOARDS_DIR)/$(BOARD).mk
  endif
endif
ifneq ($(filter $(PORT_TARGETS),$(MAKECMDGOALS)),)
  ifeq ($(ESPFLASH_PORT),)
    ESPFLASH_PORT := $(shell $(BOARDS_SH) $(if $(and $(BOARD),$(wildcard $(BOARDS_DIR)/boards.conf)),port $(BOARD),only-port))
    ifeq ($(ESPFLASH_PORT),)
      $(error No port for the board, see above)
    endif
  endif
  $(info Using port: $(ESPFLASH_PORT))
  export ESPFLASH_PORT
endif

# Target chip: esp32c3, esp32c5, esp32c6 or esp32s3. The original ESP32
# has no Bluetooth 5.
CHIP ?= esp32c6

ifeq ($(CHIP),esp32c3)
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
  $(error Unsupported CHIP=$(CHIP). Use: esp32c3, esp32c5, esp32c6, esp32s3)
endif

# A target directory per chip, as in ../ruuvi2mqtt-esp32.
export MCU := $(CHIP)
export CARGO_TARGET_DIR ?= target/$(CHIP)
export CARGO_WORKSPACE_DIR ?= $(CURDIR)
CARGO := cargo $(TOOLCHAIN)

HOST_CARGO := cargo +stable
HOST_TARGET := --target host-tuple

.DEFAULT_GOAL := help

.PHONY: help
help: ## Show this help
	@grep -hE '^[a-zA-Z-][a-zA-Z0-9_\.-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-10s\033[0m %s\n", $$1, $$2}'

.PHONY: lint
lint: ## Format and lint
	$(CARGO) fmt
	$(CARGO) clippy --target $(TARGET) -- -D clippy::all -W clippy::pedantic
	$(HOST_CARGO) clippy $(HOST_TARGET) --lib --tests -- -D clippy::all -W clippy::pedantic

.PHONY: test
test: ## Run the host-side unit tests
	$(HOST_CARGO) test $(HOST_TARGET) --lib

.PHONY: build
build: ## Build the firmware (release)
	$(CARGO) build --release --target $(TARGET)

.PHONY: flash
flash: build ## Build and flash the firmware (`make flash log` to see its address)
	espflash flash --non-interactive $(CARGO_TARGET_DIR)/$(TARGET)/release/air-simulator

.PHONY: monitor
monitor: ## Open the interactive serial monitor
	espflash monitor

# Seconds of serial output for `make log`, enough for the boot log
SECS ?= 10

.PHONY: log
log: ## Print the serial output for SECS seconds, without prompts
	$(BOARDS_SH) log $(SECS)

.PHONY: boards
boards: ## List the attached boards (resets unknown ones behind a USB-UART bridge)
	@$(BOARDS_SH) list

.PHONY: clean
clean: ## Remove build artifacts
	$(CARGO) clean
