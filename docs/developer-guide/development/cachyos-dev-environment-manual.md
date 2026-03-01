# CachyOS Multi-Project Development Environment Manual

## Quickstart (Copy to New Repo)

For every new repository:

```bash
# 1) Add your repo under the standard workspace path
mkdir -p ~/dev/<org> && cd ~/dev/<org>
git clone <repo-url>
cd <repo>

# 2) Use the standard Make targets
make init
make bootstrap
make services-up
make doctor

# 3) Start development
make dev
```

If this target contract is not present yet, copy the template from this manual into the project's `Makefile` and adapt only internal commands.

## Purpose

This manual defines a single, repeatable development setup for CachyOS (Arch-based) that prevents cross-project contamination.

Use this for every repository so onboarding, troubleshooting, and daily commands are consistent.

## Core Principles

- One repository equals one Distrobox container.
- Keep host tooling minimal.
- Run project commands inside the project container.
- Keep environment files and service data scoped to the repository.
- Expose a consistent `Makefile` interface in all projects.

## Host Baseline (CachyOS)

Install the host dependencies once:

```bash
sudo pacman -Syu --needed base-devel git make podman podman-compose distrobox
```

Optional but useful:

```bash
sudo pacman -S --needed docker docker-compose direnv
```

Notes:
- Prefer Podman on CachyOS/Arch.
- Docker is acceptable if you standardize on it across your projects.
- Do not rely on host-level Rust/Bun/Node for project workflows.

## Workspace and Naming Conventions

Use a predictable workspace layout:

```text
~/dev/<org>/<repo>
```

Container naming convention:

```text
dev-<org>-<repo>
```

Examples:
- repo path: `~/dev/detair/canis`
- container: `dev-detair-canis`

## One-Time Project Initialization

From the project root:

```bash
make init
make bootstrap
make services-up
make doctor
```

What this should do:
- `init`: create or refresh the project Distrobox container
- `bootstrap`: install project dependencies inside the container
- `services-up`: start project-local services (DB, cache, storage)
- `doctor`: verify tools, ports, and service health

## Daily Workflow

Standard command flow:

```bash
make dev
```

Common operations:

```bash
make test
make lint
make fmt
make migrate
make services-down
```

## Isolation Rules (Anti-Contamination)

- Never share `.env` files between repositories.
- Never share `.venv`, `node_modules`, `target`, or build output directories.
- Never run migration/build/test commands from host shell.
- Use unique compose project names per repository (`COMPOSE_PROJECT_NAME=<repo>`).
- If multiple repos run simultaneously, use repo-specific port overrides.

## Canonical Makefile Target Contract

Every repository should provide these targets:

- `init`
- `bootstrap`
- `doctor`
- `services-up`
- `services-down`
- `migrate`
- `dev`
- `test`
- `lint`
- `fmt`
- `clean`

Optional but recommended:

- `shell`
- `logs`
- `rebuild`

## Reusable Makefile Starter (Template)

Copy this starter block into a new repo and adapt internal commands only.

```make
.PHONY: init bootstrap doctor services-up services-down migrate dev test lint fmt clean

CONTAINER_NAME ?= dev-$(shell basename $$(dirname $(CURDIR)))-$(shell basename $(CURDIR))
COMPOSE_FILE ?= docker-compose.dev.yml
COMPOSE_PROJECT_NAME ?= $(shell basename $(CURDIR))

init:
	@echo "Create/refresh $(CONTAINER_NAME)"
	@echo "Implement: distrobox create/upgrade logic"

bootstrap:
	@echo "Install project dependencies in container"

doctor:
	@echo "Check required tools, env vars, ports, and services"

services-up:
	COMPOSE_PROJECT_NAME=$(COMPOSE_PROJECT_NAME) docker compose -f $(COMPOSE_FILE) up -d

services-down:
	COMPOSE_PROJECT_NAME=$(COMPOSE_PROJECT_NAME) docker compose -f $(COMPOSE_FILE) down

migrate:
	@echo "Run migrations inside container"

dev:
	@echo "Run primary development command"

test:
	@echo "Run all tests inside container"

lint:
	@echo "Run lints/static analysis inside container"

fmt:
	@echo "Run formatters inside container"

clean:
	@echo "Clean project-local artifacts only"
```

## New Project Onboarding Checklist

- Create repo in `~/dev/<org>/<repo>`.
- Add standard Make targets.
- Add `.env.example` and project-local `.env` policy.
- Add compose file with project-scoped naming.
- Confirm `make init bootstrap services-up doctor` works on fresh machine.
- Confirm `make test lint fmt` works in clean container.

## Troubleshooting and Reset Playbook

Container drift or broken toolchains:

```bash
make rebuild
```

Service collisions:
- Verify `COMPOSE_PROJECT_NAME` is unique.
- Check host ports in compose overrides.

Database mismatch:
- Verify current repo `.env` values.
- Re-run `make migrate` from inside project container.

Hard reset for one project only:

```bash
make services-down
make clean
```

Then rebuild with:

```bash
make init
make bootstrap
make services-up
make doctor
```

## Verification Checklist

Before considering a setup complete:

- `make -n init bootstrap doctor services-up services-down migrate dev test lint fmt`
- `make test`
- `make lint`
- `make fmt`

All commands should execute without requiring global toolchain changes on the host.
