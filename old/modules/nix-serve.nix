{ config
, lib
, ...
}: # Run nix-serve on Tsugumi and Saya.
lib.mkIf (config.networking.hostName == "tsugumi" || config.networking.hostName == "saya") {
  services.nix-serve = {
    enable = true;
    port = 5000;
    secretKeyFile = config.age.secrets."nix-store/private-key".path;
    openFirewall = true;
  };

  # And add whichever machine we *aren't* on as a binary cache.
  #nix.settings = {
  #  substituters = lib.mkBefore [(if config.networking.hostName == "tsugumi" then "http://saya:5000" else "http://tsugumi:5000")];
  #  trusted-public-keys = [ (builtins.readFile ../secrets/nix-store/public-key) ];
  #};
}
