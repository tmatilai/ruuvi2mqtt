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

# The board's port: ESPFLASH_PORT, the board registered as DEVICE in the
# firmware's devices/devices.conf, or the only attached board. The
# firmware's boards.sh finds it, so no target prompts.
BOARDS = BOARDS_CONF=../ruuvi2mqtt-esp32/devices/devices.conf ESPFLASH_PORT='$(ESPFLASH_PORT)' ../ruuvi2mqtt-esp32/boards.sh
ifneq ($(filter flash monitor log,$(MAKECMDGOALS)),)
  ifeq ($(ESPFLASH_PORT),)
    ESPFLASH_PORT := $(shell $(BOARDS) $(if $(DEVICE),port $(DEVICE),only-port))
    ifeq ($(ESPFLASH_PORT),)
      $(error No port for the board, see above)
    endif
  endif
  $(info Using port: $(ESPFLASH_PORT))
  export ESPFLASH_PORT
endif

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
	$(BOARDS) log $(SECS)

.PHONY: boards
boards: ## List the attached boards (resets unknown ones behind a USB-UART bridge)
	@$(BOARDS) list

.PHONY: clean
clean: ## Remove build artefacts
	$(CARGO) clean
