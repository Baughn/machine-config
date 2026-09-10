{ pkgs }:
let
  worlds = [ "local" "incremental" "full" "manual" "blocked" "interrupted" "tokens" "prune" ];
in
pkgs.testers.runNixOSTest {
  name = "minecraft-storage";
  nodes.machine = { pkgs, ... }: {
    imports = [ ../machines/tsugumi/minecraft-storage.nix ];
    virtualisation.emptyDiskImages = [ 4096 4096 ];
    virtualisation.memorySize = 2048;
    boot.supportedFilesystems = [ "zfs" ];
    networking.hostId = "feedbeef";
    users.users.minecraft = { isNormalUser = true; };
    environment.systemPackages = [ pkgs.python3 ];
    services.zrepl = {
      enable = true;
      settings.jobs = [
        {
          name = "backup-sink";
          type = "sink";
          root_fs = "stash/zrepl";
          serve = { type = "local"; listener_name = "backup-sink"; };
          recv.placeholder.encryption = "off";
        }
        {
          name = "rpool";
          type = "push";
          connect = { type = "local"; listener_name = "backup-sink"; client_identity = "rpool"; };
          filesystems = { "rpool/minecraft<" = true; } //
            builtins.listToAttrs (map (name: { name = "rpool/minecraft/${name}/dynmap"; value = false; }) worlds);
          snapshotting.type = "manual";
          conflict_resolution.initial_replication = "all";
          replication.protection.incremental = "guarantee_incremental";
          pruning = {
            keep_sender = [{ type = "last_n"; count = 4; }];
            keep_receiver = [{ type = "last_n"; count = 8; }];
          };
        }
      ];
    };
  };
  testScript = ''
    import json
    import shlex

    start_all()
    machine.wait_for_unit("multi-user.target")
    machine.succeed("systemctl stop zrepl")
    machine.succeed("zpool create rpool /dev/vdb", "zpool create stash /dev/vdc")
    machine.succeed("zfs create -o mountpoint=none rpool/minecraft", "zfs snapshot rpool/minecraft@root", "zfs create -p -o mountpoint=none stash/zrepl")
    helper = machine.succeed("readlink -f $(command -v minecraft-storage)").strip()
    live_root = "rpool/minecraft/"
    backup_root = "stash/zrepl/rpool/" + live_root

    def storage(args, success=True):
        command = "sudo -u minecraft sudo -- " + shlex.quote(helper) + " " + args
        return (machine.succeed if success else machine.fail)(command)

    def sync(world, snap):
        machine.succeed("systemctl start zrepl")
        machine.wait_until_succeeds("zrepl signal wakeup rpool", timeout=10)
        machine.wait_until_succeeds(f"zfs list -H {backup_root}{world}@{snap}", timeout=45)
        # Wait for sender confirmation, not merely completion of zfs receive.
        guid = int(machine.succeed(f"zfs get -Hp -o value guid {live_root}{world}@{snap}").strip())
        machine.wait_until_succeeds(f"zfs list -H -t bookmark {live_root}{world}#zrepl_CURSOR_G_{guid:016x}_J_rpool", timeout=45)
        ready = "import json,sys; j=json.load(sys.stdin)['Jobs']['rpool']['push']; assert all(j[k]['State']=='Done' for k in ('PruningSender','PruningReceiver'))"
        machine.wait_until_succeeds("zrepl status --mode raw | python3 -c " + shlex.quote(ready), timeout=45)
        machine.succeed("systemctl stop zrepl")

    def seed(world):
        live = live_root + world
        path = "/home/minecraft/" + world
        machine.succeed(f"mkdir -p {path}", f"zfs create -o mountpoint=legacy -o compression=zstd {live}")
        machine.succeed(f"mount -t zfs {live} {path}", f"mkdir {path}/dynmap")
        machine.succeed(f"zfs create -o mountpoint=legacy {live}/dynmap", f"mount -t zfs {live}/dynmap {path}/dynmap")
        machine.succeed(f"echo child > {path}/dynmap/keep")
        # Production worlds carry cursor bookmarks from a retired job that are
        # older than every replica snapshot. They must survive a full receive.
        machine.succeed(f"zfs snapshot {live}@genesis")
        guid = int(machine.succeed(f"zfs get -Hp -o value guid {live}@genesis").strip())
        machine.succeed(f"zfs bookmark {live}@genesis {live}#zrepl_CURSOR_G_{guid:016x}_J_retired-job",
                        f"zfs destroy {live}@genesis", "sleep 1")
        for snap in ("old", "middle", "new"):
            machine.succeed(f"echo {snap} > {path}/data", f"zfs snapshot {live}@{snap}", "sleep 1")
        sync(world, "new")
        return live, backup_root + world, path

    def assert_restored(world, snap):
        live, backup, path = live_root + world, backup_root + world, "/home/minecraft/" + world
        assert machine.succeed(f"cat {path}/data").strip() == snap
        assert machine.succeed(f"cat {path}/dynmap/keep").strip() == "child"
        assert machine.succeed(f"zfs get -H -o value mountpoint {live}").strip() == "legacy"
        assert machine.succeed(f"zfs get -H -o value compression {live}").strip() == "zstd"
        machine.fail(f"zfs list {live}@new", f"zfs list {backup}@new")
        machine.succeed(f"zfs list -H -t bookmark -o name -d1 {live} | grep -q _J_retired-job")
        machine.succeed("test ! -e /var/lib/minecraft-storage/rollback.json")
        machine.succeed(f"echo after > {path}/data", f"zfs snapshot {live}@after")
        sync(world, "after")
        assert machine.succeed(f"zfs get -Hp -o value guid {live}@after").strip() == machine.succeed(f"zfs get -Hp -o value guid {backup}@after").strip()

    with subtest("merged history and read-only HDD mounting"):
        live, backup, path = seed("full")
        machine.succeed(f"zfs destroy {live}@old", f"zfs destroy {live}@middle")
        output = machine.succeed("sudo -u minecraft " + helper + " snapshots")
        names = [line.split("\t")[0] for line in output.splitlines()]
        assert names.count(live + "@old") == 1
        assert names.count(live + "@new") == 1
        assert not any(name.startswith("stash/") for name in names)
        storage("mount " + live + "@old")
        assert machine.succeed("cat /run/minecraft-snapshot/data").strip() == "old"
        machine.fail("touch /run/minecraft-snapshot/write")
        machine.succeed(f"zfs holds -H {backup}@old | grep minecraft-storage:mount")
        storage("mount " + live + "@old", success=False)
        storage("unmount")
        machine.fail(f"zfs holds -H {backup}@old | grep minecraft-storage:mount")

    with subtest("full HDD restore preserves mounted child datasets"):
        storage("rollback " + live + "@old")
        assert_restored("full", "old")

    with subtest("SSD rollback repairs an active zrepl job"):
        live, backup, path = seed("local")
        old_guid = int(machine.succeed(f"zfs get -Hp -o value guid {live}@old").strip())
        foreign_bookmark = f"{live}#zrepl_CURSOR_G_{old_guid:016x}_J_other-job"
        machine.succeed(f"zfs bookmark {live}@old {foreign_bookmark}", f"zfs hold other-job {backup}@old")
        machine.succeed("systemctl start zrepl")
        storage("rollback " + live + "@middle")
        machine.succeed("systemctl is-active zrepl")
        machine.succeed("systemctl stop zrepl")
        machine.succeed(f"zfs list {backup}@old")
        machine.succeed(f"zfs list -t bookmark {foreign_bookmark}", f"zfs holds -H {backup}@old | grep other-job")
        assert_restored("local", "middle")

    with subtest("HDD incremental restore"):
        live, backup, path = seed("incremental")
        machine.succeed(f"zfs destroy {live}@middle")
        storage("rollback " + live + "@middle")
        machine.succeed(f"zfs list {live}@old", f"zfs list {backup}@old")
        assert_restored("incremental", "middle")

    with subtest("SSD-only manual snapshot"):
        live, backup, path = seed("manual")
        machine.succeed(f"echo manual > {path}/data", f"zfs snapshot {live}@manual")
        machine.succeed(f"echo dirty > {path}/data")
        storage("rollback " + live + "@manual")
        assert machine.succeed(f"cat {path}/data").strip() == "manual"
        sync("manual", "manual")

    with subtest("foreign holds and clones fail without changing history"):
        live, backup, path = seed("blocked")
        machine.succeed(f"zfs hold foreign {backup}@new", "systemctl start zrepl")
        storage("rollback " + live + "@old", success=False)
        machine.succeed("systemctl is-active zrepl", f"zfs list {live}@new", f"zfs list {backup}@new")
        machine.succeed("systemctl stop zrepl", f"zfs release foreign {backup}@new")
        machine.succeed(f"zfs clone -o mountpoint=none {live}@new rpool/clone")
        storage("rollback " + live + "@old", success=False)
        machine.succeed(f"zfs list {live}@new", "zfs destroy rpool/clone")
        storage("rollback " + live + "/dynmap@missing", success=False)
        machine.succeed(f"zfs snapshot {live}/dynmap@one", f"echo dirty > {path}/dynmap/keep")
        storage("rollback " + live + "/dynmap@one")
        assert machine.succeed(f"cat {path}/dynmap/keep").strip() == "child"

    with subtest("sudo boundary"):
        storage("rollback rpool/root@nope", success=False)
        storage("mount " + backup + "@old", success=False)
        storage("status -c /tmp/evil", success=False)
        storage("snapshot " + live + "@manual-safe")
        machine.succeed(f"zfs list {live}@manual-safe")

    with subtest("missing replicas and conflicting snapshot identities"):
        machine.succeed(f"zfs rename {backup} {backup}-offline")
        storage("rollback " + live + "@old", success=False)
        machine.succeed(f"zfs list {live}@new", f"zfs rename {backup}-offline {backup}")
        machine.succeed(f"zfs destroy {live}@old", f"zfs snapshot {live}@old")
        storage("mount " + live + "@old", success=False)
        storage("rollback " + live + "@old", success=False)
        machine.succeed(f"zfs list {live}@new", f"zfs destroy {live}@old")

    with subtest("obsolete partial receive is aborted on the affected replica"):
        live, backup, path = seed("tokens")
        machine.succeed(f"dd if=/dev/urandom of={path}/blob bs=1M count=16 status=none", f"zfs snapshot {live}@large")
        machine.fail("bash -o pipefail -c " + shlex.quote(
            f"zfs send -i {live}@new {live}@large | head -c 1048576 | zfs receive -s -u {backup}"
        ))
        assert machine.succeed(f"zfs get -H -o value receive_resume_token {backup}").strip() != "-"
        storage("rollback " + live + "@old")
        assert machine.succeed(f"zfs get -H -o value receive_resume_token {backup}").strip() == "-"
        assert_restored("tokens", "old")

    with subtest("interrupted full receive survives reboot and resumes"):
        live, backup, path = seed("interrupted")
        machine.succeed(f"zfs destroy {live}@old", f"zfs destroy {live}@middle")
        # Fault injection is confined to a root-owned copy inside this VM.
        # Kill the Python parent when it starts the receive, after SSD snapshots
        # have been removed, while retaining the real implementation otherwise.
        zfs = machine.succeed("readlink -f $(command -v zfs)").strip()
        shim = "#!/bin/sh\nif [ \"$1\" = receive ]; then kill -KILL \"$PPID\"; exit 42; fi\nexec " + zfs + " \"$@\"\n"
        machine.succeed("python3 -c " + shlex.quote(
            "from pathlib import Path; import re; "
            f"p=Path('/run/fail-zfs'); p.write_text({shim!r}); p.chmod(0o700); "
            f"s=Path({helper!r}).read_text(); "
            "s=re.sub(r'/nix/store/[^\\s\\\"\\\']+/bin/zfs', '/run/fail-zfs', s); "
            "p=Path('/run/fail-storage'); p.write_text(s); p.chmod(0o700)"
        ))
        machine.succeed("systemctl start zrepl")
        machine.fail("/run/fail-storage rollback " + live + "@old")
        journal = json.loads(machine.succeed("cat /var/lib/minecraft-storage/rollback.json"))
        assert journal["phase"] == "restore" and journal["was_active"]
        machine.fail("systemctl is-active zrepl")
        machine.succeed(f"zfs list {backup}@old")
        machine.crash()
        machine.start()
        machine.wait_for_unit("multi-user.target")
        machine.succeed("zpool list rpool || zpool import rpool", "zpool list stash || zpool import stash")
        machine.succeed("systemctl start zrepl")
        machine.fail("systemctl is-active zrepl")
        storage("snapshot " + live + "@blocked", success=False)
        storage("rollback " + live + "@middle", success=False)
        # A role-account symlink must not redirect root's remount into /etc.
        machine.succeed(f"mv {path} {path}-underlying", f"ln -s /etc {path}")
        storage("rollback " + live + "@old", success=False)
        machine.fail("systemctl is-active zrepl")
        assert live not in machine.succeed("findmnt -rn -T /etc -o SOURCE")
        machine.succeed(f"rm {path}", f"mv {path}-underlying {path}")
        storage("rollback " + live + "@old")
        machine.succeed("systemctl is-active zrepl", "systemctl stop zrepl")
        assert_restored("interrupted", "old")

    with subtest("replication and pruning continue after repair"):
        live, backup, path = seed("prune")
        storage("rollback " + live + "@old")
        for index in range(10):
            machine.succeed(f"zfs snapshot {live}@later{index}")
        sync("prune", "later9")
        machine.fail(f"zfs list {live}@old", f"zfs list {backup}@old")
  '';
}
