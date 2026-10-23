#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
COMPOSE = ROOT / "infra/compose/docker-compose.yml"
ENV_BETA = ROOT / "infra/compose/.env.beta"
SETUP_BETA = ROOT / "infra/scripts/setup-beta.sh"

COMPOSE_TOKENS = [
    "--denied-peer-ip=172.16.0.0-172.31.255.255",
    "--no-tcp-relay",
    "--no-multicast-peers",
    "REDIS_URL=redis://:${VALKEY_PASSWORD",
    "REDISCLI_AUTH",
    "CORS_ALLOWED_ORIGINS=",
    "RATE_LIMIT_TRUSTED_PROXIES=",
    "PUBLIC_IP=",
    "AWS_ACCESS_KEY_ID=",
]


def read_text(path: Path, errors: list[str]) -> str:
    if not path.exists():
        errors.append(f"Missing required file: {path.relative_to(ROOT)}")
        return ""
    return path.read_text(encoding="utf-8")


def require_token(content: str, token: str, label: str, errors: list[str]) -> None:
    if token not in content:
        errors.append(f"{label}: missing required token: {token}")


def forbid_pattern(content: str, pattern: str, label: str, errors: list[str]) -> None:
    if re.search(pattern, content, flags=re.MULTILINE | re.DOTALL):
        errors.append(f"{label}: forbidden pattern matched: {pattern}")


def service_block(content: str, name: str) -> str:
    """Text between `  <name>:` and the next top-level service (or section)."""
    match = re.search(
        rf"^  {re.escape(name)}:\n(.*?)(?=^  [A-Za-z0-9_-]+:\n|^[A-Za-z]|\Z)",
        content,
        flags=re.MULTILINE | re.DOTALL,
    )
    return match.group(1) if match else ""


def check_compose(content: str, errors: list[str]) -> None:
    for token in COMPOSE_TOKENS:
        require_token(content, token, "docker-compose.yml", errors)

    forbid_pattern(content, r"RATE_LIMIT_TRUST_PROXY(?!IES)", "docker-compose.yml", errors)

    coturn = service_block(content, "coturn")
    if not coturn:
        errors.append("docker-compose.yml: coturn service block not found")
    else:
        forbid_pattern(coturn, r"voicechat", "docker-compose.yml coturn block", errors)
        require_token(coturn, "- turn", "docker-compose.yml coturn block", errors)


def check_env_beta(content: str, errors: list[str]) -> None:
    forbid_pattern(content, r"RATE_LIMIT_TRUST_PROXY(?!IES)", ".env.beta", errors)


def check_setup_beta(content: str, errors: list[str]) -> None:
    for line in content.splitlines():
        if "VALKEY_PASSWORD=" in line:
            forbid_pattern(line, r"rand -base64", "setup-beta.sh VALKEY_PASSWORD line", errors)


def main() -> int:
    errors: list[str] = []

    compose_text = read_text(COMPOSE, errors)
    env_text = read_text(ENV_BETA, errors)
    setup_text = read_text(SETUP_BETA, errors)

    if compose_text:
        check_compose(compose_text, errors)
    if env_text:
        check_env_beta(env_text, errors)
    if setup_text:
        check_setup_beta(setup_text, errors)

    if errors:
        print("Compose hardening check failed:")
        for err in errors:
            print(f"- {err}")
        return 1

    print("Compose hardening check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
