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

  # KWin is under active local development (kwin-bug/); a changed KWin can't
  # be picked up by a live switch.
  me.deploy.rebootPatterns = [ "kwin" ];
}
