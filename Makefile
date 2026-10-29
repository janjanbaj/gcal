# ==============================================================================
# GCAL-TUI - Dynamic Makefile & Build Automation
# ==============================================================================

# OS & Architecture Detection
OS := $(shell uname -s 2>/dev/null || echo Unknown)
ARCH := $(shell uname -m 2>/dev/null || echo Unknown)
SHELL := /bin/bash

# Dynamic Path Discovery
HOME_DIR ?= $(HOME)
CARGO ?= $(shell command -v cargo 2>/dev/null || echo $(HOME_DIR)/.cargo/bin/cargo)
RUSTC ?= $(shell command -v rustc 2>/dev/null || echo $(HOME_DIR)/.cargo/bin/rustc)

# Binary & Installation Targets
BIN_NAME := gcal
CARGO_BIN_DIR ?= $(HOME_DIR)/.cargo/bin
TARGET_RELEASE := target/release/$(BIN_NAME)

# Config Paths by Platform
ifeq ($(OS),Darwin)
	CONFIG_DIR := $(HOME_DIR)/Library/Application Support/com.jorg.j-gcal
	DATA_DIR := $(HOME_DIR)/Library/Application Support/com.jorg.j-gcal
else
	CONFIG_DIR := $(HOME_DIR)/.config/j-gcal
	DATA_DIR := $(HOME_DIR)/.local/share/j-gcal
endif

