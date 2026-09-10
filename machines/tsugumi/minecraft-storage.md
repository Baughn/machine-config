# Minecraft snapshot storage

`minecraft-storage` is the narrow root helper used by the Minecraft account's
`mount-snapshot.py` and `rollback-to-snapshot.py`. Its commands remain:

```
minecraft-storage status|pools|snapshots|unmount
minecraft-storage snapshot|mount|rollback rpool/minecraft/DATASET@SNAPSHOT
```

`snapshots` merges the SSD history and the replica at
`stash/zrepl/rpool/rpool/minecraft`, retaining the original five tab-separated
columns and logical `rpool/minecraft` names. Identical copies appear once; the
SSD supplies their size statistics. Within each dataset snapshots are ordered
by creation time. Conflicting GUIDs under the same logical name are listed
using the SSD copy with a warning on stderr; `mount` and `rollback` refuse
that name. Missing trees are reported on stderr. Listing does not require sudo.

`mount` prefers the SSD copy and falls back to the HDD. It mounts read-only at
`/run/minecraft-snapshot`, with `nosuid,nodev,noexec`, and holds that snapshot
against pruning until `unmount`. A subsequent privileged invocation cleans up a
stale mount hold after an interrupted mount or reboot.

## Native rollback

Stop the world before rollback. The account's existing wrapper handles its
server check and confirmation; the root helper never executes account-owned
scripts. Stop other writers and leave the world stopped throughout recovery.

Rollback affects one dataset, leaving child datasets such as `dynmap` intact.
Newer snapshots on the live dataset and corresponding replica are discarded.
The helper briefly stops **all zrepl jobs**, since they share one daemon.

- A surviving SSD snapshot uses native ZFS rollback.
- An HDD-only snapshot uses an incremental receive from an older shared
  snapshot when available.
- An HDD-only snapshot older than every SSD snapshot requires a full receive.
  This removes the SSD snapshots, temporarily unmounts the world and its
  mounted children, and restores their original mounts after receiving.
  The stream is nonrecursive and excludes dataset properties. Live local
  properties and child datasets are preserved.

The replica is rewound to the newest shared base that survives the restore.
For a local-only target, this can precede the target; zrepl subsequently sends
forward from that base. Its `rpool` replication cursor and `backup-sink`
last-received hold are rebuilt at the shared GUID. Obsolete partial receive
state on that replica is aborted. Explicit metadata repair is confined to those
two jobs on the affected datasets; native rollback still discards newer
bookmarks alongside newer snapshots. A previously stopped daemon remains stopped.

The helper refuses unrelated holds, blocking clones, conflicting identities,
missing replicas, an existing replica without a surviving common base, and
full receives that would remove older SSD-only history. Full receive currently
requires unencrypted, non-clone datasets and the normal
`/home/minecraft/DATASET` mount layout. Exclusions come from the configured
zrepl sender filesystem filter; excluded datasets keep local rollback behavior.

## Interrupted rollback

The root-owned journal is `/var/lib/minecraft-storage/rollback.json`. While it
exists, zrepl's systemd unit refuses to start, including after reboot. Other
mutating helper commands are blocked. Snapshot listing and pool inspection
remain available.

Keep the world stopped and repeat **the same rollback command**. The helper
validates the recorded GUIDs, completes the remaining restore and metadata
repair, remounts the recorded filesystems, and restores zrepl's original service
state. Child processes retain the operation lock, so a retry waits for an
orphaned send/receive to finish before proceeding.

Do not delete the journal or override the systemd condition merely to restart
backups: that could replicate a partially restored world. If the retry reports
changed/missing recovery snapshots, unexpected holds, or a changed mount
layout, a root administrator must resolve that specific condition first.

The companion `complete.json` records a pending daemon restart after successful
recovery. Subsequent privileged invocations finish that restart automatically.

## Verification

Python tests cover selection, output compatibility, the sudo argument boundary,
and recovery orchestration. The `minecraft-storage-vm` flake check uses real
ZFS pools and zrepl on disposable VM disks to exercise native rollback,
HDD restores, mounts, metadata repair, interruption/reboot recovery, and renewed
replication/pruning. No production rollback is part of testing.
