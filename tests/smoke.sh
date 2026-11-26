#!/bin/bash
# Smoke test: every script parses, and install / reinstall / uninstall behave on a
# throwaway home folder. Installs no packages and leaves your own session alone:
# commands that would reach it (pkill, hyprctl, gsettings, ...) are stubbed out.
#
#   bash tests/smoke.sh
set -u
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
fails=0
ok()   { printf '\033[32mok\033[0m   %s\n' "$1"; }
fail() { printf '\033[31mFAIL\033[0m %s\n' "$1"; fails=$((fails + 1)); }
check() { local what="$1"; shift; if "$@"; then ok "$what"; else fail "$what"; fi; }
not() { ! "$@"; }

# ------------------------------------------------------------------ syntax --
for f in "$SRC"/install.sh "$SRC"/uninstall.sh "$SRC"/bin/*; do
  case "$(head -1 "$f")" in
    *python*) check "parses: ${f#$SRC/}" python3 -c 'import ast, sys; ast.parse(open(sys.argv[1]).read())' "$f" ;;
    *)        check "parses: ${f#$SRC/}" bash -n "$f" ;;
  esac
done
check "parses: lib/brand.py" python3 -c 'import ast, sys; ast.parse(open(sys.argv[1]).read())' "$SRC/lib/brand.py"
for t in "$SRC"/share/themes/*.toml; do
  check "theme loads: $(basename "$t" .toml)" python3 -c 'import sys, tomllib; tomllib.load(open(sys.argv[1], "rb"))' "$t"
done

# ------------------------------------------------------------ stubbed env --
# The installs below build the Rust programs, so they need what that takes.
command -v cargo >/dev/null && pkg-config --exists gtk+-3.0 gtk-layer-shell-0 ||
  { echo "needs: sudo apt install cargo pkg-config libgtk-3-dev libgtk-layer-shell-dev" >&2; exit 1; }
STUBS="$TMP/stubs"
mkdir -p "$STUBS"
for c in pkill hyprctl makoctl notify-send gsettings waybar setsid swaybg systemctl \
         fc-cache xdg-mime sudo systemd-run; do
  printf '#!/bin/sh\nexit 0\n' > "$STUBS/$c"
done
for c in pipx figlet; do                     # no lettering: falls back to plain text
  printf '#!/bin/sh\nexit 1\n' > "$STUBS/$c"
done
chmod +x "$STUBS"/*

run() {   # run <home> <script> [args...]: in a clean environment, answering "y"
  local home="$1"; shift
  mkdir -p "$home/run"
  yes | env -i HOME="$home" USER=tester PATH="$STUBS:/usr/bin:/bin" \
    XDG_RUNTIME_DIR="$home/run" LANG=C.UTF-8 bash "$@" > "$home/log" 2>&1
}
backups() { find "$1/.config" -name '*.bak-[0-9]*' 2>/dev/null | wc -l; }

# ----------------------------------------------- fresh install, reinstall --
H="$TMP/fresh"
run "$H" "$SRC/install.sh" --configs-only --name Tester
check "install succeeds" grep -q 'Done.' "$H/log" || tail -20 "$H/log"
check "configs written" test -f "$H/.config/hypr/hyprland.lua" -a -f "$H/.config/waybar/style.css"
check "manifest written" test -s "$H/.config/nameplate/managed"
check "no backups on a fresh install" test "$(backups "$H")" -eq 0

run "$H" "$SRC/install.sh" --configs-only --theme gruvbox
check "reinstall with a new theme makes no backups" test "$(backups "$H")" -eq 0
check "theme applied" grep -q gruvbox "$H/.config/nameplate/theme"
check "reinstall keeps the name" grep -qx Tester "$H/.config/nameplate/name"

run "$H" "$H/.config/nameplate/bin/theme" set nord --keep-wallpaper   # as from the menu
run "$H" "$SRC/install.sh" --configs-only
check "switching theme from the menu doesn't cause backups later" test "$(backups "$H")" -eq 0

echo "# my edit" >> "$H/.config/hypr/hyprland.lua"
run "$H" "$SRC/install.sh" --configs-only
check "your own edit is backed up" test "$(find "$H/.config/hypr" -name 'hyprland.lua.bak-*' | wc -l)" -eq 1
check "scripts get no backups" test "$(find "$H/.config/nameplate/bin" -name '*.bak-*' | wc -l)" -eq 0

# --------------------------------------------------------- Rust programs --
B="$H/.config/nameplate/bin"
NP="$B/nameplate"
check "nameplate was built" test -x "$NP"
check "nameplate prints a bar item" sh -c '"$1" bar logo | grep -q "\"class\""' _ "$NP"
check "nameplate refuses an unknown item" not "$NP" bar nonsense
check "nameplate refresh accepts items" "$NP" refresh sound dnd
check "nameplate-agents was built" test -x "$B/nameplate-agents"
check "agents stats is JSON" sh -c '"$1" stats | python3 -c "import json,sys; json.load(sys.stdin)"' _ "$B/nameplate-agents"
check "nameplate-shell was built" test -x "$B/nameplate-shell"
check "wallpaper list has the default" sh -c 'HOME="$2" "$1" wallpaper list | grep -q wallpaper.png' _ "$B/nameplate-shell" "$H"
check "dock pin works" sh -c 'HOME="$2" "$1" dock pin x.desktop && HOME="$2" "$1" dock pinned | grep -qx x.desktop' _ "$B/nameplate-shell" "$H"
check "panel refuses an unknown kind" not "$B/nameplate-shell" panel nonsense
check "panel close works with nothing open" "$B/nameplate-shell" panel close
check "replaced scripts are gone" not test -e "$B/dock" -o -e "$B/wallpaper" -o -e "$B/agents"
check "Rust unit tests pass" env CARGO_HOME="$H/.cargo" CARGO_TARGET_DIR="$H/.cache/nameplate/rust-target" \
  cargo test --quiet --locked --manifest-path "$SRC/rust/Cargo.toml" -p nameplate -p nameplate-agents

# ------------------------------------------------------------ appearance --
check "lock blur file created" grep -qx '$np_blur = 3' "$H/.config/nameplate/lock-blur.conf"
run "$H" "$H/.config/nameplate/bin/toggle" lock-blur light
check "lock blur can be changed" grep -qx '$np_blur = 1' "$H/.config/nameplate/lock-blur.conf"
check "lock screen reads the blur file" grep -q 'nameplate/lock-blur.conf' "$H/.config/hypr/hyprlock.conf"

# ------------------------------------------------------------- reminders --
R="$H/.config/nameplate/bin/reminder"
check "reminder understands \"20m stretch\"" run "$H" "$R" set 20m stretch
check "reminder understands \"at 15:30 call home\"" run "$H" "$R" set at 15:30 call home
check "reminder refuses a time it can't read" not run "$H" "$R" set someday maybe
check "reminder list works with none set" run "$H" "$R" list --plain

# ------------------------------- install over existing configs, uninstall --
H="$TMP/existing"
mkdir -p "$H/.config/alacritty" "$H/.config/hypr"
echo "# my terminal" > "$H/.config/alacritty/alacritty.toml"
echo "# keep me" > "$H/.config/hypr/notes.txt"
run "$H" "$SRC/install.sh" --configs-only
check "existing config is backed up" grep -q 'backed up' "$H/log"
sleep 1   # a second install in the same second would reuse the backup's name
run "$H" "$SRC/install.sh" --configs-only
run "$H" "$SRC/uninstall.sh"
check "uninstall restores your original" grep -qx '# my terminal' "$H/.config/alacritty/alacritty.toml"
check "uninstall keeps your other files" test -f "$H/.config/hypr/notes.txt"
check "uninstall removes the project's files" test ! -e "$H/.config/hypr/hyprland.lua" -a ! -e "$H/.config/nameplate"
check "uninstall removes emptied folders" test ! -e "$H/.config/waybar"

echo
if (( fails )); then printf '\033[31m%d failed\033[0m\n' "$fails"; exit 1; fi
printf '\033[32mall passed\033[0m\n'
