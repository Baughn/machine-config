# Starting points for saya-client

Written by saya on 2026-10-03 from what the other agents knew then. Treat it as hints to
check, not facts. Your own notes belong in `../notes/` and `../tools/`. This directory is
read-only and replaced on each deploy.

## The pack (from tsugumi-minecraft, msg 1555926134124707882)

- The current pack, **E36**, runs on **Cleanroom 0.6.12-alpha**: Minecraft 1.12.2, compatible
  with Forge mods, and it needs **Java 21+** (`java` on your PATH is 21). Give it an 8 GB+ heap.
- **Live pack:** an MCUpdater manifest at `https://madoka.brage.info/pack/ServerPack.xml`
  (server id `e36`). Players launch it with Prism Launcher: Prism supplies Cleanroom, and
  MCUpdater's `instances/e36` directory serves as the Prism instance's `.minecraft`.
- **Unreleased builds (RCs):** the builder flake, `github:Erisia/builder`, has the client
  side as Nix outputs:
  - `nix build github:Erisia/builder/<rev>#e36-clientModsDir`: the client mod jars;
  - `nix build github:Erisia/builder/<rev>#e36-clientConfigDir`: config, scripts, resources,
    patchouli_books, and so on;
  - the Cleanroom installer URL is in the builder's `launcher-lock.json`.
  Ask tsugumi-minecraft for the branch or rev under test. If saya can't build it,
  tsugumi-minecraft can build it on tsugumi and hand it over.
- `autoConnect=false`: the pack sets no server address. Connect to `10.171.0.1:25666`.
- No mod is known to refuse offline accounts.

## Launching headless (untested ideas)

- **Display:** `cage` (wlroots kiosk) or `gamescope --backend headless`, both on your PATH,
  give you a Wayland compositor with no monitor. Xwayland gives LWJGL2 its X11. With
  NVIDIA, wlroots may need `WLR_RENDERER=gles2` or `vulkan`. Check what actually renders on
  the GPU (`glxinfo -B` from mesa-demos under the compositor). Fallback: `Xvfb` with Mesa
  llvmpipe (CPU rendering), which is slow but fine for still frames.
- **Sound:** `ALSOFT_DRIVERS=null` keeps OpenAL quiet.
- **Account:** an offline launch takes `--username <Name> --uuid <any> --accessToken 0
  --userType legacy`. The lab server must be in offline mode.
- **Launch without a launcher:** Prism and MCUpdater are GUI tools. In the end, Java gets
  started with a classpath, the Cleanroom main class and game arguments. Work the command
  out once (from Prism's instance files, or from Cleanroom's version JSON) and script it.
- **Screenshots:** `grim` takes them under a wlroots compositor. The game's own screenshot
  (F2, or from your mod via `ScreenShotHelper`) also works. Images you read are capped at
  2048x2048.
- **Input fallback:** `xdotool` against the Xwayland display.
- **Native libraries:** saya has nix-ld with the libraries Minecraft and LWJGL load (OpenAL,
  GL, X11, udev). Downloaded natives and JDKs usually just work. If one doesn't, ask saya
  to add the library.

## Writing a client mod for 1.12.2 / Cleanroom

- Cleanroom has mod templates (look for CleanroomMC's template or "ForgeDevEnv" on GitHub)
  with modern Gradle and Java 21. Classic ForgeGradle 2.3 (Java 8) templates work for plain
  Forge 1.12.2 mods as well.
- Useful client internals (MCP names): `Minecraft.getMinecraft()`, `mc.currentScreen`,
  `mc.player`, `mc.world`, `GuiScreen.buttonList`, `GuiContainer.inventorySlots`,
  `GuiContainer.getSlotUnderMouse()`, `ItemStack.getTooltip(player, ITooltipFlag)`,
  `ScreenShotHelper`, `ClientCommandHandler.instance.registerCommand`, `mc.displayGuiScreen`,
  `KeyBinding`. Run game work on the client thread with `mc.addScheduledTask`; your socket
  server runs on its own thread.
- Recipe viewers (JEI/HEI): a `@JEIPlugin` gets the `IJeiRuntime` (`getIngredientFilter()`,
  `getRecipeRegistry()`), so you can ask "is this ingredient hidden?" or "what recipes make
  Y?" directly.
- `latest.log` and `crash-reports/` in the game directory are your first stop when it
  doesn't start.

## Asking for help

- Lab servers and clones: tsugumi-lab.
- The pack, its builds, the server side: tsugumi-minecraft.
- Missing packages, permissions, ports, anything in the NixOS config: saya.
