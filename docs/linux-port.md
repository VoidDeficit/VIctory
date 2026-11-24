# Linux port

Goal set 2026-11-24. Nothing below is implemented yet; this file is the
working plan and checklist.

**Goal.** VIctory runs Grand Theft Auto VI (Xbox Series X|S) on Linux from the
files the user supplies, with the same behaviour the Windows build has today:
boot, menus, saves, input, audio, and the world, at usable frame rates, without
resetting the display driver.

**Primary route.** Run the existing Windows build under Wine/Proton with
vkd3d-proton providing D3D12. The title is Windows x86-64 machine code and the
loader deliberately uses the Windows loader for mapping, relocations, TLS and
unwind registration (`crates/victory-loader/src/lib.rs`, steps 1-2), so a
Wine-free process is a separate, much larger project (section 7), not a
prerequisite for playing on Linux.

## 1. Definition of done

Track A (product goal) is done when all of these hold on Linux:

- [ ] `victory run <exe> --execute` boots to the start screen at 30 fps.
- [ ] Intro video plays with sound.
- [ ] Profile and saves survive a kill at any step.
- [ ] Loading into the world completes in about the same time as on Windows.
- [ ] The world renders the same content as on Windows (first mission intro).
- [ ] No display-driver reset during a session.
- [ ] XInput pad or keyboard drives the title.
- [ ] A crash writes `crashes.log` naming `module+rva` and the stack.
- [ ] One command starts everything; the prefix is set up automatically.

Non-goals for the first milestone: online, HDR, resolution scaling, reverb,
trigger rumble (the Windows build lacks these too, see README).

## 2. What already runs on Linux (measured 2026-11-24)

| Thing | State |
|-------|-------|
| `cargo xwin build --target x86_64-pc-windows-msvc` | works; debug and release `victory.exe` |
| `cargo xwin clippy ... -- -D warnings` | clean (CI parity) |
| `victory --help`, `config`, `inspect` under Wine 11.22 | works |
| `inspect` on `game_scarlett_master.exe` | works (PE32+, 0xb06d000 bytes, entry 0x56a1ec8) |
| native tests: `victory-core`, `victory-formats`, `victory-pipeline`, `victory-shader` | pass |
| native build of the whole workspace | fails: `victory-gpu`, `victory-hle`, `victory-loader` are `cfg(windows)` |
| tests that need a real D3D12 SM 6.6 device (15 in `victory-shader`) | cannot run under Wine |
| title under Wine + vkd3d-proton + DXVK's dxgi | boots to the account picker, 30 fps |
| title into the menu | crashes: vkd3d-proton emits invalid SPIR-V, RADV segfaults (section 4) |

Machine: Radeon RX 9070 XT (RDNA4), RADV Vulkan 1.4.363, `meshShader = true`,
`shaderInt64 = true`, `descriptorIndexing = true`, `maxMeshOutputVertices = 256`.
GE-Proton 11-7 is installed and ships vkd3d-proton
(`files/lib/wine/vkd3d-proton/x86_64-windows/{d3d12,d3d12core}.dll`).

## 3. Why two tracks

The Windows-only surface is small but deep:

| Crate | Windows coupling |
|-------|------------------|
| `victory-gpu` | calls D3D12/DXGI directly (21 files import `windows`); no backend trait |
| `victory-hle` | implements console APIs on top of Win32/WASAPI/XInput/COM |
| `victory-loader` | uses the Windows loader for the image, plus SEH, TLS and threads |
| `victory-shader` | platform-neutral; emits DXIL bitcode itself, DXC only for tests |
| `victory-pe`, `-formats`, `-pipeline`, `-core` | platform-neutral |

Xbox executables are Windows x86-64, so whatever hosts them must provide the
Windows ABI, TEB, SEH and PE loading. Wine already does that; vkd3d-proton
already translates D3D12 to Vulkan. Writing both again inside VIctory is the
work of section 7 and only worth starting if Wine blocks us.

## 4. Phase 0 — Proton runtime (days) — **done, one blocker**

Set up a dedicated prefix with vkd3d-proton and boot the (unmodified) Windows
build. No source changes in this phase.

- [x] Create prefix: `WINEPREFIX=~/.local/share/victory/prefix wineboot -i`.
- [x] Install vkd3d-proton 64-bit: copy `d3d12.dll` and `d3d12core.dll` from
      GE-Proton (`files/lib/wine/vkd3d-proton/x86_64-windows/`) into
      `drive_c/windows/system32/`, then force them with
      `WINEDLLOVERRIDES="d3d12,d3d12core=n,b"`.
