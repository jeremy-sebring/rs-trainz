# rs-trainz Makefile
# DC Model Train Throttle Controller
# 
# Usage: make <target>
# Run 'make help' for available targets

# ESP32 toolchain and target
ESP_TOOLCHAIN := +esp
ESP_TARGET := riscv32imc-esp-espidf
ESP_RELEASE := --release

# Find ESP32 GCC dynamically (installed by esp-idf-sys)
ESP_GCC := $(shell find ~/.espressif/tools/riscv32-esp-elf -name "riscv32-esp-elf-gcc" 2>/dev/null | head -1)
ifdef ESP_GCC
  ESP_RUSTFLAGS := -C link-arg=-nostartfiles -C link-arg=--ldproxy-linker=$(ESP_GCC)
  export CARGO_TARGET_RISCV32IMC_ESP_ESPIDF_RUSTFLAGS = $(ESP_RUSTFLAGS)
endif

# Check for ESP toolchain before ESP builds
.PHONY: check-esp-toolchain
check-esp-toolchain:
ifndef ESP_GCC
	@echo "$(YELLOW)ESP32 GCC toolchain not found in ~/.espressif$(NC)"
	@echo "Run 'make esp' once to let esp-idf-sys download it, then retry."
	@exit 1
endif

# Colors for pretty output
GREEN := \033[0;32m
YELLOW := \033[0;33m
CYAN := \033[0;36m
NC := \033[0m # No Color

.PHONY: help build check test test-single clippy lint fmt clean \
        esp esp-display esp-wifi esp-full flash monitor \
        no-std doc ci

#=============================================================================
# Help
#=============================================================================

help: ## Show this help message
	@echo ""
	@echo "$(CYAN)rs-trainz$(NC) - DC Model Train Throttle Controller"
	@echo ""
	@echo "$(YELLOW)Desktop Targets:$(NC)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*Desktop' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(GREEN)%-15s$(NC) %s\n", $$1, $$2}'
	@echo ""
	@echo "$(YELLOW)ESP32 Targets:$(NC)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*ESP32' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(GREEN)%-15s$(NC) %s\n", $$1, $$2}'
	@echo ""
	@echo "$(YELLOW)Quality Targets:$(NC)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*Quality' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(GREEN)%-15s$(NC) %s\n", $$1, $$2}'
	@echo ""
	@echo "$(YELLOW)Other Targets:$(NC)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*Other' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(GREEN)%-15s$(NC) %s\n", $$1, $$2}'
	@echo ""

#=============================================================================
# Desktop Build & Test
#=============================================================================

build: ## [Desktop] Build for desktop/testing
	cargo build

check: ## [Desktop] Check compilation without building
	cargo check

test: ## [Desktop] Run all tests
	cargo test

test-single: ## [Desktop] Run a single test (usage: make test-single TEST=test_name)
	@if [ -z "$(TEST)" ]; then \
		echo "$(YELLOW)Usage: make test-single TEST=test_name$(NC)"; \
		exit 1; \
	fi
	cargo test $(TEST)

test-verbose: ## [Desktop] Run all tests with verbose output
	cargo test -- --nocapture

#=============================================================================
# Code Quality
#=============================================================================

clippy: ## [Quality] Run clippy linter
	cargo clippy -- -D warnings

clippy-fix: ## [Quality] Run clippy and apply automatic fixes
	cargo clippy --fix --allow-dirty --allow-staged

fmt: ## [Quality] Format code with rustfmt
	cargo fmt

fmt-check: ## [Quality] Check code formatting without making changes
	cargo fmt -- --check

lint: clippy fmt-check ## [Quality] Run all linting (clippy + fmt check)

#=============================================================================
# no_std Compatibility
#=============================================================================

no-std: ## [Quality] Verify no_std compatibility
	cargo check --no-default-features

#=============================================================================
# ESP32 Builds
#=============================================================================

esp: ## [ESP32] Build basic ESP32 firmware
	cargo $(ESP_TOOLCHAIN) build $(ESP_RELEASE) --target $(ESP_TARGET) --features esp32

esp-display: ## [ESP32] Build with display support
	cargo $(ESP_TOOLCHAIN) build $(ESP_RELEASE) --target $(ESP_TARGET) --features esp32,display

