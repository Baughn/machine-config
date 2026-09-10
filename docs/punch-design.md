# Punch: Discord-authorized game access

Status: initial implementation tested, 2026-09-10. Replaces the Minecraft-only access
service. No deployment is part of this change.

## Behavior

`https://punch.brage.info/` is the shared access portal. Game websites link there
with an **Enable server access** button. The portal authenticates with Discord,
checks membership of the configured guild, and remembers the account's role IDs
for an absolute 90-day browser session. OAuth access/refresh tokens are discarded
after login; gaining another role requires another login. Role removal is not
polled and does not revoke existing sessions or grants.

Each named group has a label, one or more eligible Discord role IDs, a host,
address families, and TCP/UDP ports. Any matching role enables the group; a user can qualify for multiple
groups. Visiting the portal renews the observed addresses for every eligible
group for 14 days on its host, for supported address families. The page shows eligible games and grants with their game,
address, and expiry. Client requests cannot select arbitrary ports or groups.

Minecraft remains responsible for its player whitelist. Other games remain
responsible for their own player authorization. Public IPs can be shared, so
this service reduces exposure to scanners rather than identifying game clients.
Established TCP connections and UDP conntrack flows may finish after expiry;
only new connections require an unexpired grant.

## Browser flow and API

1. A game website links to `https://punch.brage.info/`. Using a top-level page
   avoids cross-site cookies and keeps Discord credentials out of game websites.
2. `GET /login` starts Discord OAuth (`identify guilds.members.read`). The callback
   is `https://punch.brage.info/oauth/callback`, with single-use, browser-bound
   OAuth state. At least one configured group must match the user's roles.
3. A host-only `Secure`, `HttpOnly`, `SameSite=Lax` cookie identifies the session.
   The server stores its hash and the role snapshot, never a Discord token.
4. `GET /api/status` returns grants, group labels/eligibility, session expiry, and
   a CSRF token. GET requests do not renew grants.
5. The page sends a same-origin, CSRF-protected `POST /api/visit` to issue two
   single-use discovery tickets bound to the session and address family.
6. The browser posts tickets to `/punch/authorize` on `v4.brage.info` and
   `v6.brage.info`. CORS admits only the portal origin; tickets travel in JSON
   bodies and no browser cookie is sent to discovery hosts. Either family can
   succeed independently. The server derives groups from the session's roles.
7. `POST /api/logout` ends the session and invalidates pending tickets, leaving
   existing grants intact.

Use the same computer and network as the game. IPv4/IPv6-only discovery hosts
reveal each reachable family, but browser-only VPNs, privacy proxies, and IPv6
source-address changes can still produce a different address from the game.
Grant individual addresses, not IPv6 prefixes. Revisit after a network change.

`v4.brage.info` is managed by UniFi. `modules/cloudflare-dyndns.nix` maintains
`v6.brage.info` as a DNS-only AAAA record independently of `brage.info`. Caddy
replaces `X-Punch-Client-IP` with its direct peer address and connects to the
frontend over a private Unix socket. Arbitrary forwarded headers are ignored.
The discovery records must reach tsugumi directly, not a CDN proxy.

## NixOS configuration

The shared implementation lives in `modules/punch.nix` and `modules/punch/`. Shared
service settings are in tsugumi's default config; Minecraft declares its group
in `machines/tsugumi/minecraft.nix`:

```nix
me.punch = {
  enable = true;
  hostname = "punch.brage.info";
  clientId = "1547287991494647939";
  guildId = "153634590190206977";
  clientSecretFile = config.age.secrets.minecraft-access-discord-secret.path;
};

me.punch.groups.minecraft = {
  label = "Minecraft";
  roleIds = [ "480078714709737473" ];
  ports.tcp = [ 25565 25566 25575 ];
  ports.udp = [ 24454 ];
};
```

Add games by adding groups. Group names use lowercase letters, digits and
underscores, starting with a letter. Port lists default to empty. Multiple groups
may share a port; membership of either group permits that port. One role may
appear in several groups. A group's `host` defaults to the local hostname; only
local groups affect that host's firewall. `addressFamilies` defaults to `[ 4 6 ]`.

Stationeers runs at `saya.brage.info`, with IPv6-only access on UDP 27015 and
27016. Its role is `480078523487354881`. `lib/stationeers-access.nix` defines the
group once, imported by both `machines/saya/stationeers.nix` and
`machines/tsugumi/punch-remote.nix`. Saya enables the broker with
`frontendEnable = false`; Discord and the public website remain on tsugumi.

Tsugumi sends remote `/grant` and `/list` requests over the existing WireGuard
link to `10.171.0.6:9781`. Saya binds only that private address and permits the
port on `wg1`, whose only peer is tsugumi. Each request also requires a bearer
token loaded from the shared agenix secret `punch-saya-token.age` through systemd
credentials. WireGuard provides transport encryption and peer authentication;
the application token restricts access by other processes using the private
network. Do not expose this plaintext HTTP endpoint on a public interface.
The remote broker accepts only its own configured groups and address families.
On saya it requires and starts after `wireguard-wg1.service`, so the bind address
is configured before the broker starts.

