# VoiceChat Development Makefile
#
# Usage: make <target>
# Run `make help` to see all available targets.

.PHONY: help setup init bootstrap doctor dev server client test check lint fmt clean \
        services-up services-down migrate \
        docker-up docker-down docker-logs docker-clean \
        db-migrate db-reset db-seed \
        test-everyone-security \
        build release

# Default target
.DEFAULT_GOAL := help

# Colors
CYAN := \033[36m
GREEN := \033[32m
YELLOW := \033[33m
RESET := \033[0m

COMPOSE_CMD := $(shell if command -v docker >/dev/null 2>&1; then echo "docker compose"; elif command -v podman-compose >/dev/null 2>&1; then echo "podman-compose"; else echo "docker compose"; fi)

#==============================================================================
# Help
#==============================================================================

help: ## Show this help message
	@echo ""
	@echo "$(CYAN)VoiceChat Development Commands$(RESET)"
	@echo ""
	@echo "$(GREEN)Setup:$(RESET)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | grep -E "setup|install" | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(CYAN)%-15s$(RESET) %s\n", $$1, $$2}'
	@echo ""
	@echo "$(GREEN)Development:$(RESET)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | grep -E "dev|server|client|watch" | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(CYAN)%-15s$(RESET) %s\n", $$1, $$2}'
	@echo ""
	@echo "$(GREEN)Testing & Quality:$(RESET)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | grep -E "test|check|lint|fmt" | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(CYAN)%-15s$(RESET) %s\n", $$1, $$2}'
	@echo ""
	@echo "$(GREEN)Docker:$(RESET)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | grep -E "docker" | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(CYAN)%-15s$(RESET) %s\n", $$1, $$2}'
	@echo ""
	@echo "$(GREEN)Database:$(RESET)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | grep -E "db-" | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(CYAN)%-15s$(RESET) %s\n", $$1, $$2}'
	@echo ""
	@echo "$(GREEN)Build:$(RESET)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | grep -E "build|release|clean" | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(CYAN)%-15s$(RESET) %s\n", $$1, $$2}'
	@echo ""

#==============================================================================
# Setup
#==============================================================================

setup: ## Run full development setup
	@./scripts/dev-setup.sh

init: setup ## Initialize project development environment

bootstrap: install ## Install project dependencies

doctor: ## Verify toolchain, env, and service status
	@echo "$(CYAN)Checking required commands...$(RESET)"
	@command -v cargo >/dev/null || (echo "$(YELLOW)Missing: cargo$(RESET)" && exit 1)
	@command -v bun >/dev/null || (echo "$(YELLOW)Missing: bun$(RESET)" && exit 1)
	@(command -v docker >/dev/null || command -v podman >/dev/null) || (echo "$(YELLOW)Missing: docker or podman$(RESET)" && exit 1)
	@(docker compose version >/dev/null 2>&1 || podman-compose --version >/dev/null 2>&1) || (echo "$(YELLOW)Missing: docker compose plugin or podman-compose$(RESET)" && exit 1)
	@echo "$(CYAN)Checking environment file...$(RESET)"
	@[ -f .env ] && echo "$(GREEN).env present$(RESET)" || echo "$(YELLOW).env missing (run make setup)$(RESET)"
	@echo "$(CYAN)Checking service status...$(RESET)"
	@(docker compose -f docker-compose.dev.yml ps >/dev/null 2>&1 || podman-compose -f docker-compose.dev.yml ps >/dev/null 2>&1) && echo "$(GREEN)Compose reachable$(RESET)" || echo "$(YELLOW)Compose services unavailable (run make services-up)$(RESET)"

setup-clean: ## Clean setup (removes .env and Docker volumes)
	@./scripts/dev-setup.sh --clean

install: ## Install all dependencies (Rust + Node)
	@echo "$(CYAN)Installing Rust dependencies...$(RESET)"
	@cargo fetch
	@echo "$(CYAN)Installing Node dependencies...$(RESET)"
	@cd client && bun install

install-tools: ## Install development tools (sqlx-cli, cargo-watch, etc.)
	@echo "$(CYAN)Installing development tools...$(RESET)"
	cargo install sqlx-cli --no-default-features --features postgres
	cargo install cargo-watch
	cargo install cargo-deny
	cargo install cargo-nextest

#==============================================================================
# Development
#==============================================================================

dev: docker-up ## Start server in watch mode (auto-reload)
	@echo "$(CYAN)Starting server in watch mode...$(RESET)"
	cargo watch -x 'run -p vc-server'

server: ## Start server (no auto-reload)
	cargo run -p vc-server

client: ## Start client in dev mode
	cd client && bun run tauri dev

client-web: ## Start client web UI only (no Tauri)
	cd client && bun run dev

