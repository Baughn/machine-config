{ config, lib, pkgs, ... }:

{
  nixpkgs.config.permittedInsecurePackages = [
    "openssl-1.1.1w" # EOL in Sep, not insecure as such.
  ];

  services.home-assistant = {
    enable = true;
    config = {
      name = "Home";
      unit_system = "metric";
      time_zone = "Europe/Dublin";
      external_url = "https://home.brage.info";
      http = {
        use_x_forwarded_for = true;
        trusted_proxies = [ "127.0.0.1" "::1" ];
      };
    };
    extraComponents = [
      "google_translate"
      "weather"
    ];
    extraPackages = python3Packages: with python3Packages; [
      dateutil
      numpy
    ];
  };
}
