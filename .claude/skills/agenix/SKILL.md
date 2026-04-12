---
name: agenix
description: >
  Manage agenix-encrypted secrets in the NixOS repository at ~/cachy-nix.
  Use when: adding a new secret for a NixOS service, changing which hosts
  can decrypt a secret, rekeying secrets after host changes, or wiring
  secrets into NixOS modules/systemd services. Triggers on mentions of
  agenix, age secrets, encrypted secrets, secret management, or adding
  credentials/passwords/keys for NixOS services.
---

# Agenix Secret Management

## Repository layout

```
secrets/
├── secrets.nix    # Which public keys can decrypt each .age file
└── *.age          # Encrypted secret files
```

Secrets are decrypted at NixOS activation time to `/run/agenix/<name>` (tmpfs, never on disk).

## Key architecture

Always read `secrets/secrets.nix` first. It defines:

- Machine host keys (from `/etc/ssh/ssh_host_ed25519_key.pub`)
- User SSH keys (for encrypting during development)
- `allKeys` — convenience list of all keys

Current keys:

```
saya   = ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHkaJd61/WV8hrah8wsuuTVmTBM4JsU1UWJMQyABaHVY
svein  = ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGppkBITukYVejPl3BiRmCDSfdrItzM59XpwwK7W/mXH
```

## Adding a new secret

### 1. Register in secrets.nix

```nix
# Single machine + user:
"my-secret.age".publicKeys = [ svein saya ];
# All machines + users:
"my-secret.age".publicKeys = allKeys;
```

### 2. Encrypt the plaintext

**NEVER use `agenix -e`** (interactive editor, unusable by Claude).

Collect recipient keys from secrets.nix for the entry, then use `age` directly:

```bash
# From a string (use echo -n to avoid trailing newline):
echo -n "secret value" | nix run nixpkgs#age -- --encrypt \
  -r "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHkaJd61/WV8hrah8wsuuTVmTBM4JsU1UWJMQyABaHVY" \
  -r "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGppkBITukYVejPl3BiRmCDSfdrItzM59XpwwK7W/mXH" \
  -o secrets/my-secret.age

# From a file:
nix run nixpkgs#age -- --encrypt \
  -r "ssh-ed25519 ..." -r "ssh-ed25519 ..." \
  -o secrets/my-secret.age /path/to/plaintext
```

Run from repo root (`~/cachy-nix`).

### 3. Declare in NixOS config

For shared secrets, add to `modules/agenix/nixos.nix`:

```nix
age.secrets.my-secret.file = ../../secrets/my-secret.age;
```

For machine-specific secrets, add to the machine's `default.nix`:

```nix
age.secrets.my-secret = {
  file = ../../secrets/my-secret.age;
  owner = "root";
  group = "root";
  mode = "0400";
};
```

### 4. Reference the decrypted path

```nix
config.age.secrets.my-secret.path
# Resolves to /run/agenix/my-secret at runtime
```

### 5. Track before build

New `.age` files must be tracked by jj before Nix can see them in a flake build.

## Reading a secret (verification)

```bash
nix run nixpkgs#age -- --decrypt -i /home/svein/.ssh/id_ed25519 secrets/my-secret.age
```

## Updating an existing secret

Same as creating — overwrite the `.age` file with new encrypted content.
The recipient keys must match what `secrets.nix` specifies.

## Rekeying (after adding/removing keys in secrets.nix)

```bash
cd /home/svein/cachy-nix/secrets && agenix -r
```

Or without agenix on PATH:

```bash
cd /home/svein/cachy-nix/secrets && nix run github:ryantm/agenix -- -r
```

This decrypts each `.age` file with your SSH key and re-encrypts for the updated
recipients. Required when adding a new machine's key or rotating keys.

## Using secrets in systemd services

### DynamicUser services (sandboxed)

Cannot read `/run/agenix/` directly. Use `LoadCredential` + `$CREDENTIALS_DIRECTORY`:

```nix
systemd.services.my-service = {
  script = ''
    export SECRET="$(< "$CREDENTIALS_DIRECTORY/cred-name")"
    exec ${pkg}/bin/my-service
  '';
  serviceConfig = {
    LoadCredential = [ "cred-name:${cfg.secretFile}" ];
    DynamicUser = true;
  };
};
```

Module option pattern for the secret path:

```nix
secretFile = lib.mkOption {
  type = lib.types.path;
  description = "Path to the secret file";
};
# Caller sets: secretFile = config.age.secrets."my-secret".path;
```

### Dedicated user services

Set `owner` in the `age.secrets` declaration, then reference the path directly:

```nix
age.secrets.my-secret = {
  file = ../../secrets/my-secret.age;
  owner = "myservice";
};
# Then in the service:
ExecStart = "${pkg}/bin/my-service --config ${config.age.secrets.my-secret.path}";
```

## Gotchas

- **`echo -n`** — always use `-n` to avoid encrypting a trailing newline
- **Track before build** — `jj` must track new `.age` files so the flake can see them
- **Rekey after host changes** — moving a secret to a different machine requires rekeying
- **Test decryption** before rebuilding:
  `nix run nixpkgs#age -- --decrypt -i ~/.ssh/id_ed25519 secrets/my-secret.age`
- **`--encrypt` / `--decrypt`** — use the long flags; `-e` and `-d` are the short forms
  but the long forms are clearer in scripts
