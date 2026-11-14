#!/bin/bash
# hypr-nameplate: an Omarchy-style Hyprland desktop for Ubuntu, with your name on it.
# Installs next to GNOME, backs up any config it replaces, and is safe to re-run.
set -u
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
APP="$CFG/nameplate"
BIN="$HOME/.local/bin"
STAMP="$(date +%Y%m%d-%H%M%S)"
PPA="ppa:cppiber/hyprland"
BASE_PKGS="alacritty pipx curl fontconfig xz-utils"
CORE_PKGS="hyprland hyprlock hypridle waybar xdg-desktop-portal-hyprland"
EXTRA_PKGS="hyprpolkitagent fuzzel mako-notifier swaybg brightnessctl playerctl grim slurp wl-clipboard pavucontrol libnotify-bin"
export PATH="$BIN:$PATH"

NAME="${USER:-nameplate}"
TERMINAL_ONLY=0
CONFIGS_ONLY=0
failed=()

usage() {
  cat <<MSG
Usage: bash install.sh [options]

  --name NAME       Name shown on the screensaver, lock screen and bar
                    (default: your username, "$NAME"). Up to about 12
                    characters gets the large block lettering.
  --terminal-only   Only the terminal, prompt and screensaver, on your current
                    GNOME desktop. Skips Hyprland entirely.
  --configs-only    Don't install any packages; just (re)write the configs.
                    Use this to change the name or restore the defaults.
  -h, --help        Show this help.
MSG
}

while (( $# )); do
  case "$1" in
    --name)          NAME="${2:-}"; shift ;;
    --name=*)        NAME="${1#*=}" ;;
    --terminal-only) TERMINAL_ONLY=1 ;;
    --configs-only)  CONFIGS_ONLY=1 ;;
    -h|--help)       usage; exit 0 ;;
    *)               echo "Unknown option: $1"; usage; exit 1 ;;
  esac
  shift
done

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$1"; }
warn() { printf '\033[1;33m%s\033[0m\n' "$1"; }
die()  { printf '\033[1;31m%s\033[0m\n' "$1"; exit 1; }
ask()  { local a; read -r -p "$1 [y/N] " a; [[ $a == [yY]* ]]; }

[[ $EUID -eq 0 ]] && die "Run this as your normal user, not with sudo."
[[ $NAME =~ ^[A-Za-z0-9][A-Za-z0-9._\ -]{0,31}$ ]] ||
  die "Name must be 1-32 characters: letters, numbers, spaces, dots, dashes or underscores."

put() {   # put <source> <destination>, backing up a different existing file
  mkdir -p "$(dirname "$2")"
  if [[ -e $2 ]] && ! cmp -s "$1" "$2"; then
    cp -a "$2" "$2.bak-$STAMP"
    echo "backed up $2 -> $2.bak-$STAMP"
  fi
  cp "$1" "$2"
}

render() {   # render <template> <destination>: fill in the placeholders, then put
  local tmp; tmp="$(mktemp)"
  NP_HOME="$HOME" NP_NAME="$NAME" NP_ICONS="$ICON_THEME" NP_LOCK="$LOCK_BRAND" python3 - "$1" > "$tmp" <<'PY'
import os, sys
text = open(sys.argv[1], encoding="utf-8").read()
for token, key in (("@HOME@", "NP_HOME"), ("@NAME@", "NP_NAME"),
                   ("@ICON_THEME@", "NP_ICONS"), ("@LOCK_BRAND@", "NP_LOCK")):
    text = text.replace(token, os.environ[key])
sys.stdout.write(text)
PY
  put "$tmp" "$2"
  rm -f "$tmp"
}

# ---------------------------------------------------------------- packages --