The portal contacts hosts concurrently with a five-second timeout per request.
A failure on saya does not roll back a successful Minecraft renewal. Responses
include per-game errors, and the page reports which games actually renewed.
Broker errors distinguish address limits, global capacity, mismatched groups,
unsupported address families, invalid requests, and firewall failures; the portal
uses fixed user-facing messages for these codes.
IPv4 discovery never grants Stationeers access or reports it as enabled. Status
lists available grants and identifies hosts whose grants could not be fetched.
No service configures router forwarding.

The existing encrypted secret filename is deliberately retained: this is the
same Discord application. Add the new callback URL in the Discord developer
portal before switching. No real secret enters the Nix store.

## Firewall, storage, and migration

The unprivileged `punch.service` stores sessions under `/var/lib/punch`.
`punch-firewall.service` owns `/var/lib/punch-firewall/grants.sqlite` and exposes
only private `/grant` and `/list` operations. Tsugumi uses a Unix socket that
only the frontend account can reach; Caddy cannot. Saya uses the authenticated
WireGuard endpoint described above. Only Caddy and the frontend can reach the HTTP socket.
The broker can update nftables but accepts only configured group names and
individual addresses, never commands, port lists, or caller-selected expiries.

Grants are `(Discord user ID, IP address, group ID, absolute expiry)`. The
firewall uses the latest live expiry for each `(group, address)` across users.
Renewing one group does not extend another group's grant. Removing a group from
configuration removes its grants and rules on reconciliation. Changing ports
within a group applies to its existing grants. Removing an address family
prunes its grants and updates the firewall on reconciliation; established flows
retain the usual conntrack exemption.

Separate IPv4/IPv6 timed sets per group are installed in an atomic nftables
transaction. The input guard tests every group's source/port allowance before
rejecting unlisted traffic to the union of protected ports. Established flows
are permitted. It runs before the existing NixOS iptables firewall; base port
accepts are still required for listed traffic to pass both layers. Unrelated
ports are untouched. The guard persists across firewall stop/reload.

Kernel timeouts expire grants even when both applications are stopped. Reboot
and reload restore only remaining lifetimes. The broker and reload hook serialize
updates using a file lock. Failed kernel updates roll back database changes and
do not report success. A fresh startup installs a closed guard before reading
persistent state; corrupt state must not expose protected ports.

The tsugumi configuration does not import legacy Minecraft grants; users must
visit Punch to obtain new grants. The former firewall table is still retired
atomically so it cannot block Punch-authorized connections.

Use a normal NixOS switch to retire the old services; do not run old and new
grant brokers simultaneously. Keep the old data for rollback, but do not expect
old software to understand new multi-game grants.

Old browser cookies cannot cross from `minecraft.brage.info` to `punch.brage.info`;
users sign in again. The Minecraft hostname now serves the existing static
Minecraft website, also served at `madoka.brage.info`. The website source in
`~/builder/web` contains the portal links and must be built/published through its
normal deployment process.

Limits: per broker, 16 distinct active addresses per account and 4096 grant rows;
centrally, eight remembered sessions per account, 4096 sessions, 4096 pending
OAuth logins, and 8192 rate entries. Renewal visits are limited to one per account per 30 seconds and login
starts to one per source address per 10 seconds. Tickets expire after 60 seconds;
OAuth state after ten minutes. Expired data is pruned on subsequent operations.

Access logs and request-bearing Caddy error logs are suppressed for Punch hosts,
including backend failure during OAuth callbacks. Application errors exclude
credentials and client addresses.

## Validation

Passed: 16 integration tests, a two-host NixOS firewall/HTTPS VM
test, the Minecraft website build, JavaScript syntax validation, Python lint,
production option evaluation, and parsing of the merged production Caddyfile.

```sh
nix build .#checks.x86_64-linux.punch .#checks.x86_64-linux.punch-vm --no-link -L
```

The integration tests use a mock Discord API. They cover login, role snapshots,
single-role isolation, multi-role unions, replay/CSRF/CORS protection, grant
expiry, limits, failed updates, address-family changes, and broker error codes. The VM tests kernel behavior
for IPv4/IPv6, TCP/UDP, overlapping group ports, unrelated ports, HTTPS discovery,
forged headers, socket permissions, restart, expiry, and corrupt state. Remote
coverage checks authentication, host isolation, IPv6-only grants on both
Stationeers ports, and partial failures while the remote broker is stopped.

On first deployment, restart Caddy after its account gains the `punch-http`
group (`sudo systemctl restart caddy`). A configuration reload retains the
running process's old supplementary groups and can leave Punch returning 502
even though a fresh process running as `caddy` can access the socket.

After deployment, verify public DNS, the registered callback, live Discord login,
and reachability from a real game client. Deploy both host configurations to
activate the distributed service. The VM uses test tokens and synthetic Discord
roles; its remote host protects the production Stationeers port numbers.

## References

- [Discord OAuth2](https://docs.discord.com/developers/topics/oauth2)
- [Discord guild membership](https://docs.discord.com/developers/resources/user#get-current-user-guild-member)
- [nftables element timeouts](https://wiki.nftables.org/wiki-nftables/index.php/Element_timeouts)
