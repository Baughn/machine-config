{ ... }:

{
  # Upstream default is `true` but deprecated; pin to the new default
  # explicitly so the behaviour is stable across NixOS upgrades.
  boot.zfs.forceImportRoot = false;

  # Default TXG timeout is 5s, which causes constant small writes to the
  # SSDs from chatty fsync-less workloads. 5 minutes batches them; anything
  # that fsyncs is still durable.
  boot.extraModprobeConfig = ''
    options zfs zfs_txg_timeout=300
  '';

  # We really don't care.
  networking.hostId = "deafbeef";
}
