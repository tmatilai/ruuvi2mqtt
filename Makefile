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
lint: fmt-docs check-docs ## Check the version, format and lint
	script/check-version
	cargo fmt --all
	cargo clippy --workspace --tests --all-targets --all-features -- -D clippy::all -W clippy::pedantic

.PHONY: test
test: lint ## Lint and test
	cargo test --workspace

.PHONY: local
local: test ## Test and build locally
	cargo build

.PHONY: cross
cross: $(ARCHS) ## Build all non-local architectures

# The same version as CI, so local builds use the release builds' images.
# renovate: datasource=crate depName=cross
CROSS_VERSION := 0.2.5

# renovate: datasource=npm depName=prettier
PRETTIER_VERSION := 3.10.0
# CI sets --check.
PRETTIER_MODE := --write

.PHONY: fmt-docs
fmt-docs: ## Format the Markdown files with Prettier
	npx --yes prettier@$(PRETTIER_VERSION) $(PRETTIER_MODE) --log-level warn $$(git ls-files '*.md')

# The same version as CI.
# renovate: datasource=crate depName=typos-cli
TYPOS_VERSION := 1.52.2

# renovate: datasource=npm depName=remark-cli
REMARK_CLI_VERSION := 12.0.1
# renovate: datasource=npm depName=remark-validate-links
REMARK_VALIDATE_LINKS_VERSION := 13.1.0

# remark looks for a plugin from the working directory, not beside itself, so
# it gets the plugin's path.
.PHONY: check-docs
check-docs: ## Check the spelling, and the links between the Markdown files
	typos
	npx --yes -p remark-cli@$(REMARK_CLI_VERSION) -p remark-validate-links@$(REMARK_VALIDATE_LINKS_VERSION) \
		-c 'remark --use "$$(dirname "$$(command -v remark)")/../remark-validate-links/index.js" --frail --quiet --no-stdout $$(git ls-files "*.md")'

.PHONY: setup
setup: ## Install the tools for the app and the ESP32 firmware
	cargo install --locked cross@$(CROSS_VERSION)
	cargo install --locked typos-cli@$(TYPOS_VERSION)
	$(MAKE) -C ruuvi2mqtt-esp32 setup

.PHONY: $(ARCHS)
$(ARCHS): ## Build for the specified architecture
	cross build --target $(TARGET_ARCH_$@)

.PHONY: release
release: ## Prepare the release PR: make release VERSION=1.2.3
	script/release $(VERSION)
