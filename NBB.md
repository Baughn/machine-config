● The wrapper is working. Smoke test produced a clean trace: .out.bin starts with oixd\0\0\0\0 (= WORKER_MAGIC_2), meta captured server state, all 4 files written.

  Here's a one-liner you can run later to spot any captured failures (bytes that don't start with oixd):

  ssh svein@tsugumi.local 'for f in /var/log/nbb-trace/*.out.bin; do h=$(head -c 4 "$f" | od -An -tx1 | tr -d " \n"); [ "$h" != "6f697864" ] && { echo "BAD: $f (head=$h)"; head -c 64 "$f" | od -An -c | sed "s/^/    /"; echo "
  meta=${f%.out.bin}.meta"; }; done'

  When the bug next manifests, the matching .meta file will show ps -efH, the wrapper's open fds, and the parent (sshd-session) fds at the exact instant the protocol got polluted — that should pin down whether it's a leaked fd from a sibling
  process, a PAM/session-open print, or the user's shell config doing something.

  A few notes:
  - /var/log/nbb-trace is svein:root 0755, so the user's wrapper can write there. I had to manually chown the existing dir; the tmpfiles rule in nbb-diag.nix is now consistent with that.
  - Traces auto-expire after 7 days via systemd-tmpfiles.
  - I had to fix a pre-existing caddy hash mismatch in machines/tsugumi/caddy.nix to unblock the deploy — separate issue, just FYI.
  - comfyui.service was already broken before my changes; the saya deploy reported it as a failed unit but the switch went through.

  To roll the diagnostic back when you're done: remove the ./nbb-diag.nix import from machines/tsugumi/default.nix, revert the two id_ed25519_nbb_diag references in machines/saya/default.nix, and rebuild both.