watch: docker-up ## Start both server (watch) and client
	@echo "$(YELLOW)Starting server and client...$(RESET)"
	@echo "$(YELLOW)Note: Run in separate terminals for better experience$(RESET)"
	@echo ""
	@echo "  Terminal 1: make dev"
	@echo "  Terminal 2: make client"

#==============================================================================
# Testing & Quality
#==============================================================================

test: ## Run all tests
	cargo nextest run 2>/dev/null || cargo test

test-server: ## Run server tests only
	cargo nextest run -p vc-server 2>/dev/null || cargo test -p vc-server

test-watch: ## Run tests in watch mode
	cargo watch -x 'nextest run' 2>/dev/null || cargo watch -x test

test-everyone-security: ## Run ignored @everyone security integration test in Docker
	@./scripts/test-everyone-security.sh

check: ## Run cargo check and clippy
	@echo "$(CYAN)Running cargo check...$(RESET)"
	cargo check --all-targets
	@echo "$(CYAN)Running clippy...$(RESET)"
	cargo clippy --all-targets -- -D warnings

lint: check ## Alias for check
	@cd client && bun run lint

fmt: ## Format all code
	cargo fmt --all
	cd client && bun run format

fmt-check: ## Check code formatting
	cargo fmt --all -- --check
	cd client && bun run format -- --check

licenses: ## Check dependency licenses
	cargo deny check licenses

audit: ## Security audit of dependencies
	cargo deny check advisories

#==============================================================================
# Docker
#==============================================================================

docker-up: ## Start Docker services (PostgreSQL, Valkey, RustFS, MailHog)
	@$(COMPOSE_CMD) -f docker-compose.dev.yml up -d

services-up: docker-up ## Start development services

docker-down: ## Stop Docker services
	@$(COMPOSE_CMD) -f docker-compose.dev.yml down

services-down: docker-down ## Stop development services

docker-logs: ## View Docker service logs
	@$(COMPOSE_CMD) -f docker-compose.dev.yml logs -f

docker-ps: ## Show Docker service status
	@$(COMPOSE_CMD) -f docker-compose.dev.yml ps

docker-clean: ## Stop services and remove volumes
	@$(COMPOSE_CMD) -f docker-compose.dev.yml down -v
	@echo "$(GREEN)Docker volumes removed$(RESET)"

docker-restart: docker-down docker-up ## Restart Docker services

#==============================================================================
# Database
#==============================================================================

db-migrate: ## Run database migrations
	sqlx migrate run --source server/migrations

migrate: db-migrate ## Run database migrations

db-revert: ## Revert last migration
	sqlx migrate revert --source server/migrations

db-reset: ## Reset database (drop and recreate)
	@echo "$(YELLOW)Resetting database...$(RESET)"
	sqlx database drop -y || true
	sqlx database create
	sqlx migrate run --source server/migrations
	@echo "$(GREEN)Database reset complete$(RESET)"

db-status: ## Show migration status
	sqlx migrate info --source server/migrations

db-shell: ## Open psql shell
	@docker exec -it voicechat-dev-postgres psql -U voicechat -d voicechat

db-seed: ## Seed database with test data
	@echo "$(CYAN)Seeding database...$(RESET)"
	@if [ -f server/seeds/dev.sql ]; then \
		docker exec -i voicechat-dev-postgres psql -U voicechat -d voicechat < server/seeds/dev.sql; \
	else \
		echo "$(YELLOW)No seed file found at server/seeds/dev.sql$(RESET)"; \
	fi

#==============================================================================
# Build
#==============================================================================

build: ## Build all packages (debug)
	cargo build --all

build-server: ## Build server only
	cargo build -p vc-server

build-client: ## Build client app
	cd client && bun run tauri build

release: ## Build release binaries
	cargo build --release --all

release-server: ## Build server release binary
	cargo build --release -p vc-server

clean: ## Clean build artifacts
	cargo clean
	rm -rf client/dist client/src-tauri/target

clean-all: clean docker-clean ## Clean everything including Docker volumes

#==============================================================================
# Utilities
#==============================================================================

.env: ## Create .env from example
	@if [ ! -f .env ]; then \
		cp .env.example .env; \
		echo "$(GREEN)Created .env from .env.example$(RESET)"; \
		echo "$(YELLOW)Please update JWT_SECRET with a secure value$(RESET)"; \
	else \
		echo "$(YELLOW).env already exists$(RESET)"; \
	fi

docs: ## Generate and open documentation
	cargo doc --open --no-deps

loc: ## Count lines of code
	@echo "$(CYAN)Lines of code:$(RESET)"
	@find . -name '*.rs' -not -path './target/*' | xargs wc -l | tail -1
	@find client/src -name '*.ts' -o -name '*.tsx' | xargs wc -l 2>/dev/null | tail -1 || echo "  0 TypeScript"

tree: ## Show project structure
	@tree -I 'target|node_modules|dist|.git' -L 3
