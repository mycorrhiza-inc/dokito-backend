# dokito-backend

## How to process documents.

A Rust-based backend service for the Dokito project.

## Quick Start

### Prerequisites

Install Nix and direnv:

```bash
# Install Nix
curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install

# Install direnv (Ubuntu/Debian)
curl -sfL https://direnv.net/install.sh | bash

# Add direnv hook to your shell (.bashrc/.zshrc)
eval "$(direnv hook bash)"  # or zsh
```

## Environment Variables

Copy `example.env` to `.env` and configure:

- `DATABASE_URL` - PostgreSQL connection string
- `DIGITALOCEAN_S3_ACCESS_KEY` - DigitalOcean Spaces access key
- `DIGITALOCEAN_S3_SECRET_KEY` - DigitalOcean Spaces secret key
- `DEEPINFRA_API_KEY` - DeepInfra API key for AI services

See `example.env` for all variables and default values.

# Run

**Production Container:**

```bash
nix run .#build-container
docker run --env-file .env dokito-backend:latest
```

## Development

If you're developing on this project, the direnv setup above automatically provides:

- Rust toolchain (cargo, rustc, rust-analyzer)
- Required system dependencies (OpenSSL, pkg-config)
- Proper environment variables for compilation

Just run `cargo run` or use your IDE after the initial setup.

or alternatively if that isnt working just run:

```bash
devenv shell
```
