# Hibiki home-manager module — GPU-rendered music player
#
# Namespace: programs.hibiki
#
# Module factory: receives { hmHelpers } from flake.nix, returns HM module.
{ hmHelpers }:
{
  lib,
  config,
  pkgs,
  ...
}:
with lib;
let
  cfg = config.programs.hibiki;
in
{
  options.programs.hibiki = {
    enable = mkOption {
      type = types.bool;
      default = false;
      description = "Enable the hibiki music player.";
    };
    package = mkOption {
      type = types.package;
      default = pkgs.hibiki;
      description = "The hibiki package to install.";
    };
  };
  config = mkIf cfg.enable {
    home.packages = [ cfg.package ];
  };
}
