{ pkgs, lib, config, inputs, ... }:
{
  # Base configuration shared across all project devenvs
  dotenv.enable = true;
  dotenv.disableHint = true;
  cachix.enable = false;

  # Common packages available to all projects
  languages.rust.enable = true;

  packages = with pkgs; [
    openssl
  ];

  env = {
  };

  # Build the Rust project
  processes = {
    dokito-backend.exec = "cd dokito_processing_monolith && cargo run";
  };

  # Container configurations
  containers = {
    # Production container that runs the dokito backend
    "dokito-backend" = {
      startupCommand = pkgs.writeShellScript "run-dokito" ''
        cd /app/dokito_processing_monolith
        echo "Building dokito backend..."
        cargo build --release
        echo "Starting dokito backend..."
        exec ./target/release/dokito_processing_monolith
      '';
    };

    # Development container that runs the backend in development mode
    "dokito-backend-dev" = {
      startupCommand = pkgs.writeShellScript "run-dokito-dev" ''
        cd /app/dokito_processing_monolith
        echo "Starting dokito backend in development mode..."
        exec cargo run
      '';
    };
  };

  enterShell = ''
    workspace-info
  '';

  # See full reference at https://devenv.sh/reference/options/
}
