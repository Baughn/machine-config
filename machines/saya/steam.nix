{ pkgs, ... }:

let
  # proton-unstick — kill a wedged Proton/Steam game *and* its leftover
  # wineserver. A deadlocked game leaves its wineserver (and fsync shared state)
  # alive; Steam's "Stop" doesn't reap it, so every relaunch re-attaches to the
  # poisoned prefix and hangs the same way. Killing the wineserver is what
  # actually breaks the cycle.
  #
  #   proton-unstick            kill ALL running Proton games + their wineservers
  #   proton-unstick <pattern>  only games whose launch cmdline matches <pattern>
  #                             (an AppId like 1594320, or a name like Captain)
  proton-unstick = pkgs.writeShellApplication {
    name = "proton-unstick";
    runtimeInputs = with pkgs; [ procps gnugrep coreutils ];
    text = ''
      pattern="''${1:-}"

      kill_tree() {
        local pid=$1
        local -a children
        mapfile -t children < <(pgrep -P "$pid" 2>/dev/null || true)
        local child
        for child in "''${children[@]}"; do
          kill_tree "$child"
        done
        kill -KILL "$pid" 2>/dev/null || true
      }

      # 1. Steam launch reapers carry "SteamLaunch AppId=<id>" in their cmdline.
      mapfile -t reapers < <(pgrep -f 'SteamLaunch AppId=' || true)
      targets=()
      appids=()
      if (( ''${#reapers[@]} )); then
        for pid in "''${reapers[@]}"; do
          cmd=$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null || true)
          [[ -n "$cmd" ]] || continue
          if [[ -n "$pattern" && "$cmd" != *"$pattern"* ]]; then
            continue
          fi
          targets+=("$pid")
          if [[ "$cmd" =~ AppId=([0-9]+) ]]; then
            appids+=("''${BASH_REMATCH[1]}")
          fi
        done
      fi

      if (( ''${#targets[@]} == 0 )); then
        if [[ -n "$pattern" ]]; then
          echo "proton-unstick: no running Proton game matches '$pattern'." >&2
        else
          echo "proton-unstick: no running Proton games found." >&2
        fi
      fi

      # 2. Kill each matched game's full process subtree.
      if (( ''${#targets[@]} )); then
        for pid in "''${targets[@]}"; do
          echo "proton-unstick: killing game process tree (reaper pid $pid)"
          kill_tree "$pid"
        done
      fi

      # 3. Kill the wineserver(s) — the part Steam's "Stop" misses.
      mapfile -t wineservers < <(pgrep -f 'Steam.*wineserver' || true)
      if (( ''${#wineservers[@]} )); then
        for pid in "''${wineservers[@]}"; do
          if [[ -n "$pattern" ]]; then
            # Targeted mode: only ever touch wineservers belonging to a game we
            # actually matched. If the pattern matched no running game, kill
            # nothing — never fall through to nuking unrelated wineservers.
            (( ''${#appids[@]} > 0 )) || continue
            envdata=$(tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null || true)
            match=0
            for id in "''${appids[@]}"; do
              if grep -qF "compatdata/$id/" <<<"$envdata"; then match=1; break; fi
            done
            (( match )) || continue
          fi
          echo "proton-unstick: killing wineserver pid $pid"
          kill -KILL "$pid" 2>/dev/null || true
        done
      fi

      echo "proton-unstick: done — relaunch from Steam."
    '';
  };
in
{
  programs.steam.enable = true;

  environment.systemPackages = [ proton-unstick ];

  # Steam launches transient app-steam@<id>.scope user units that fail routinely as
  # games exit; without this filter every shell prompt would flag them as failures.
  me.shell.userFailedUnitsExclude = [ "^app-steam[@-]" ];
}
