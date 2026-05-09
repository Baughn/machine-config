{ config, pkgs, ... }:

{
  age.secrets.restic-password = {
    file = ../../secrets/restic-password.age;
    owner = "svein";
    mode = "0400";
  };

  services.restic.backups.home = {
    user = "svein";
    passwordFile = config.age.secrets.restic-password.path;
    repository = "sftp:svein@tsugumi.local:short-term/backups/saya";
    backupPrepareCommand = "${pkgs.restic}/bin/restic -r sftp:svein@tsugumi.local:short-term/backups/saya unlock";
    paths = [ "/home/svein" ];
    exclude = [
      "/home/*/.cache/*"
      "!/home/*/.cache/huggingface"
      "/home/*/.local/share/baloo/*"
      "/home/*/.local/share/Steam/steamapps"
      "**/shadercache"
      "**/Cache"
      "**/cache"
      "**/_cacache"
      "**/.venv"
      "**/venv"
      "**/ComfyUI/output"
    ];
    extraBackupArgs = [
      "--exclude-caches"
      "--compression=max"
      "--read-concurrency=4"
    ];
    timerConfig.OnCalendar = "*:0/30";
    pruneOpts = [
      "--keep-hourly=36"
      "--keep-daily=7"
      "--keep-weekly=4"
      "--keep-monthly=3"
    ];
  };

  home-manager.users.svein.age.secrets.restic-password = {
    file = ../../secrets/restic-password.age;
    path = "/home/svein/.config/agenix/restic.pw";
  };
}
