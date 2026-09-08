# Security fixes prepared on 2026-09-08

These changes are staged in the configuration repository. Deployment and changes
to existing runtime files require separate approval.

| Area | Repository change |
| --- | --- |
| Minecraft sudo | Replace direct ZFS/mount wildcard rules with `minecraft-storage`, validating every operation and snapshot name. Mount snapshots read-only at a root-owned location. |
| Caddy administration | Use `/run/caddy/admin.sock`, accessible only to Caddy/root; disable persisted API configuration. |
| Static websites | Serve files through a separate `caddy-static` identity with no TLS credentials and no access to Caddy state, agenix secrets, or home directories. The TLS proxy forwards over a restricted Unix socket. |
| Dynmap | Allow only GET/HEAD at `map.brage.info` and `incognito.brage.info`. Guard both backends against unrelated local accounts. Remove the stale `home.brage.info` route. |
| SilverBullet | Guard direct backend access against unrelated local accounts; Caddy continues to enforce Authelia authentication. |
| Prometheus | Require Authelia authentication at `status.brage.info`. |
| Redis | Pass administrator authentication through the environment and the ACL password through stdin. Reset the restricted ACL before setting its exact permissions, and fail on errors. |
| Victron | Reject malformed measurements without panicking, accept only configured device types, bound series to 1,024 and key length to 256 bytes, and restrict metrics HTTP to loopback. Set a 128 MiB memory limit. |
| Victron ingress | Use an atomically replaced nftables IPv4/IPv6 allowlist of AS14593 prefixes, refreshed hourly from RIPE RIS. Retain the last successful list on refresh failure and restore it across boots. Without a valid cache, reject traffic until the first successful update. |
| Ganbot | Apply a repository-owned patch to the pinned package: cap image dimensions at 4,096 per axis, input downloads at 32 MiB/30 seconds, and concurrent conversions at two. Run conversions off the async worker threads; keep their semaphore permits until completion even if clients disconnect. Set a 2 GiB service memory limit. |

Starlink source filtering is temporary network-level filtering, not sender
authentication. Other Starlink customers and attackers able to spoof accepted
source addresses may still forge measurements. Replace it with an authenticated
protocol as planned. The updater tracks origin AS14593, not Starlink transit
customers or separate regional ASNs. The current feed was successfully fetched
and validated during implementation.

The custom nftables tables `local_web_access` and `victron_ingress` intentionally
survive NixOS firewall stop/reload. Removing the guards before replacing them
would briefly expose protected backends. If these protections are later retired,
remove the tables deliberately as part of that migration.

## Before deployment

1. Inspect the existing Minecraft scripts and update their privileged commands:

   ```text
   sudo minecraft-storage status
   sudo minecraft-storage pools
   sudo minecraft-storage snapshots
   sudo minecraft-storage snapshot rpool/minecraft/erisia@NAME
   sudo minecraft-storage rollback rpool/minecraft/erisia@NAME
   sudo minecraft-storage mount rpool/minecraft/erisia@NAME
   sudo minecraft-storage unmount
   ```

   Mounted content is now at `/run/minecraft-snapshot`, not
   `/home/minecraft/snapshot`. `rollback` retains the previous destructive `-r`
   behavior within the validated dataset hierarchy. There are no raw mount or
   ZFS sudo allowances for compatibility.
2. Check that the public `/srv` files are readable by the new `caddy-static`
   account. Files that are readable only by the old Caddy identity may need
   narrowly scoped permission changes. No runtime permissions have been changed.
3. Restart Caddy for the initial admin-socket transition; subsequent explicit
   reloads use the new socket. Confirm static websites, authentication, and the
   new Starlink prefix refresh service before considering deployment complete.
4. In each actual Dynmap configuration, merge `allowwebchat: false` into its
   `InternalClientUpdateComponent` and `allowchat: false` into its
   `SimpleWebChatComponent`. Bind the web server to loopback and review whether
   `allow-symlinks` can be disabled. The deployed version/configuration is not
   managed here. Proxy and local-account enforcement do not depend on this edit,
   but the native settings also remove the nonfunctional chat interface.

No Minecraft instance port changes are included; those have a separate plan.
Magic-reboot's raw shared-secret protocol remains an accepted operational risk:
after use with suspected capture, rotate its secret and reload the module or
reboot **both** receivers. Replacing the agenix file alone does not change a key
already loaded into the kernel.

The additional concern about services sharing `svein` remains: moving
qBittorrent, the seeder, and custom bots to separate service accounts requires
inspection and migration of their existing state and file permissions. This
repository change does not claim that account isolation has been completed.

## Validation completed without deployment

- Minecraft helper and Starlink prefix validation tests pass.
- Victron's five tests, strict Clippy, formatting, and documentation build pass.
- The Ganbot patch applies to the pinned source, compiles, and its oversized-image
  regression test passes. Its pre-existing unused-code warnings remain.
- Caddy adapts the updated proxy configuration; the static-server configuration
  validates. The adapted Dynmap routes reject writes before proxying requests,
  and the status route authenticates before reaching Prometheus.
- Both complete NixOS system builds (`saya` and `tsugumi`) pass, including the
  patched Ganbot package, Victron package, sudoers validation, and service units.
- `checks.x86_64-linux.security-scripts` passes in the Nix build sandbox.
- The firewall VM test passes for IPv4/IPv6 backend access, owner exceptions,
  Victron allowlisted and rejected sources, missing/corrupt prefix caches, and
  firewall reload/stop/start. Packet tests use synthetic addresses inside the VM;
  they do not send traffic to those addresses over the internet.
- The same VM runs the real static-file service with its sandbox: Caddy can
  retrieve public content, an unrelated account cannot access its Unix socket,
  and a public symlink into Caddy's private state cannot be served.

References: [RIPE announced-prefix API](https://stat.ripe.net/docs/data-api/api-endpoints/announced-prefixes),
[Caddy administration](https://caddyserver.com/docs/caddyfile/options#admin),
[Dynmap upstream configuration](https://raw.githubusercontent.com/webbukkit/dynmap/v3.0/spigot/src/main/resources/configuration.txt).
