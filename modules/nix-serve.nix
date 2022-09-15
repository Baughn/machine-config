{ config, lib, ... }:
{
  # If this is tsugumi, run nix-serve.
  services.nix-serve = lib.mkIf (config.networking.hostName == "tsugumi") {
    enable = true;
    port = 5000;
    secretKeyFile = config.age.secrets."nix-store/private-key".path;
  };

  # Otherwise, add tsugumi as a binary cache.
  nix.binaryCaches = lib.mkIf (config.networking.hostName != "tsugumi") (lib.mkFront [
    "http://tsugumi:5000"
  ]);
  nix.settings.trusted-public-keys = lib.mkIf (config.networking.hostName != "tsugumi") [
    (builtins.readFile ../secrets/nix-store/public-key)
  ];
}