- [x] Install DXVK's `dxgi.dll`, `d3d11.dll` and `d3d10core.dll` beside them
      (`files/lib/wine/dxvk/x86_64-windows/`, override `native`). Wine's own
      dxgi sends `CreateSwapChainForHwnd` into wined3d and crashes on the first
      present; DXVK's dxgi, which Proton uses, presents fine.
- [x] Confirm the device: SM 6.6, mesh shader tier 1, 64-bit atomics on
      descriptor-heap resources, wave lanes 32-64, resource binding tier 3,
      DXR 1.1 (vkd3d-proton 3.1.0 on RADV 26.3.1).
- [x] Boot: the title reaches the account picker at 30 fps, presents, and
      advances on its own after about half a minute (or on Enter in a
      diagnostics build, `VICTORY_KEYS=40:Enter`).
- [x] One command: `scripts/linux/run-victory.sh` (prefix, DLLs, game folder
      from `victory.toml`, launch).

**Blocker: the menu transition kills the process (exit 3).** When the title
creates the mesh pipeline `d4111a6e6ea49d91` + pixel `c0a7fe77b1862e10`,
vkd3d-proton emits SPIR-V for the fragment shader that is invalid:

    block <ID> '2531' exits the loop headed by <ID> '2449',
    but not via a structured exit

RADV does not reject it and segfaults in `radv_shader_spirv_to_nir`
(`spirv_to_nir`). Reproduced with vkd3d-proton 3.0.0 and 3.1.0 and with Mesa
26.3.1 (CachyOS and Arch builds) and 25.3.5, so it is not a version
regression. The same DXIL runs on Windows D3D12: either our emitter produces
control flow vkd3d-shader mistranslates, or vkd3d-shader's structurizer is
wrong for it. The schema, the dumped DXIL/SPIR-V and the validation output are
in `~/.local/share/victory/diagnostics/2026-11-24-spirv/`.

Tried and ineffective: `VKD3D_CONFIG=nodxr`, `VKD3D_DISABLE_EXTENSIONS` for
mesh shaders / descriptor buffers / ray query (the crash moves to the next
pipeline), the Vulkan validation layer (dies during device creation), older
Mesa from `/tmp` ICDs. Not yet tried: AMDVLK, RADV from git, a vkd3d-shader
fix.

`VICTORY_SKIP_PIPELINES` now also stops the host pipeline from being built
(previously it only dropped the draws), which gets the menu past the first
crash; the title then dies on the next new pipeline (vertex `089d6271be6650ef`,
pixel `fa455c8a3c32577b`), so this is a diagnostic tool, not a workaround.