install_base() {
  step "Installing the terminal and helpers (asks for your password)"
  sudo apt-get update && sudo apt-get install -y $BASE_PKGS || failed+=("apt packages")

  step "Installing terminaltexteffects (the screensaver animations)"
  pipx install terminaltexteffects || pipx upgrade terminaltexteffects || failed+=("terminaltexteffects")

  step "Installing JetBrainsMono Nerd Font"
  local fonts="$HOME/.local/share/fonts/JetBrainsMonoNerd"
  if fc-list | grep -qi "JetBrainsMono Nerd Font"; then
    echo "already installed"
  else
    mkdir -p "$fonts"
    if curl -fL --retry 3 -o /tmp/JetBrainsMono.tar.xz \
      https://github.com/ryanoasis/nerd-fonts/releases/latest/download/JetBrainsMono.tar.xz &&
      tar -xJf /tmp/JetBrainsMono.tar.xz -C "$fonts"; then
      fc-cache -f
    else
      failed+=("Nerd Font download")
    fi
    rm -f /tmp/JetBrainsMono.tar.xz
  fi

  step "Installing the Starship prompt"
  mkdir -p "$BIN"
  if command -v starship >/dev/null; then
    echo "already installed"
  else
    curl -fsSL https://starship.rs/install.sh | sh -s -- -y -b "$BIN" || failed+=("Starship")
  fi
}

install_hyprland() {
  . /etc/os-release
  case "${VERSION_ID:-}" in
    24.04|26.04) ;;
    *) warn "Tested on Ubuntu 24.04 only; written to also work on 26.04. You are on ${PRETTY_NAME:-something else}."
       ask "Continue anyway?" || exit 0 ;;
  esac

  step "Before Hyprland is installed"
  cat <<MSG
Hyprland comes from a community package source ($PPA), because Ubuntu's
own package is too old for this config. On Ubuntu 24.04 that source also
carries a newer libinput (the input-device library), which GNOME uses too.
That is the one part of this setup that touches your existing desktop, so a
Timeshift snapshot first is what lets you undo it completely.
MSG
  ask "Have you taken a Timeshift snapshot (or another full backup)?" ||
    die "Stopping. Take a snapshot (sudo apt install timeshift), then run this again."

  if grep -Eqs '^\s*AutomaticLoginEnable\s*=\s*[Tt]rue' /etc/gdm3/custom.conf; then
    warn "Automatic login is on. Turn it off in Settings > System > Users, or you"
    warn "won't see the login screen where you choose between Hyprland and GNOME."
    ask "Continue anyway?" || exit 0
  fi

  step "Adding $PPA"
  command -v add-apt-repository >/dev/null || sudo apt-get install -y software-properties-common
  sudo add-apt-repository -y "$PPA" && sudo apt-get update || die "Could not add the package source."

  step "Checking what apt would change"
  local plan removals upgrades
  plan="$(apt-get install -s $CORE_PKGS 2>&1)" ||
    { echo "$plan" | tail -20; die "apt cannot install Hyprland cleanly. Nothing was installed."; }
  removals="$(grep '^Remv' <<<"$plan")"
  if [[ -n $removals ]]; then
    echo "$removals"
    die "apt wants to REMOVE the packages above, so this stops here. Nothing was installed."
  fi
  upgrades="$(grep -E '^Inst [^ ]+ \[' <<<"$plan" | awk '{print "  " $2, $3, "->", $4}' | tr -d '()')"
  echo "Packages to install or upgrade: $(grep -c '^Inst' <<<"$plan")   To remove: 0"
  if [[ -n $upgrades ]]; then
    echo "Already-installed packages that get a newer version:"
    echo "$upgrades"
  fi
  ask "Install?" || die "Stopped before installing. To remove the package source: sudo add-apt-repository -r $PPA"

  step "Installing Hyprland"
  sudo apt-get install -y $CORE_PKGS || die "Install failed; see the output above."

  step "Installing helpers"
  local p
  for p in $EXTRA_PKGS; do
    if sudo apt-get install -y "$p" >/dev/null 2>&1; then echo "ok   $p"; else echo "FAIL $p"; failed+=("$p"); fi
  done

  if ! id -nG "$USER" | grep -qw video; then
    step "Allowing brightness keys (adds you to the 'video' group)"
    sudo usermod -aG video "$USER" || failed+=("video group")
  fi
}

