{ lib, ... }:

{
  options.me.firejail = {
    enable = lib.mkEnableOption "Firejail application sandboxing";
  };

  # Firejail is Linux-only; no-op on macOS.
}
