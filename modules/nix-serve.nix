{
  config,
  lib,
  ...
}: # Run nix-serve on Tsugumi and Saya.
lib.mkIf (config.networking.hostName == "tsugumi" || config.networking.hostName == "saya") {
  services.nix-serve = {
    enable = true;
    port = 5000;
    secretKeyFile = config.age.secrets."nix-store/private-key".path;
  };

  # And add whichever machine we *aren't* on as a binary cache.
  nix.settings = {
    substituters = lib.mkBefore [(if config.networking.hostName == "tsugumi" then "saya:5000" else "tsugumi:5000")];
  };

  networking.firewall.allowedTCPPorts = [ 5000 ];
}