# ---------------------------------------------------------------- branding --

make_brand() {   # writes screensaver.txt (and logo.png when possible) into $BRAND
  local font
  for font in delta_corps_priest_1 ansi_regular small; do
    if pipx run pyfiglet -f "$font" -w 1000 "$NAME" 2>/dev/null | python3 "$SRC/lib/brand.py" "$BRAND" 2>/dev/null; then
      echo "lettering: $font"
      return
    fi
  done
  if command -v figlet >/dev/null && figlet -w 1000 "$NAME" | python3 "$SRC/lib/brand.py" "$BRAND" 2>/dev/null; then
    echo "lettering: figlet"
    return
  fi
  warn "Could not draw block lettering (no internet, or the name is too long); using plain text."
  printf '%s\n' "$NAME" > "$BRAND/screensaver.txt"
}

# -------------------------------------------------------------------- main --

if (( ! CONFIGS_ONLY )); then
  install_base
  (( TERMINAL_ONLY )) || install_hyprland
fi

step "Drawing \"$NAME\""
BRAND="$(mktemp -d)"
make_brand

ICON_THEME="$(gsettings get org.gnome.desktop.interface icon-theme 2>/dev/null | tr -d "'")"
[[ -n $ICON_THEME ]] || ICON_THEME="Yaru"

if [[ -f $BRAND/logo.png ]]; then
  LOCK_BRAND="# Your name
image {
    monitor =
    path = $APP/logo.png
    size = 130
    rounding = 0
    border_size = 0
    position = 0, 8%
    halign = center
    valign = center
}"
else
  LOCK_BRAND="# Your name
label {
    monitor =
    text = $NAME
    color = rgba(122, 162, 247, 1.0)
    font_size = 64
    font_family = JetBrainsMono Nerd Font ExtraBold
    position = 0, 8%
    halign = center
    valign = center
}"
fi

