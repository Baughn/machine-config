{
  config,
  lib,
  ...
}: {
  # If this is tsugumi, run nix-serve.
  services.nix-serve = lib.mkIf (config.networking.hostName == "tsugumi") {
    enable = true;
    port = 5000;
    secretKeyFile = config.age.secrets."nix-store/private-key".path;
  };

  # Otherwise, add tsugumi as a binary cache.
  nix.settings = lib.mkIf (config.networking.hostName != "tsugumi") {
    substituters = lib.mkBefore ["https://store.brage.info"];
    trusted-public-keys = [
      (builtins.readFile ../secrets/nix-store/public-key)
    ];
  };
}
