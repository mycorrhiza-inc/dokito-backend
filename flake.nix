{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nix2container = {
      url = "github:nlewo/nix2container";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, naersk, nix2container }:
    let
      # Support multiple host systems
      hostSystems = [ "x86_64-linux" "aarch64-darwin" "x86_64-darwin" ];
      # Always target x86_64-linux for containers
      targetSystem = "x86_64-linux";

      # Generate packages for each host system
      forAllSystems = nixpkgs.lib.genAttrs hostSystems;

      # Get packages for cross-compilation
      getPkgs = hostSystem:
        if hostSystem == targetSystem then
          # Native compilation
          nixpkgs.legacyPackages.${hostSystem}
        else
          # Cross-compilation
          import nixpkgs {
            system = hostSystem;
            crossSystem = { system = targetSystem; };
          };

      # Build function that works for any system
      mkDokitoBackend = hostSystem:
        let
          pkgs = getPkgs hostSystem;
          naersk' = pkgs.callPackage naersk {};
        in
        naersk'.buildPackage {
          src = ./.;
          name = "dokito_processing_monolith";

          # Build only the main binary, not the whole workspace
          cargoBuildOptions = x: x ++ [ "--package" "dokito_processing_monolith" ];

          # Add any additional build inputs if needed
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          buildInputs = with pkgs; [
            openssl
          ];

          # Set environment variables for OpenSSL
          OPENSSL_NO_VENDOR = 1;
          PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
        };

      # Container function (always built for x86_64-linux)
      mkContainer = hostSystem:
        let
          pkgs = getPkgs hostSystem;
          dokito-backend = mkDokitoBackend hostSystem;
        in
        pkgs.dockerTools.buildImage {
          name = "dokito-backend";
          tag = "latest";

          copyToRoot = pkgs.buildEnv {
            name = "image-root";
            paths = [ dokito-backend pkgs.coreutils pkgs.bash ];
            pathsToLink = [ "/bin" ];
          };

          config = {
            Cmd = [ "${dokito-backend}/bin/dokito_processing_monolith" ];
            Env = [
              "PATH=/bin"
            ];
          };
        };

    in {
      packages = forAllSystems (system:
        let
          dokito-backend = mkDokitoBackend system;
          container = mkContainer system;
        in {
          default = dokito-backend;
          dokito-backend = dokito-backend;
          container = container;
        });

      # Development shell (use native tools for development)
      devShells = forAllSystems (system:
        let
          # Use native packages for development shell
          pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              cargo
              rustc
              rust-analyzer
              pkg-config
            ];

            buildInputs = with pkgs; [
              openssl
            ];

            OPENSSL_NO_VENDOR = 1;
            PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
          };
        });

      # Apps for easy running
      apps = forAllSystems (system:
        let
          pkgs = getPkgs system;
          dokito-backend = mkDokitoBackend system;
        in {
          default = {
            type = "app";
            program = "${dokito-backend}/bin/dokito_processing_monolith";
          };

          build-container = {
            type = "app";
            program = "${pkgs.writeShellScript "build-container" ''
              set -euo pipefail
              echo "Building container for x86_64-linux..."
              if ! nix build .#container; then
                echo "Container build failed!" >&2
                exit 1
              fi
              echo "Container built successfully!"
              echo "Loading into Docker..."
              if ! docker load < result; then
                echo "Failed to load container into Docker!" >&2
                exit 1
              fi
              echo "Container loaded into Docker as dokito-backend:latest"
            ''}";
          };
        });
    };
}