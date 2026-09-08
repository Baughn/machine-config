# Minecraft access frontend

Status: initial implementation tested, updated 2026-09-09. Production activation
requires Discord application settings and its encrypted client secret.

## Purpose and agreed behavior

Protect Minecraft on tsugumi from automated scanners, worms, and unsolicited
connections by requiring visitors to authorize their network addresses through
Discord at `minecraft.brage.info`.

- Users must have the Minecraft players role in the configured Discord guild
  at login. Remembered sessions may renew access without contacting Discord.
- Access lasts 14 days. Visiting the website renews access; Minecraft traffic
  does not. Renew only the addresses observed during that visit, not all old
  addresses associated with the account.
- Do not poll for role removal or revoke existing grants based on Discord
  events. Server admins handle removals through Minecraft's whitelist.
- Support IPv4 and IPv6. After login, explain that visitors should use the
  machine and network they play from, and list their allowed IPs and expiries.
- Keep Minecraft authentication and its whitelist. A public IP can be shared
  by multiple people; this gate protects network reachability, not player identity.

## Login and subsequent visits

Use Discord's OAuth authorization-code flow with `identify` and
`guilds.members.read`. Exchange the code on the backend and call
`GET /users/@me/guilds/{guild_id}/member`. Require the configured role ID in
the returned membership. Guild and role IDs are configuration, not names.
No Discord bot or manually copied verification code is needed.

Use a session-bound, single-use OAuth state value and a fixed callback URL.
Use the Discord access token only for the login role check, then discard it and
any returned refresh token. The browser receives only an opaque session cookie
(`Secure`, `HttpOnly`, `SameSite=Lax`, host-only); store its hash server-side.
Exclude credentials and authorization codes from logs.

A browser session has a 90-day absolute lifetime from login, with no sliding
extension. On subsequent authenticated visits, renew observed addresses without
contacting Discord. Once the session expires, require another Discord login and
role check. Role removal does not invalidate a session or an existing IP grant.

Page loads are read-only HTTP GETs. The page initiates renewal using an
authenticated, CSRF-protected POST during a visit. Discord failure prevents new
logins but does not affect remembered sessions. Logout ends the browser session
and invalidates its pending discovery tickets, without removing IP grants.

Bound each account to 16 active addresses and eight remembered browsers. Limit
renewal visits to one per account per 30 seconds and login starts to one per
source address per 10 seconds. Cap grants, sessions, and pending logins at 4096
each, and active rate-limit entries at 8192. Discovery tickets expire after 60 seconds and OAuth login
attempts after 10 minutes. Expired state is pruned during subsequent operations.

## Discovering both address families

One HTTPS connection reveals only one source address. A dual-stack main hostname
does not by itself discover both addresses, even on the same machine.

Use the existing IPv4-only `v4.brage.info` record, managed by the UniFi router,
and provision an IPv6-only `v6.brage.info` AAAA record through
`modules/cloudflare-dyndns.nix`. The latter name is proposed, not yet provisioned.
The authenticated page makes an HTTPS request to each
to authorize the actual source address seen on that connection. Either can fail
without preventing a grant for the other family. Show which families succeeded
and list all unexpired grants belonging to the signed-in account.

The dynamic DNS module supports `additionalHostnames`, each with its own updater
service, timer, and cache. The original `brage.info` instance keeps its name and
state directory. Failed updates retry independently. The new record is DNS-only
(not Cloudflare-proxied); it must have no A record. UniFi retains ownership of
the IPv4 record.

Configure Caddy HTTPS routes and certificates for both discovery hosts, with a
dedicated endpoint such as `/minecraft-access/authorize`. Verify that the IPv4
router forwards HTTPS to tsugumi and that IPv6 HTTPS reaches it directly. Do not
redirect discovery requests to the dual-stack main hostname, which would lose
the selected address family.

Use short-lived, single-use authorization tickets bound to the remembered
session and address family. Send tickets in POST bodies, not URLs. Allow
only the main site's origin and do not share the session cookie across hosts.
Tickets and OAuth tokens must not appear in request-body logs.

Caddy supplies the observed client address over a backend connection accessible
only to Caddy. Never accept a user-specified IP or trust arbitrary client-supplied
forwarding headers. The discovery hosts should connect directly to this server;
any upstream proxy requires an explicit trusted-proxy configuration.

Grant individual IPv4 and IPv6 addresses, not whole IPv6 prefixes. Browser-only
VPNs, privacy proxies, and IPv6 address changes can still cause Minecraft to use
a different address. The page should explain this and suggest revisiting after
a network change; being on the same machine is necessary but not sufficient.

## NixOS configuration

Options live in `machines/tsugumi/minecraft-access.nix`, imported by the machine
Minecraft module. The port configuration is:

```nix
me.minecraft.ports = {
  tcp = [ 25565 25566 25575 ];
  udp = [ 24454 ]; # Simple voice chat
};
```

