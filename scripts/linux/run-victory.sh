#!/usr/bin/env bash
# Runs VIctory's Windows build on Linux with Wine, vkd3d-proton (D3D12) and
# DXVK's dxgi (swapchain). See docs/linux-port.md.
#
#   scripts/linux/run-victory.sh [extra arguments for `victory run`]
#
# Environment:
#   VICTORY_EXE        Windows build to run (default: release target)
#   VICTORY_GAME_DIR   folder holding the title's files (default: [paths].game_dir
#                      from victory.toml, with Wine's Z: stripped)
#   VICTORY_TITLE_EXE  title executable name (default: game_scarlett_master.exe)
#   VICTORY_WINEPREFIX prefix (default: ~/.local/share/victory/prefix)
#   PROTON_DIR         Proton/GE-Proton whose vkd3d-proton and DXVK to use
#   VICTORY_LOG        passed to the title (e.g. info, victory_gpu=debug)
#   VICTORY_KEYS       diagnostics builds only: "45:Enter,60:Enter"
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
prefix=${VICTORY_WINEPREFIX:-$HOME/.local/share/victory/prefix}
exe=${VICTORY_EXE:-$root/target/x86_64-pc-windows-msvc/release/victory.exe}
title_exe=${VICTORY_TITLE_EXE:-game_scarlett_master.exe}

if [ ! -f "$exe" ]; then
    echo "run-victory: $exe not found; build it first:" >&2
    echo "  cargo xwin build --target x86_64-pc-windows-msvc --release -p victory" >&2
    exit 1
fi

game_dir=${VICTORY_GAME_DIR:-}
if [ -z "$game_dir" ] && [ -f "$root/victory.toml" ]; then
    raw=$(sed -n "s/^[[:space:]]*game_dir[[:space:]]*=[[:space:]]*'\(.*\)'/\1/p" \
        "$root/victory.toml" | head -1)
    if [ -n "$raw" ]; then
        # Z:\mnt\... is /mnt/... on the host.
        game_dir=$(printf '%s' "$raw" | sed 's/^[A-Za-z]://' | sed 's#\\#/#g')
    fi
fi
if [ ! -d "${game_dir:-}" ]; then
    echo "run-victory: no game folder; set VICTORY_GAME_DIR" >&2
    exit 1
fi

# Proton Experimental has the vkd3d-proton build Proton itself uses (3.2+,
# which structures the translated shaders this title produces); a GE-Proton
# is the fallback, but its older vkd3d-proton crashes RADV on some shaders.
steam_libraries() {
    printf '%s\n' "$HOME/.local/share/Steam" "$HOME/.steam/steam"
    local vdf
    for vdf in "$HOME/.steam/steam/steamapps/libraryfolders.vdf" \
        "$HOME/.local/share/Steam/steamapps/libraryfolders.vdf"; do
        [ -f "$vdf" ] && sed -n 's/.*"path"[[:space:]]*"\(.*\)"/\1/p' "$vdf"
    done
}
if [ -z "${PROTON_DIR:-}" ]; then
    while IFS= read -r lib; do
        candidate="$lib/steamapps/common/Proton - Experimental"
        if [ -d "$candidate/files/lib/wine/vkd3d-proton" ]; then
            PROTON_DIR=$candidate
            break
        fi
    done < <(steam_libraries | sort -u)
fi
if [ -z "${PROTON_DIR:-}" ]; then
    PROTON_DIR=$(ls -d "$HOME"/.local/share/Steam/compatibilitytools.d/GE-Proton* \
        "$HOME"/.steam/root/compatibilitytools.d/GE-Proton* 2>/dev/null | sort -V | tail -1)
fi
if [ -z "${PROTON_DIR:-}" ] || [ ! -d "$PROTON_DIR/files/lib/wine/vkd3d-proton" ]; then
    echo "run-victory: no Proton with vkd3d-proton found; set PROTON_DIR" >&2
    exit 1
fi

export WINEPREFIX=$prefix
export WINEDEBUG=${WINEDEBUG:--all}
export VKD3D_DEBUG=${VKD3D_DEBUG:-none}
export DXVK_LOG_LEVEL=${DXVK_LOG_LEVEL:-none}
export WINEDLLOVERRIDES="d3d12,d3d12core,dxgi,d3d11,d3d10core=n,b${WINEDLLOVERRIDES:+,$WINEDLLOVERRIDES}"

if [ ! -d "$prefix/drive_c/windows/system32" ]; then
    echo "run-victory: creating prefix $prefix"
    wineboot -i >/dev/null 2>&1
fi

sys32=$prefix/drive_c/windows/system32
install_dll() {
    local src=$1 dst=$sys32/$(basename "$1")
    if ! cmp -s "$src" "$dst"; then
        echo "run-victory: installing $(basename "$src")"
        cp -f "$src" "$dst"
    fi
}
vkd3d=$PROTON_DIR/files/lib/wine/vkd3d-proton/x86_64-windows
dxvk=$PROTON_DIR/files/lib/wine/dxvk/x86_64-windows
for dll in d3d12.dll d3d12core.dll; do install_dll "$vkd3d/$dll"; done
for dll in dxgi.dll d3d11.dll d3d10core.dll; do install_dll "$dxvk/$dll"; done

cd "$game_dir"
title_win=$(winepath -w "$game_dir/$title_exe")

echo "run-victory: prefix=$prefix" >&2
echo "run-victory: title=$title_win" >&2
exec wine "$exe" run "$title_win" --execute "$@"
