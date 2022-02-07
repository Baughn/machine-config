{ pkgs, ... }:

let
  mcu-bootstrap = pkgs.fetchurl {
    url = https://files.mcupdater.com/MCUpdater-latest.jar;
    sha256 = "12zy7lbv7mblhlgpvyr0x576s1ab40xyagq7hg9mmkrznkk62481";
  };
  mcupdater = pkgs.writeShellApplication {
    name = "mcupdater";
    runtimeInputs = [ pkgs.steam-run ];
    text = ''
      steam-run java -jar ${mcu-bootstrap}
  '';
  };
in

{
  environment.systemPackages = [ mcupdater ];
}
