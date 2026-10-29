# The board, as in ../ruuvi2mqtt-esp32: DEVICE from the firmware's
# devices/devices.conf, or the only attached board if it's registered
# there. Its devices/<name>.mk sets CHIP and the LED. DEVICE= skips the
# registry. The firmware's boards.sh finds the port, so no target prompts.
DEVICES := ../ruuvi2mqtt-esp32/devices
BOARDS = BOARDS_CONF=$(DEVICES)/devices.conf ESPFLASH_PORT='$(ESPFLASH_PORT)' ../ruuvi2mqtt-esp32/boards.sh
DEVICE_TARGETS := lint build flash monitor log
PORT_TARGETS := flash monitor log

ifneq ($(filter $(DEVICE_TARGETS),$(MAKECMDGOALS)),)
  ifeq ($(origin DEVICE),undefined)
    ifneq ($(wildcard $(DEVICES)/devices.conf),)
      DETECTED := $(shell $(BOARDS) detect)
      ifeq ($(DETECTED),)
        $(error Auto-detection failed, see above)
      endif
      DEVICE := $(word 1,$(DETECTED))
      ESPFLASH_PORT := $(word 2,$(DETECTED))
    endif
  endif
  ifneq ($(DEVICE),)
    $(info Using device: $(DEVICE))
    ifeq ($(wildcard $(DEVICES)/$(DEVICE).mk),)
      $(error Device config '$(DEVICES)/$(DEVICE).mk' not found)
    endif
    include $(DEVICES)/$(DEVICE).mk
  endif
endif
ifneq ($(filter $(PORT_TARGETS),$(MAKECMDGOALS)),)
  ifeq ($(ESPFLASH_PORT),)
    ESPFLASH_PORT := $(shell $(BOARDS) $(if $(and $(DEVICE),$(wildcard $(DEVICES)/devices.conf)),port $(DEVICE),only-port))
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
	$(BOARDS) log $(SECS)

.PHONY: boards
boards: ## List the attached boards (resets unknown ones behind a USB-UART bridge)
	@$(BOARDS) list

.PHONY: clean
clean: ## Remove build artifacts
	$(CARGO) clean
