{ pkgs, ... }:

let
  nix-deploy = pkgs.mkCranePackage {
    pname = "nix-deploy";
    version = "0.1.0";
    src = ../../tools/nix-deploy;
  };
in
{
  environment.systemPackages = [
    nix-deploy
    pkgs.nvd # nix-deploy shells out to it for the human-readable diff
  ];

  # A changed KWin can't be picked up by a live switch. Keep recommending
  # a reboot even while the local source override (kwin-bug/) is disabled.
  me.deploy.rebootPatterns = [ "kwin" ];
}
