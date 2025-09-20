{ pkgs, lib, config, inputs, ... }:

let
x = 3;
in
{
  # Base configuration shared across all project devenvs
  dotenv.enable = true;
  dotenv.disableHint = true;
  cachix.enable = false;

  # Common packages available to all projects
  packages = with pkgs; [
  ];

  env = {
  };


  enterShell = ''
    workspace-info
  '';

  # See full reference at https://devenv.sh/reference/options/
}