step "Copying configs"
put "$SRC/config/alacritty/alacritty.toml"   "$CFG/alacritty/alacritty.toml"
put "$SRC/config/alacritty/screensaver.toml" "$CFG/alacritty/screensaver.toml"
put "$SRC/config/starship.toml"              "$CFG/starship.toml"
put "$BRAND/screensaver.txt"                 "$APP/screensaver.txt"
if [[ -f $BRAND/logo.png ]]; then put "$BRAND/logo.png" "$APP/logo.png"; else rm -f "$APP/logo.png"; fi
put "$SRC/share/wallpaper.png"               "$APP/wallpaper.png"
render "$SRC/share/KEYS.txt"                 "$APP/KEYS.txt"
printf '%s\n' "$NAME" > "$APP/name"
for f in "$SRC"/bin/*; do
  put "$f" "$APP/bin/$(basename "$f")"
  chmod +x "$APP/bin/$(basename "$f")"
done
rm -rf "$BRAND"

if (( ! TERMINAL_ONLY )); then
  put    "$SRC/config/hypr/hyprland.lua"    "$CFG/hypr/hyprland.lua"
  render "$SRC/config/hypr/hyprlock.conf"   "$CFG/hypr/hyprlock.conf"
  render "$SRC/config/hypr/hypridle.conf"   "$CFG/hypr/hypridle.conf"
  render "$SRC/config/waybar/config.jsonc"  "$CFG/waybar/config.jsonc"
  put    "$SRC/config/waybar/style.css"     "$CFG/waybar/style.css"
  render "$SRC/config/fuzzel/fuzzel.ini"    "$CFG/fuzzel/fuzzel.ini"
  put    "$SRC/config/mako/config"          "$CFG/mako/config"
  put    "$SRC/config/xdg-desktop-portal/hyprland-portals.conf" "$CFG/xdg-desktop-portal/hyprland-portals.conf"
  [[ -e $CFG/waybar/config ]] || ln -s config.jsonc "$CFG/waybar/config"   # older waybar reads "config"

  # Spotify cuts off its player bar under Hyprland unless it runs as a native Wayland app.
  # A copy of its launcher entry in ~/.local/share/applications takes priority over the system one.
  for src in /var/lib/snapd/desktop/applications/spotify_spotify.desktop /usr/share/applications/spotify.desktop; do
    [[ -f $src ]] || continue
    dst="$HOME/.local/share/applications/$(basename "$src")"
    mkdir -p "$(dirname "$dst")"
    [[ -f $dst ]] || cp "$src" "$dst"
    sed -i '/^Exec=/{ /ozone-platform/! s#\( %[uUfF]\)\{0,1\}$# --enable-features=UseOzonePlatform --ozone-platform=wayland\1# }' "$dst"
    echo "Spotify set to run natively on Wayland ($dst)"
  done
fi

step "Enabling the prompt in ~/.bashrc"
if grep -q 'starship init bash' "$HOME/.bashrc" 2>/dev/null; then
  echo "already enabled"
else
  [[ -f $HOME/.bashrc ]] && cp -a "$HOME/.bashrc" "$HOME/.bashrc.bak-$STAMP"
  cat >> "$HOME/.bashrc" <<'RC'

# hypr-nameplate prompt
export PATH="$HOME/.local/bin:$PATH"
command -v starship >/dev/null && eval "$(starship init bash)"
RC
  echo "added"
fi

step "Screensaver on GNOME"
# Hyprland uses hypridle; GNOME has no idle hook for this, so a small watcher starts at login.
mkdir -p "$CFG/autostart"
cat > "$CFG/autostart/nameplate-screensaver.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=nameplate screensaver
Comment=Shows the screensaver when the GNOME session is idle
Exec=bash $APP/bin/idle-watch
OnlyShowIn=GNOME;
X-GNOME-Autostart-enabled=true
NoDisplay=true
DESKTOP
rm -f "$CFG/autostart/adoodevv-screensaver.desktop"   # entry from the pre-release version of this setup
pkill -f "[b]in/idle-watch" 2>/dev/null
if [[ ${XDG_CURRENT_DESKTOP:-} == *GNOME* && -n ${DBUS_SESSION_BUS_ADDRESS:-} ]]; then
  setsid -f bash "$APP/bin/idle-watch" >/dev/null 2>&1
  echo "watcher running (starts after 2.5 minutes idle)"
else
  echo "starts at your next GNOME login"
fi

# Live session: pick up a changed name without logging out
if [[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]] && (( ! TERMINAL_ONLY )); then
  pkill -x waybar 2>/dev/null; setsid -f waybar >/dev/null 2>&1
fi

echo
if (( ${#failed[@]} )); then
  printf '\033[1;31mFinished, but these steps failed:\033[0m\n'
  printf '  - %s\n' "${failed[@]}"
  echo "Check your connection and run the installer again."
else
  printf '\033[1;32mDone.\033[0m\n'
fi

echo
echo "Try it:"
echo "  alacritty                                   the new terminal"
echo "  ~/.config/nameplate/bin/launch-screensaver  preview the screensaver (any key exits)"
if (( ! TERMINAL_ONLY && ! CONFIGS_ONLY )); then
  cat <<MSG

Start Hyprland:
  1. Log out (or restart).
  2. On the login screen click your name, then the gear icon at the bottom right.
  3. Choose "Hyprland" (not the "uwsm" one if both are listed), then log in.
  4. Press SUPER + K for the keyboard cheat sheet.

Back to GNOME: SUPER + Escape -> Log out, then pick "Ubuntu" from the gear icon.
MSG
fi
echo
echo "Change the name later:  bash install.sh --configs-only --name NEWNAME"
echo "Remove everything:      bash uninstall.sh"
