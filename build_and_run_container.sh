#!/usr/bin/env bash
set -euo pipefail

# Only install Nix if it's not already installed
if ! command -v nix >/dev/null 2>&1; then
  echo "Nix not found, installing..."
  sh <(curl --proto '=https' --tlsv1.2 -L https://nixos.org/nix/install) --daemon
else
  echo "Nix already installed, skipping install."
fi

# Build container; only run docker if build succeeds
if nix run --extra-experimental-features nix-command --extra-experimental-features flakes .#build-container; then
  echo "Container built successfully. Running docker..."
  docker run --env-file .env dokito-backend:latest
else
  echo "Container build failed. Skipping docker run." >&2
  exit 1
fi