Both fields are lists of `lib.types.port`, defaulting to empty. This is the
single source for the protected port sets and the corresponding firewall
integration. The values above reflect the current Minecraft file's TCP rules
and voice-chat rule. These values remain machine configuration.
UDP 51820 is WireGuard and stays outside this gate.

An independent nftables input guard runs before the existing iptables firewall.
The base firewall still accepts the configured ports so listed traffic can pass
both layers; the earlier guard drops unlisted sources, including over loopback.
It remains installed even when the base firewall is stopped. The module asserts
that the base firewall is enabled and uses the supported iptables backend.
Other settings include the public hostname, Discord guild and role IDs, client
ID, and an agenix-backed client-secret path. Secret contents must not enter the
Nix store.

## Service, storage, and firewall

Components: Caddy, an unprivileged Python/aiohttp service, SQLite, and a narrowly
scoped privileged Python/aiohttp firewall broker. The web service has no arbitrary
root command execution or unrestricted firewall-management privileges.

Store grants as `(Discord user ID, IP address, absolute expiry)`. Allow multiple
addresses per user and multiple users per address. The effective firewall expiry
for a shared address is the latest unexpired grant for it. Expired grants can be
deleted; retain no indefinite IP history by default.

Use separate timed IPv4 and IPv6 nftables sets, replaced in one atomic batch.
The broker owns the grant database; the web service owns a separate session
database. Private Unix sockets restrict HTTP access to Caddy and broker access
to the web-service account. The broker serializes updates with the reload hook.
Kernel timeouts enforce expiry even when the web service is down. Reconcile
database state with the sets at startup and after firewall reload, restoring
only remaining lifetimes. Renewal must update the kernel timeout as well as the
database. Report success only once the firewall update has succeeded.

The helper accepts only account IDs and validated individual addresses, and
calculates the configured lease expiry itself; the caller cannot select ports,
tables, commands, or rules.
Install the gate before exposing protected services. Startup, reload, and helper
failures must not briefly expose the ports to everyone. Existing grants should
continue until their expiry when the web service is unavailable.

Expiry blocks new connections while letting established sessions finish. The
guard checks established conntrack state before set membership. UDP flows also
retain access while their established conntrack entry remains alive.

## Production setup

The access service defaults to disabled until the following settings are supplied:

```nix
me.minecraft.access = {
  enable = true;
  clientId = "DISCORD_APPLICATION_ID";
  guildId = "DISCORD_GUILD_ID";
  roleId = "MINECRAFT_PLAYERS_ROLE_ID";
  clientSecretFile = config.age.secrets.minecraft-access-discord-secret.path;
};
```

Register `https://minecraft.brage.info/oauth/callback` as the Discord OAuth
redirect URI. Encrypt the client secret with agenix; its recipient entry is in
`secrets/secrets.nix`, and `machines/tsugumi/secrets.nix` conditionally loads
`secrets/minecraft-access-discord-secret.age` when access is enabled. No secret
belongs in the Nix store or chat.

The tsugumi dynamic DNS configuration adds `v6.brage.info` as an independently
updated AAAA record on deployment. UniFi continues to own `v4.brage.info`.
Verify IPv4 forwarding, IPv6 reachability, DNS family separation, and the main
hostname before enabling access. Caddy obtains certificates for all three names;
discovery endpoints must not redirect to the main hostname. Access logging and
request-bearing error logging are disabled for these virtual hosts so callback
codes are not recorded even if the backend is unavailable.

Run the mock-Discord integration tests and NixOS firewall/HTTPS VM test with:

```sh
nix build .#checks.x86_64-linux.minecraft-access .#checks.x86_64-linux.minecraft-access-vm --no-link -L
```

## Validation before deployment

Completed: ten mock-Discord/grant integration tests, a NixOS VM test covering
HTTPS discovery and firewall behavior, Python lint, JavaScript syntax validation,
tsugumi configuration evaluation, and a build of the new DNS updater unit.
The VM covers reboot, stop/reload, expiry with the applications stopped,
established-session survival, corrupt state, and callback error-log privacy.
Live Discord login and public DNS/routing still need verification after setup.

- Authorized role, missing role, non-member, expired session, OAuth replay, and
  Discord failure paths.
- IPv4-only, IPv6-only, and dual-stack clients; discovery failure for one family;
  spoofed forwarding headers and replayed discovery tickets.
- Unlisted addresses cannot reach any configured protected port; listed addresses
  can. Unrelated ports remain unaffected.
- Shortened test leases expire in the kernel while the application is stopped.
  Website renewal extends only observed addresses, with correct shared-IP behavior.
- Reboot and firewall reload preserve remaining lifetimes and never fail open.

## References

- [Discord OAuth2](https://docs.discord.com/developers/topics/oauth2)
- [Discord current-user guild membership](https://docs.discord.com/developers/resources/user#get-current-user-guild-member)
- [ipset timeouts](https://ipset.netfilter.org/ipset.man.html)
- [nftables element timeouts](https://wiki.nftables.org/wiki-nftables/index.php/Element_timeouts)
