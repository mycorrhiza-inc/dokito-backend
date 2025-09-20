# Dokito Backend - Makefile for Development and Production
# Government Document Processing System

# Configuration
PROJECT_NAME := dokito-backend
MAIN_CRATE := dokito_processing_monolith
TYPES_CRATE := dokito_types
DOCKER_IMAGE := $(PROJECT_NAME)
DOCKER_TAG := latest
PORT := 8123

# Rust configuration
CARGO := cargo
CARGO_FLAGS := --color always
RELEASE_FLAGS := --release
DEV_FLAGS := 

# Docker configuration
DOCKER := docker
DOCKERFILE := $(MAIN_CRATE)/Dockerfile

# Environment variables check
ENV_VARS := DATABASE_URL DIGITALOCEAN_S3_ACCESS_KEY DIGITALOCEAN_S3_SECRET_KEY DIGITALOCEAN_S3_ENDPOINT DIGITALOCEAN_S3_CLOUD_REGION OPENSCRAPERS_S3_OBJECT_BUCKET DEEPINFRA_API_KEY

# Colors for output
RED := \033[0;31m
GREEN := \033[0;32m
YELLOW := \033[0;33m
BLUE := \033[0;34m
NC := \033[0m # No Color

.PHONY: help dev build-dev build-prod test fmt lint check clean deps env-check db-setup docker-build docker-run docker-clean install watch all

# Default target
.DEFAULT_GOAL := help

## Help
help: ## Show this help message
	@echo "$(BLUE)Dokito Backend - Government Document Processing System$(NC)"
	@echo "$(BLUE)=================================================$(NC)"
	@echo ""
	@echo "$(GREEN)Development Commands:$(NC)"
	@echo "  $(YELLOW)dev$(NC)             Start development server with hot reload"
	@echo "  $(YELLOW)watch$(NC)           Start development server with file watching"
	@echo "  $(YELLOW)build-dev$(NC)       Build all crates in debug mode"
	@echo "  $(YELLOW)test$(NC)            Run all tests for both crates"
	@echo "  $(YELLOW)fmt$(NC)             Format all Rust code"
	@echo "  $(YELLOW)lint$(NC)            Run clippy linting on all crates"
	@echo "  $(YELLOW)check$(NC)           Quick compilation check for all crates"
	@echo ""
	@echo "$(GREEN)Production Commands:$(NC)"
	@echo "  $(YELLOW)build-prod$(NC)      Build optimized release binaries"
	@echo "  $(YELLOW)docker-build$(NC)    Build production Docker image"
	@echo "  $(YELLOW)docker-run$(NC)      Run containerized application"
	@echo "  $(YELLOW)docker-clean$(NC)    Clean up Docker containers and images"
	@echo ""
	@echo "$(GREEN)Utility Commands:$(NC)"
	@echo "  $(YELLOW)deps$(NC)            Install/update dependencies"
	@echo "  $(YELLOW)env-check$(NC)       Verify required environment variables"
	@echo "  $(YELLOW)db-setup$(NC)        Initialize database schema"
	@echo "  $(YELLOW)clean$(NC)           Clean all build artifacts"
	@echo "  $(YELLOW)install$(NC)         Install required development tools"
	@echo "  $(YELLOW)audit$(NC)           Run security audit on dependencies"
	@echo "  $(YELLOW)health$(NC)          Check application health"
	@echo "  $(YELLOW)init-config$(NC)     Initialize application configuration"
	@echo "  $(YELLOW)info$(NC)            Show project information"
	@echo ""
	@echo "$(GREEN)Workflows:$(NC)"
	@echo "  $(YELLOW)quick$(NC)           Quick development check (compile + test)"
	@echo "  $(YELLOW)all$(NC)             Full build pipeline"
	@echo "  $(YELLOW)ci$(NC)              CI/CD pipeline simulation"

## Development Commands
dev: env-check ## Start development server with hot reload
	@echo "$(GREEN)Starting development server...$(NC)"
	cd $(MAIN_CRATE) && $(CARGO) run $(CARGO_FLAGS)

watch: ## Start development server with file watching (requires cargo-watch)
	@echo "$(GREEN)Starting development server with file watching...$(NC)"
	@if command -v cargo-watch >/dev/null 2>&1; then \
		cd $(MAIN_CRATE) && cargo watch -x run; \
	else \
		echo "$(RED)cargo-watch not found. Install with: cargo install cargo-watch$(NC)"; \
		echo "$(YELLOW)Falling back to regular dev mode...$(NC)"; \
		$(MAKE) dev; \
	fi

build-dev: ## Build all crates in debug mode
	@echo "$(GREEN)Building all crates in debug mode...$(NC)"
	@cd $(TYPES_CRATE) && $(CARGO) build $(CARGO_FLAGS) $(DEV_FLAGS)
	@cd $(MAIN_CRATE) && $(CARGO) build $(CARGO_FLAGS) $(DEV_FLAGS)
	@echo "$(GREEN)Debug build completed successfully!$(NC)"

test: ## Run all tests for both crates
	@echo "$(GREEN)Running tests for all crates...$(NC)"
	@cd $(TYPES_CRATE) && $(CARGO) test $(CARGO_FLAGS)
	@cd $(MAIN_CRATE) && $(CARGO) test $(CARGO_FLAGS)
	@echo "$(GREEN)All tests passed!$(NC)"

fmt: ## Format all Rust code
	@echo "$(GREEN)Formatting code...$(NC)"
	@cd $(TYPES_CRATE) && $(CARGO) fmt
	@cd $(MAIN_CRATE) && $(CARGO) fmt
	@echo "$(GREEN)Code formatting completed!$(NC)"

lint: ## Run clippy linting on all crates
	@echo "$(GREEN)Running clippy lints...$(NC)"
	@cd $(TYPES_CRATE) && $(CARGO) clippy $(CARGO_FLAGS) -- -D warnings
	@cd $(MAIN_CRATE) && $(CARGO) clippy $(CARGO_FLAGS) -- -D warnings
	@echo "$(GREEN)Linting completed successfully!$(NC)"

check: ## Quick compilation check for all crates
	@echo "$(GREEN)Running compilation checks...$(NC)"
	@cd $(TYPES_CRATE) && $(CARGO) check $(CARGO_FLAGS)
	@cd $(MAIN_CRATE) && $(CARGO) check $(CARGO_FLAGS)
	@echo "$(GREEN)Compilation checks passed!$(NC)"

## Production Commands
build-prod: ## Build optimized release binaries
	@echo "$(GREEN)Building release binaries...$(NC)"
	@cd $(TYPES_CRATE) && $(CARGO) build $(CARGO_FLAGS) $(RELEASE_FLAGS)
	@cd $(MAIN_CRATE) && $(CARGO) build $(CARGO_FLAGS) $(RELEASE_FLAGS)
	@echo "$(GREEN)Release build completed successfully!$(NC)"
	@echo "$(BLUE)Binary location: $(MAIN_CRATE)/target/release/$(MAIN_CRATE)$(NC)"

docker-build: ## Build production Docker image
	@echo "$(GREEN)Building Docker image: $(DOCKER_IMAGE):$(DOCKER_TAG)$(NC)"
	$(DOCKER) build -f $(DOCKERFILE) -t $(DOCKER_IMAGE):$(DOCKER_TAG) .
	@echo "$(GREEN)Docker image built successfully!$(NC)"

docker-run: env-check ## Run containerized application
	@echo "$(GREEN)Running Docker container...$(NC)"
	@echo "$(YELLOW)Note: Make sure to set environment variables properly$(NC)"
	$(DOCKER) run -p $(PORT):$(PORT) \
		-e DATABASE_URL="$(DATABASE_URL)" \
		-e DIGITALOCEAN_S3_ACCESS_KEY="$(DIGITALOCEAN_S3_ACCESS_KEY)" \
		-e DIGITALOCEAN_S3_SECRET_KEY="$(DIGITALOCEAN_S3_SECRET_KEY)" \
		-e DIGITALOCEAN_S3_ENDPOINT="$(DIGITALOCEAN_S3_ENDPOINT)" \
		-e DIGITALOCEAN_S3_CLOUD_REGION="$(DIGITALOCEAN_S3_CLOUD_REGION)" \
		-e OPENSCRAPERS_S3_OBJECT_BUCKET="$(OPENSCRAPERS_S3_OBJECT_BUCKET)" \
		-e DEEPINFRA_API_KEY="$(DEEPINFRA_API_KEY)" \
		-e PORT=$(PORT) \
		--name $(PROJECT_NAME)-container \
		$(DOCKER_IMAGE):$(DOCKER_TAG)

docker-clean: ## Clean up Docker containers and images
	@echo "$(GREEN)Cleaning up Docker resources...$(NC)"
	-$(DOCKER) stop $(PROJECT_NAME)-container
	-$(DOCKER) rm $(PROJECT_NAME)-container
	-$(DOCKER) rmi $(DOCKER_IMAGE):$(DOCKER_TAG)
	@echo "$(GREEN)Docker cleanup completed!$(NC)"

## Utility Commands
deps: ## Install/update dependencies
	@echo "$(GREEN)Updating dependencies...$(NC)"
	@cd $(TYPES_CRATE) && $(CARGO) update
	@cd $(MAIN_CRATE) && $(CARGO) update
	@echo "$(GREEN)Dependencies updated successfully!$(NC)"

env-check: ## Verify required environment variables are set
	@echo "$(GREEN)Checking environment variables...$(NC)"
	@missing_vars=""; \
	for var in $(ENV_VARS); do \
		if [ -z "$${!var}" ]; then \
			missing_vars="$$missing_vars $$var"; \
		fi; \
	done; \
	if [ -n "$$missing_vars" ]; then \
		echo "$(RED)Missing required environment variables:$$missing_vars$(NC)"; \
		echo "$(YELLOW)Please set them in your .env file or environment$(NC)"; \
		exit 1; \
	else \
		echo "$(GREEN)All required environment variables are set!$(NC)"; \
	fi

db-setup: ## Initialize database schema
	@echo "$(GREEN)Setting up database schema...$(NC)"
	@echo "$(YELLOW)Starting server to trigger schema creation...$(NC)"
	@echo "$(BLUE)The server will automatically create the database schema on first run$(NC)"
	@echo "$(BLUE)You can also use the API endpoint: POST /task/recreate_dokito_table_schema$(NC)"

clean: ## Clean all build artifacts
	@echo "$(GREEN)Cleaning build artifacts...$(NC)"
	@cd $(TYPES_CRATE) && $(CARGO) clean
	@cd $(MAIN_CRATE) && $(CARGO) clean
	@echo "$(GREEN)Clean completed!$(NC)"

install: ## Install required development tools
	@echo "$(GREEN)Installing development tools...$(NC)"
	@if ! command -v cargo-watch >/dev/null 2>&1; then \
		echo "$(YELLOW)Installing cargo-watch...$(NC)"; \
		$(CARGO) install cargo-watch; \
	fi
	@if ! command -v cargo-audit >/dev/null 2>&1; then \
		echo "$(YELLOW)Installing cargo-audit...$(NC)"; \
		$(CARGO) install cargo-audit; \
	fi
	@echo "$(GREEN)Development tools installed!$(NC)"

audit: ## Run security audit on dependencies
	@echo "$(GREEN)Running security audit...$(NC)"
	@if command -v cargo-audit >/dev/null 2>&1; then \
		cd $(TYPES_CRATE) && $(CARGO) audit; \
		cd $(MAIN_CRATE) && $(CARGO) audit; \
	else \
		echo "$(RED)cargo-audit not found. Install with: make install$(NC)"; \
		exit 1; \
	fi

all: clean deps check lint test build-prod ## Run full build pipeline (clean, deps, check, lint, test, build)
	@echo "$(GREEN)Full build pipeline completed successfully!$(NC)"

# Development workflow shortcuts
quick: check test ## Quick development check (compile + test)
	@echo "$(GREEN)Quick development check completed!$(NC)"

ci: clean deps check lint test build-prod ## CI/CD pipeline simulation
	@echo "$(GREEN)CI/CD pipeline simulation completed!$(NC)"

# Health check endpoints (requires curl)
health: ## Check application health (server must be running)
	@echo "$(GREEN)Checking application health...$(NC)"
	@if curl -f http://localhost:$(PORT)/health >/dev/null 2>&1; then \
		echo "$(GREEN)✓ Application is healthy!$(NC)"; \
	else \
		echo "$(RED)✗ Application health check failed$(NC)"; \
		echo "$(YELLOW)Make sure the server is running with 'make dev'$(NC)"; \
		exit 1; \
	fi

init-config: ## Initialize application configuration (server must be running)
	@echo "$(GREEN)Initializing application configuration...$(NC)"
	@if curl -X POST http://localhost:$(PORT)/task/initialize_config >/dev/null 2>&1; then \
		echo "$(GREEN)✓ Configuration initialized!$(NC)"; \
	else \
		echo "$(RED)✗ Configuration initialization failed$(NC)"; \
		echo "$(YELLOW)Make sure the server is running with 'make dev'$(NC)"; \
		exit 1; \
	fi

# Show project info
info: ## Show project information
	@echo "$(BLUE)Project Information$(NC)"
	@echo "$(BLUE)==================$(NC)"
	@echo "Project Name: $(PROJECT_NAME)"
	@echo "Main Crate: $(MAIN_CRATE)"
	@echo "Types Crate: $(TYPES_CRATE)"
	@echo "Docker Image: $(DOCKER_IMAGE):$(DOCKER_TAG)"
	@echo "Default Port: $(PORT)"
	@echo ""
	@echo "$(GREEN)Crate Information:$(NC)"
	@echo "Main Crate Path: ./$(MAIN_CRATE)/"
	@echo "Types Crate Path: ./$(TYPES_CRATE)/"
	@echo ""
	@if [ -f "$(MAIN_CRATE)/Cargo.toml" ]; then \
		echo "$(GREEN)Main Crate Dependencies:$(NC)"; \
		grep -E "^[a-zA-Z_-]+ =" $(MAIN_CRATE)/Cargo.toml | head -10; \
	fi