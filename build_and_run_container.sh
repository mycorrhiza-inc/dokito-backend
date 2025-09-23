sh <(curl --proto '=https' --tlsv1.2 -L https://nixos.org/nix/install) --daemon
nix run .#build-container
docker run --env-file .env dokito-backend:latest
