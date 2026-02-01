# Secrets Management

## Encrypting a new secret with agenix

Do NOT use `agenix -e` — it opens an interactive editor. Instead, use `age` directly:

```sh
nix-shell -p age --run "age --encrypt \
  -r 'ssh-ed25519 <HOST_KEY>' \
  -r 'ssh-ed25519 <USER_KEY_1>' \
  -r 'ssh-ed25519 <USER_KEY_2>' \
  -o secrets/<name>.age \
  /path/to/plaintext"
```

The recipient keys (`-r`) must match the `publicKeys` list in `secrets.nix` for that secret.

## Adding a new secret (checklist)

1. Add `"<name>.age".publicKeys = host <machine>;` to `secrets.nix`
2. Add entry to `default.nix` with `file`, `hosts`, and optional `owner`/`mode`
3. Encrypt with `age` as above
4. Reference via `config.age.secrets."<name>".path` in the machine config
