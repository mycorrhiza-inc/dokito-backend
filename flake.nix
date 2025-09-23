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
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      naersk' = pkgs.callPackage naersk {};
      nix2containerPkgs = nix2container.packages.${system};

      # Build the Rust application from repo root (handles workspace dependencies)
      dokito-backend = naersk'.buildPackage {
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

      # Create the OCI container using dockerTools instead
      container = pkgs.dockerTools.buildImage {
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
      packages.${system} = {
        default = dokito-backend;
        dokito-backend = dokito-backend;
        container = container;
      };

      # Development shell
      devShells.${system}.default = pkgs.mkShell {
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

      # Apps for easy running
      apps.${system} = {
        default = {
          type = "app";
          program = "${dokito-backend}/bin/dokito_processing_monolith";
        };

        build-container = {
          type = "app";
          program = "${pkgs.writeShellScript "build-container" ''
            echo "Building container..."
            nix build .#container
            echo "Container built successfully!"
            echo "Loading into Docker..."
            docker load < result
            echo "Container loaded into Docker as dokito-backend:latest"
          ''}";
        };
      };
    };
}