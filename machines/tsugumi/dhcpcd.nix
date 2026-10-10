{ ... }:

# dhcpcd 10.3.2 frees a control-socket client in control_recvdata() and then uses it again
# (CVE-2026-56117). Any local user can reach it through the 0666 control socket. The
# fix is upstream commit 78ea09e ("control: Avoid hangup in the recvdata path", in 10.5.0),
# vendored here. Drop this once nixpkgs ships >= 10.5.0. Garibaldi's review, 2026-10-10;
# Baughn asked for the patch.
{
  nixpkgs.overlays = [
    (final: prev: {
      dhcpcd = prev.dhcpcd.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ./dhcpcd-control-hangup.patch ];
      });
    })
  ];
}
