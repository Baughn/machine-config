# Google Antigravity IDE, from jacopone/antigravity-nix (Baughn, Discord 1575533409319653377).
# Per Garibaldi's review (1575525564515618887):
# - the no-FHS IDE: the FHS one is not a sandbox (it mounts /etc/nixos writable) and
#   downloads its own agy binary at runtime; no-FHS uses the Nix-built CLI;
# - useUserProfile = false: otherwise the agent drives Baughn's real browser profile
#   (logged-in sessions) over CDP.
# Closed-source binaries; it runs with Baughn's full user rights either way.
{ pkgs, antigravity-nix, ... }:
{
  environment.systemPackages = [
    (pkgs.callPackage "${antigravity-nix}/pkgs/google-antigravity-ide.nix" {
      useFHS = false;
      useUserProfile = false;
    })
    (pkgs.callPackage "${antigravity-nix}/pkgs/cli.nix" {})
  ];
}