Expected trouble that has not appeared yet: device creation and feature level,
swapchain/presentation (solved by DXVK), 64-lane waves, 64-bit atomics,
console texture tilings. Observed but not fatal: descriptor heaps larger than
the host's 1,000,000-descriptor cap (capped and logged), `DSV format is
UNKNOWN` warnings, `buffer view not on an element of its host copy`.

`VICTORY_D3D12_WARP=1` has no meaning under Wine (WARP is Microsoft's software
adapter), so the safe-fallback advice from the README does not apply; a driver
hang must be handled by restarting the compositor, not by falling back.

## 5. Phase 1 — native command line (weeks)

Everything except `run` is an offline tool and does not need Windows. Making it
native removes Wine from asset work: `census`, `shader-info`, `translate` and
`lib-exports` can run over the 209 GiB tree at native speed.

- [ ] Split config defaults: `%LOCALAPPDATA%` → `$XDG_DATA_HOME` when not on
      Windows (`victory-core::config`).
- [ ] Keep `victory`, `victory-pe`, `victory-formats`, `victory-pipeline`,
      `victory-shader` free of `cfg(windows)` deps so `apps/victory` builds
      natively for the offline subcommands (either `cfg` gates per command or a
      second binary target).
- [ ] Fix the PE tests that parse the test binary itself: they read ELF on Linux
      (`crates/victory-pe/src/image.rs`), so give them bytes instead of
      `current_exe`.
- [ ] Make the asset-policy guard accept both path flavours (it already runs
      natively via `cargo xtask`).

Acceptance: on Linux, no Wine, `victory inspect <game exe> --imports` and
`victory census <game dir>` produce the same output as the Windows build.

## 6. Phase 2 — host abstraction (months)

Only needed for a native build. The shape:

- [ ] `HostGpu` trait behind `victory-gpu`: device, queue, swapchain, resources,
      descriptors, pipelines, queries. The D3D12 implementation stays as-is;
      Vulkan comes second.
- [ ] Root signatures → `VkPipelineLayout`, descriptor heaps → descriptor
      buffers/sets, resource states → barriers, indirect arguments → `vkCmd*Indirect`.
- [ ] Shaders: `victory-shader` IR gains a SPIR-V emitter, or its DXIL output is
      fed to `vkd3d-shader`'s DXIL frontend if that covers SM 6.6. Start with
      menus (no mesh shaders), then world (mesh + indirect).
- [ ] `HostAudio` (PipeWire or ALSA) replacing WASAPI; `HostInput` (evdev or
      SDL3) replacing XInput; `HostFiles` for paths, case-insensitivity and
      overlapped reads.
- [ ] Presentation on Wayland/X11.

## 7. Phase 3 — Wine-free hosting (research, gated)

Blockers, in the order they must be solved:

1. PE mapping with relocations, section flags and image identity for
   `module+rva` reporting.
2. Windows x86-64 ABI calls are reachable from Rust (`extern "win64"`), but the
   title also calls back into us through function pointers and expects TEB/PEB
   at `gs:[0x30]`; a per-thread fake TEB via `arch_prctl` is possible.
3. SEH: `.pdata`/`RUNTIME_FUNCTION` unwinding, `RtlVirtualUnwind`,
   `RtlDispatchException`, C++ exceptions from the title's CRT DLLs.
4. TLS callbacks, thread start/end hooks, APCs, thread pool, synchronisation.
5. Debugger and crash reporting.

This is the shape of Wine's ntdll, not a weekend refactor. Do not start it
unless Phase 0 hits something Wine cannot be made to do.

## 8. Milestones

| # | Milestone | Route | State |
|---|-----------|-------|-------|
| M0 | Prefix boots the title to the start screen | A | boot and picker work at 30 fps; menu transition crashes on invalid SPIR-V (see section 4) |
| M1 | Video, audio, saves | A | not reached |
| M2 | World loads | A | not reached |
| M3 | Full parity | A | not reached |
| M4 | Native offline tools | B/1 | not started |
| M5 | Native menus | B/2 | not started |

Estimates: M0 blocked on the vkd3d-shader/Mesa issue, days to weeks of
upstream work; M1-M2 weeks (coupled to upstream's own world progress), M4
weeks, M5 months and high risk.

## 9. Risks

- **Invalid SPIR-V from vkd3d-proton** (the current blocker): RADV crashes
  instead of refusing the pipeline; a fix needs vkd3d-shader or our emitter.
- **Driver reset** entering the world: known from the Windows build; test with
  captures and short sessions, keep a TTY to restart the display manager.
- **vkd3d-proton gaps**: 64-bit image atomics, wave size 64, sampler feedback,
  `maxMeshOutputVertices`, descriptor heaps above 1,000,000 entries.
- **Wine gaps**: DirectStorage, `gameinput.dll`, Game Pass packaging APIs —
  our HLE should cover them, but 32/64-bit thunking and COM registration may
  differ from Windows.
- **Performance**: submission-per-frame waiting is already a shortcut in
  `victory-gpu`; Wine adds another layer. Both are CPU-side and can be fixed.
- **Upstream divergence**: the Windows build is under active development
  (8,191/8,506 mesh shaders translated today). The Linux goal tracks that work,
  it does not replace it.

## 10. Next actions

1. Reproduce the invalid SPIR-V with vkd3d-shader standalone from
   `~/.local/share/victory/diagnostics/2026-11-24-spirv/*.dxil`; if it
   reproduces, file it with vkd3d-proton and attach the DXIL.
2. File the RADV crash with the `.spv` (Mesa should reject invalid SPIR-V, not
   segfault).
3. Keep `VICTORY_SKIP_PIPELINES` as a diagnostic: it now also suppresses host
   pipeline creation. Shader-by-shader it shows which translation crashes the
   driver, but it is not a way to run the game.
4. In parallel, Phase 1 (`config` defaults and the PE test fix) so asset work
   stops needing Wine.