esp-wifi: ## [ESP32] Build with WiFi support
	cargo $(ESP_TOOLCHAIN) build $(ESP_RELEASE) --target $(ESP_TARGET) --features wifi

esp-http: ## [ESP32] Build with HTTP web API
	cargo $(ESP_TOOLCHAIN) build $(ESP_RELEASE) --target $(ESP_TARGET) --features esp32-http

esp-mqtt: ## [ESP32] Build with MQTT support
	cargo $(ESP_TOOLCHAIN) build $(ESP_RELEASE) --target $(ESP_TARGET) --features esp32-mqtt

esp-net: ## [ESP32] Build with HTTP + MQTT
	cargo $(ESP_TOOLCHAIN) build $(ESP_RELEASE) --target $(ESP_TARGET) --features esp32-net

esp-full: ## [ESP32] Build with all features (display + http + mqtt)
	cargo $(ESP_TOOLCHAIN) build $(ESP_RELEASE) --target $(ESP_TARGET) --features esp32,display,esp32-net

esp-check: ## [ESP32] Check ESP32 build without compiling
	cargo $(ESP_TOOLCHAIN) check --target $(ESP_TARGET) --features esp32

esp-clippy: ## [ESP32] Run clippy on ESP32 code
	cargo $(ESP_TOOLCHAIN) clippy --target $(ESP_TARGET) --features esp32 -- -D warnings

#=============================================================================
# Flash & Monitor
#=============================================================================

flash: ## [ESP32] Flash basic firmware to device
	espflash flash target/$(ESP_TARGET)/release/esp32_main

flash-display: esp-display ## [ESP32] Build and flash with display
	espflash flash target/$(ESP_TARGET)/release/esp32_main

flash-http: esp-http ## [ESP32] Build and flash with HTTP
	espflash flash target/$(ESP_TARGET)/release/esp32_main

flash-mqtt: esp-mqtt ## [ESP32] Build and flash with MQTT
	espflash flash target/$(ESP_TARGET)/release/esp32_main

flash-net: esp-net ## [ESP32] Build and flash with HTTP + MQTT
	espflash flash target/$(ESP_TARGET)/release/esp32_main

flash-full: esp-full ## [ESP32] Build and flash with all features
	espflash flash target/$(ESP_TARGET)/release/esp32_main

monitor: ## [ESP32] Open serial monitor
	espflash monitor

flash-monitor: esp ## [ESP32] Build, flash, and open monitor
	espflash flash --monitor target/$(ESP_TARGET)/release/esp32_main

#=============================================================================
# Documentation
#=============================================================================

doc: ## [Other] Generate documentation
	cargo doc --no-deps --open

doc-all: ## [Other] Generate documentation including dependencies
	cargo doc --open

#=============================================================================
# CI & Maintenance
#=============================================================================

ci: fmt-check clippy no-std test ## [Quality] Run all CI checks (fmt, clippy, no_std, test)
	@echo "$(GREEN)All CI checks passed!$(NC)"

clean: ## [Other] Clean build artifacts
	cargo clean

clean-esp: ## [Other] Clean ESP32 build artifacts only
	rm -rf target/$(ESP_TARGET)

#=============================================================================
# Development Helpers
#=============================================================================

watch-test: ## [Desktop] Run tests on file changes (requires cargo-watch)
	cargo watch -x test

watch-check: ## [Desktop] Check on file changes (requires cargo-watch)
	cargo watch -x check

setup-esp: ## [Other] Show ESP32 toolchain setup instructions
	@echo ""
	@echo "$(CYAN)ESP32 Toolchain Setup$(NC)"
	@echo ""
	@echo "1. Install espup:"
	@echo "   $(GREEN)cargo install espup$(NC)"
	@echo ""
	@echo "2. Install the ESP toolchain:"
	@echo "   $(GREEN)espup install$(NC)"
	@echo ""
	@echo "3. Source the export file (add to shell profile):"
	@echo "   $(GREEN). ~/export-esp.sh$(NC)"
	@echo ""
	@echo "4. Install espflash for flashing:"
	@echo "   $(GREEN)cargo install espflash$(NC)"
	@echo ""