# ANSI Terminal Colors
BOLD := \033[1m
GREEN := \033[32m
CYAN := \033[36m
YELLOW := \033[33m
RED := \033[31m
RESET := \033[0m

.PHONY: all help check-deps setup build release install uninstall test lint clippy run clean audit-secrets

# Default Target
all: check-deps setup release install
	@echo -e "$(GREEN)$(BOLD)✓ Build & installation complete!$(RESET)"
	@echo -e "Run $(CYAN)$(BIN_NAME)$(RESET) in your terminal to start."

# Help Target
help:
	@echo -e "$(CYAN)$(BOLD)GCAL-TUI - Build & Management Automation$(RESET)"
	@echo -e "$(BOLD)Platform:$(RESET) $(OS) ($(ARCH)) | $(BOLD)Cargo:$(RESET) $(CARGO)"
	@echo ""
	@echo -e "$(YELLOW)Available targets:$(RESET)"
	@echo -e "  $(GREEN)make$(RESET) / $(GREEN)make all$(RESET)       Check deps, setup config, build release & install"
	@echo -e "  $(GREEN)make setup$(RESET)          Initialize .env template & application directories"
	@echo -e "  $(GREEN)make build$(RESET)          Compile in debug mode"
	@echo -e "  $(GREEN)make release$(RESET)        Compile optimized release binary with LTO & strip"
	@echo -e "  $(GREEN)make install$(RESET)        Install release binary to $(CARGO_BIN_DIR)/$(BIN_NAME)"
	@echo -e "  $(GREEN)make uninstall$(RESET)      Remove binary from $(CARGO_BIN_DIR)/$(BIN_NAME)"
	@echo -e "  $(GREEN)make test$(RESET)           Run complete automated test suite"
	@echo -e "  $(GREEN)make lint$(RESET)           Run clippy with -D warnings"
	@echo -e "  $(GREEN)make run$(RESET)            Compile release & launch gcal"
	@echo -e "  $(GREEN)make audit-secrets$(RESET)  Scan repository for secrets before pushing to GitHub"
	@echo -e "  $(GREEN)make clean$(RESET)          Clean build artifacts (target/)"

# Dependency Verification
check-deps:
	@echo -e "$(CYAN)Checking dependencies...$(RESET)"
	@if ! command -v $(CARGO) >/dev/null 2>&1; then \
		echo -e "$(RED)Error: Rust and Cargo were not found in PATH or at $(HOME_DIR)/.cargo/bin/cargo.$(RESET)"; \
		echo -e "Install Rust via: $(BOLD)curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh$(RESET)"; \
		exit 1; \
	fi
	@echo -e "  Cargo found: $$($(CARGO) --version)"
	@echo -e "  Rustc found: $$($(RUSTC) --version)"

# Setup Environment & Directories
setup: check-deps
	@echo -e "$(CYAN)Setting up application directories & configuration...$(RESET)"
	@mkdir -p "$(CONFIG_DIR)" "$(DATA_DIR)"
	@chmod 700 "$(CONFIG_DIR)" 2>/dev/null || true
	@chmod 700 "$(DATA_DIR)" 2>/dev/null || true
	@if [ ! -f .env ] && [ ! -f "$(CONFIG_DIR)/.env" ] && [ ! -f "$(DATA_DIR)/.env" ]; then \
		if [ -f .env.example ]; then \
			echo -e "  $(YELLOW)No .env found. Copying .env.example to .env...$(RESET)"; \
			cp .env.example .env; \
			chmod 600 .env 2>/dev/null || true; \
			echo -e "  $(YELLOW)! Please configure your GCAL_CLIENT_ID & GCAL_CLIENT_SECRET in .env$(RESET)"; \
		fi \
	else \
		echo -e "  $(GREEN)✓ Active .env configuration detected.$(RESET)"; \
	fi

# Build Debug
build: check-deps
	@echo -e "$(CYAN)Compiling debug build...$(RESET)"
	@$(CARGO) build

# Build Release
release: check-deps
	@echo -e "$(CYAN)Compiling optimized release build (LTO, strip, single codegen unit)...$(RESET)"
	@$(CARGO) build --release

# Install Binary
install: release
	@echo -e "$(CYAN)Installing $(BIN_NAME) to $(CARGO_BIN_DIR)...$(RESET)"
	@mkdir -p "$(CARGO_BIN_DIR)"
	@cp -f "$(TARGET_RELEASE)" "$(CARGO_BIN_DIR)/$(BIN_NAME)"
	@chmod 755 "$(CARGO_BIN_DIR)/$(BIN_NAME)"
	@echo -e "$(GREEN)✓ Successfully installed: $(CARGO_BIN_DIR)/$(BIN_NAME)$(RESET)"
	@case ":$$PATH:" in \
		*":$(CARGO_BIN_DIR):"*) ;; \
		*) echo -e "$(YELLOW)Note: $(CARGO_BIN_DIR) is not in your PATH. Add it to your shell profile (.zshrc or .bashrc):$(RESET)\n  export PATH=\"$(CARGO_BIN_DIR):\$$PATH\"" ;; \
	esac

# Uninstall Binary
uninstall:
	@echo -e "$(YELLOW)Uninstalling $(BIN_NAME)...$(RESET)"
	@rm -f "$(CARGO_BIN_DIR)/$(BIN_NAME)"
	@echo -e "$(GREEN)✓ Removed $(CARGO_BIN_DIR)/$(BIN_NAME)$(RESET)"

# Run Test Suite
test: check-deps
	@echo -e "$(CYAN)Running automated tests...$(RESET)"
	@$(CARGO) test

# Lint & Clippy
lint clippy: check-deps
	@echo -e "$(CYAN)Running Clippy with -D warnings...$(RESET)"
	@$(CARGO) clippy --all-targets -- -D warnings

# Run Application
run: release
	@echo -e "$(CYAN)Starting $(BIN_NAME)...$(RESET)"
	@$(CARGO) run --release

# Clean Build Cache
clean:
	@echo -e "$(CYAN)Cleaning build cache...$(RESET)"
	@$(CARGO) clean

# Pre-Push Secrets Audit
audit-secrets:
	@echo -e "$(CYAN)Auditing repository for secrets & identifying information...$(RESET)"
	@git status --porcelain | grep -E '\.env$$|\.token|tokencache|client_secret.*\.json' && { \
		echo -e "$(RED)$(BOLD)FAIL: Potential secret files detected in git working tree!$(RESET)"; \
		exit 1; \
	} || true
	@if git grep -I -i -E "client_secret.*=.*[A-Za-z0-9_-]{20,}" -- ':!.env.example' 2>/dev/null; then \
		echo -e "$(RED)$(BOLD)FAIL: Hardcoded secret pattern found in tracked files!$(RESET)"; \
		exit 1; \
	fi
	@if [ -f .env.example ]; then \
		if grep -E 'GCAL_CLIENT_ID="[0-9]{10,}' .env.example >/dev/null 2>&1; then \
			echo -e "$(RED)$(BOLD)FAIL: Real client ID found in .env.example!$(RESET)"; \
			exit 1; \
		fi \
	fi
	@echo -e "$(GREEN)$(BOLD)✓ Pre-push secrets audit passed cleanly! Safe to commit and push.$(RESET)"
