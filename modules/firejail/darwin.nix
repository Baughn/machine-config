{ config, lib, ... }:

{
  options.me.firejail = {
    enable = lib.mkEnableOption "Firejail application sandboxing";
  };

  config.assertions = [{
    assertion = !config.me.firejail.enable;
    message = "me.firejail.enable is Linux-only — Firejail doesn't run on macOS.";
  }];
}
