{ config, lib, ... }:

# macOS has mDNS built-in via Bonjour; it cannot be disabled.
{
  options.me.mdns = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "mDNS — always enabled on macOS via Bonjour.";
    };
  };

  config = {
    assertions = [{
      assertion = config.me.mdns.enable;
      message = "me.mdns.enable cannot be false on macOS — Bonjour is always active.";
    }];
  };
}
