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
EXTRA_PKGS="hyprsunset hyprpicker tesseract-ocr ffmpeg btop jq zenity python3-gi python3-gi-cairo gir1.2-gtk-3.0 gir1.2-gtklayershell-0.1 cliphist wf-recorder fonts-noto-color-emoji webp-pixbuf-loader power-profiles-daemon upower hyprpolkitagent fuzzel mako-notifier swaybg brightnessctl playerctl grim slurp wl-clipboard pavucontrol libnotify-bin"
export PATH="$BIN:$PATH"

NAME="$(cat "$APP/name" 2>/dev/null)"   # the name from the last install, if any
NAME="${NAME:-${USER:-nameplate}}"
TERMINAL_ONLY=0
CONFIGS_ONLY=0
THEME=""
failed=()

usage() {
  cat <<MSG
Usage: bash install.sh [options]

  --name NAME       Name shown on the screensaver and bar
                    (default: the current one, "$NAME"). Up to about 12
                    characters gets the large block lettering.
  --terminal-only   Only the terminal, prompt and screensaver, on your current
                    GNOME desktop. Skips Hyprland entirely.
  --configs-only    Don't install any packages; just (re)write the configs.
                    Use this to change the name or restore the defaults.
  --theme THEME     Colour theme, e.g. gruvbox, nord, catppuccin, rose-pine
                    (default: keep the current one, Tokyo Night on a first
                    install). See share/themes. Change it later with
                    SUPER + CTRL + SHIFT + SPACE.
  -h, --help        Show this help.
MSG
}

while (( $# )); do
  case "$1" in
    --name)          NAME="${2:-}"; shift ;;
    --name=*)        NAME="${1#*=}" ;;
    --terminal-only) TERMINAL_ONLY=1 ;;
    --configs-only)  CONFIGS_ONLY=1 ;;
    --theme)         THEME="${2:-}"; shift ;;
    --theme=*)       THEME="${1#*=}" ;;
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
[[ -z $THEME || -f $SRC/share/themes/$THEME.toml ]] ||
  die "No theme called '$THEME'. Pick one of: $(cd "$SRC/share/themes" && ls *.toml | sed 's/\.toml$//' | tr '\n' ' ')"
[[ $NAME =~ ^[A-Za-z0-9][A-Za-z0-9._\ -]{0,31}$ ]] ||
  die "Name must be 1-32 characters: letters, numbers, spaces, dots, dashes or underscores."

# $APP/managed: a sha256sum line for every config this project installs, as it stood after
# the last install (theme and font update it when they rewrite a file). A config that still
# matches it is ours, so it is replaced without a backup; anything else you edited is kept.
# $APP/originals: "destination<TAB>backup" for files that existed before the first install,
# which uninstall.sh puts back.
MANAGED="$APP/managed"
ORIGINALS="$APP/originals"
managed=()
# Installs made before the manifest existed: there's no telling which backup was your
# original, so none is recorded (uninstall.sh then leaves all the backups in place).
[[ -f $APP/name && ! -f $MANAGED ]] && LEGACY=1 || LEGACY=0

ours()  { [[ -f $MANAGED ]] && grep -qxF "$(sha256sum "$1")" "$MANAGED"; }
known() { [[ -f $MANAGED ]] && cut -d' ' -f3- "$MANAGED" | grep -qxF "$1"; }

put() {   # put <source> <destination>, backing up an existing file you edited
  mkdir -p "$(dirname "$2")"
  if [[ -e $2 ]] && ! cmp -s "$1" "$2" && ! ours "$2"; then
    cp -a "$2" "$2.bak-$STAMP"
    echo "backed up $2 -> $2.bak-$STAMP"
    if (( ! LEGACY )) && ! known "$2" && ! { [[ -f $ORIGINALS ]] && cut -f1 "$ORIGINALS" | grep -qxF "$2"; }; then
      mkdir -p "$APP"
      printf '%s\t%s\n' "$2" "$2.bak-$STAMP" >> "$ORIGINALS"
    fi
  fi
  cp "$1" "$2"
  managed+=("$2")
}

save_manifest() {   # hash the configs as they are now, keeping entries this run didn't touch
  local line p list=() kept=()
  local -A seen=()
  for p in "${managed[@]}"; do [[ -n ${seen[$p]:-} ]] || { seen[$p]=1; list+=("$p"); }; done
  if [[ -f $MANAGED ]]; then
    while IFS= read -r line; do
      p="${line:66}"
      [[ -z ${seen[$p]:-} && -f $p ]] && { seen[$p]=1; kept+=("$line"); }
    done < "$MANAGED"
  fi
  { (( ${#kept[@]} )) && printf '%s\n' "${kept[@]}"; (( ${#list[@]} )) && sha256sum "${list[@]}"; } > "$MANAGED.tmp"
  mv "$MANAGED.tmp" "$MANAGED"
}

created() {   # note a file this project created (rather than replaced), for uninstall.sh
  grep -qsxF "$1" "$APP/created" || { mkdir -p "$APP"; printf '%s\n' "$1" >> "$APP/created"; }
}

render() {   # render <template> <destination>: fill in the placeholders, then put
  local tmp; tmp="$(mktemp)"
  NP_HOME="$HOME" NP_NAME="$NAME" NP_ICONS="$ICON_THEME" python3 - "$1" > "$tmp" <<'PY'
import os, sys
text = open(sys.argv[1], encoding="utf-8").read()
for token, key in (("@HOME@", "NP_HOME"), ("@NAME@", "NP_NAME"),
                   ("@ICON_THEME@", "NP_ICONS")):
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

make_brand() {   # writes screensaver.txt into $BRAND
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

step "Copying configs"
put "$SRC/config/alacritty/alacritty.toml"   "$CFG/alacritty/alacritty.toml"
put "$SRC/config/alacritty/screensaver.toml" "$CFG/alacritty/screensaver.toml"
put "$SRC/config/starship.toml"              "$CFG/starship.toml"
put "$BRAND/screensaver.txt"                 "$APP/screensaver.txt"
rm -f "$APP/logo.png" "$APP/lock-icon.png"   # older versions drew these for the lock screen
put "$SRC/share/wallpaper.png"               "$APP/wallpaper.png"
for f in "$SRC"/share/agents/*.svg; do       # agent logos for the agents panel (from Omarchy, MIT)
  put "$f" "$APP/agents/$(basename "$f")"
done
render "$SRC/share/KEYS.txt"                 "$APP/KEYS.txt"
printf '%s\n' "$NAME" > "$APP/name"
# The helper scripts belong to this project, so they're replaced without backups
# (and the backups older versions made of them are cleared out)
mkdir -p "$APP/bin"
rm -f "$APP"/bin/*.bak-*
for f in "$SRC"/bin/*; do
  install -m 755 "$f" "$APP/bin/$(basename "$f")"
done
rm -rf "$BRAND"
"$APP/bin/wallpaper" lock 2>/dev/null || true   # lock screen background

if (( ! TERMINAL_ONLY )); then
  put    "$SRC/config/hypr/hyprland.lua"    "$CFG/hypr/hyprland.lua"
  render "$SRC/config/hypr/hyprlock.conf"   "$CFG/hypr/hyprlock.conf"
  render "$SRC/config/hypr/hypridle.conf"   "$CFG/hypr/hypridle.conf"
  render "$SRC/config/waybar/config.jsonc"  "$CFG/waybar/config.jsonc"
  put    "$SRC/config/waybar/style.css"     "$CFG/waybar/style.css"
  render "$SRC/config/fuzzel/fuzzel.ini"    "$CFG/fuzzel/fuzzel.ini"
  put    "$SRC/config/mako/config"          "$CFG/mako/config"
  put    "$SRC/config/xdg-desktop-portal/hyprland-portals.conf" "$CFG/xdg-desktop-portal/hyprland-portals.conf"
  if [[ ! -e $CFG/waybar/config ]]; then   # older waybar reads "config"
    ln -s config.jsonc "$CFG/waybar/config" && created "$CFG/waybar/config"
  fi

  # Spotify cuts off its player bar under Hyprland unless it runs as a native Wayland app.
  # A copy of its launcher entry in ~/.local/share/applications takes priority over the system one.
  for src in /var/lib/snapd/desktop/applications/spotify_spotify.desktop /usr/share/applications/spotify.desktop; do
    [[ -f $src ]] || continue
    dst="$HOME/.local/share/applications/$(basename "$src")"
    mkdir -p "$(dirname "$dst")"
    [[ -f $dst ]] || { cp "$src" "$dst" && created "$dst"; }
    sed -i '/^Exec=/{ /ozone-platform/! s#\( %[uUfF]\)\{0,1\}$# --enable-features=UseOzonePlatform --ozone-platform=wayland\1# }' "$dst"
    echo "Spotify set to run natively on Wayland ($dst)"
  done

  # Notion's snap crashes under Hyprland: it can't reach the Wayland display from its
  # sandbox. Running it through XWayland works (and is how it runs on GNOME's X11 too).
  for src in /var/lib/snapd/desktop/applications/notion*.desktop; do
    [[ -f $src ]] || continue
    dst="$HOME/.local/share/applications/$(basename "$src")"
    mkdir -p "$(dirname "$dst")"
    [[ -f $dst ]] || { cp "$src" "$dst" && created "$dst"; }
    sed -i '/^Exec=/{ /ozone-platform/! s#\( %[uUfF]\)\{0,1\}$# --ozone-platform=x11\1# }' "$dst"
    echo "Notion set to open through XWayland ($dst)"
  done

  # Proton VPN's tray menu won't open in the bar: its tray code sends the menu in a
  # format the bar rejects (see share/fix-proton-tray). Offer the one-word fix, kept
  # in place after Proton updates by an apt hook.
  proton_tray=/usr/lib/python3/dist-packages/proton/vpn/app/gtk/widgets/main/tray_icon.py
  if [[ -f $proton_tray && ! -f /etc/apt/apt.conf.d/99-hypr-nameplate-proton-tray ]]; then
    read -rp "Fix Proton VPN's tray menu so right-click works? It edits one line of Proton's app and needs sudo. [Y/n] " answer
    if [[ ${answer,,} != n* ]]; then
      sudo install -D -m 755 "$SRC/share/fix-proton-tray" /usr/local/lib/hypr-nameplate/fix-proton-tray &&
        echo 'DPkg::Post-Invoke { "/usr/local/lib/hypr-nameplate/fix-proton-tray || true"; };' |
          sudo tee /etc/apt/apt.conf.d/99-hypr-nameplate-proton-tray >/dev/null &&
        sudo /usr/local/lib/hypr-nameplate/fix-proton-tray &&
        echo "Proton VPN tray menu fixed (restart Proton to see it)" || failed+=("proton tray fix")
    fi
  fi

  # GNOME Settings refuses to open outside GNOME; telling it it is on GNOME makes it work
  # (Wi-Fi, Bluetooth, sound and most panels do; Displays does not, Hyprland owns the screen).
  src=/usr/share/applications/org.gnome.Settings.desktop
  if [[ -f $src ]]; then
    dst="$HOME/.local/share/applications/org.gnome.Settings.desktop"
    mkdir -p "$(dirname "$dst")"
    [[ -f $dst ]] || { cp "$src" "$dst" && created "$dst"; }
    sed -i '/^Exec=/{ /XDG_CURRENT_DESKTOP=GNOME/! s#^Exec=#Exec=env XDG_CURRENT_DESKTOP=GNOME #; }' "$dst"
    echo "Settings app set to open under Hyprland ($dst)"
  fi
fi

# The panels run in the background from login; stop the old copy so the next click
# starts the new one
pkill -f "nameplate/bin/panel daemon" 2>/dev/null || true
# The dock: restart it with the new version (inside Hyprland)
pkill -f "nameplate/bin/dock$" 2>/dev/null || true
# Same for the wallpaper: it only starts at login, so a new copy would
# otherwise wait for the next one before clicks and transitions update
pkill -f "nameplate/bin/wallpaper$" 2>/dev/null || true
if [[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]]; then
  sleep 0.3; setsid -f "$APP/bin/dock" >/dev/null 2>&1
  setsid -f "$APP/bin/wallpaper" >/dev/null 2>&1
fi
# hypridle reads its config only when it starts: restart it inside Hyprland
if [[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]] && pgrep -x hypridle >/dev/null; then
  pkill -x hypridle; sleep 0.5; setsid -f hypridle >/dev/null 2>&1
fi

step "Font"
# The configs above name JetBrainsMono Nerd Font at size 9; put back the font you picked
"$APP/bin/font" refresh && "$APP/bin/font" || failed+=("font")

step "Colour theme"
# The configs above arrive in Tokyo Night; put back the theme you had (or the one asked for)
mkdir -p "$APP/themes"
cp "$SRC"/share/themes/*.toml "$APP/themes/"
for f in "$SRC"/share/themes/*.hyprland.lua; do
  [[ -f $f ]] && cp "$f" "$APP/themes/"
done
if [[ -n $THEME ]]; then
  "$APP/bin/theme" set "$THEME" --keep-wallpaper >/dev/null && echo "$THEME" || failed+=("theme")
else
  "$APP/bin/theme" refresh && "$APP/bin/theme" || failed+=("theme")
fi

# Font and theme have settled the configs; remember them as ours
save_manifest

if (( ! TERMINAL_ONLY && ! CONFIGS_ONLY )) && [[ ! -d $HOME/Pictures/Wallpapers/omarchy ]]; then
  step "Wallpapers"
  if ask "Download Omarchy's wallpaper collection (74 images, about 68 MB)?"; then
    bash "$APP/bin/get-omarchy-wallpapers" || failed+=("Omarchy wallpapers")
  else
    echo "Skipped. Run ~/.config/nameplate/bin/get-omarchy-wallpapers any time."
  fi
fi

if (( ! TERMINAL_ONLY )) && command -v xdg-mime >/dev/null; then
  step "Default apps for images and PDFs"
  # Ubuntu keeps its defaults where only the GNOME session reads them, so under Hyprland
  # images and PDFs can fall through to a browser. Point them at GNOME's own viewers,
  # but only where the current default is a browser: choices you made yourself stay.
  pick() { local d; for d in "$@"; do [[ -f /usr/share/applications/$d ]] && { echo "$d"; return; }; done; }
  is_browser() { [[ ${1,,} =~ chrome|chromium|firefox|brave|vivaldi|opera|edge|browser ]]; }
  viewer=$(pick org.gnome.Loupe.desktop org.gnome.eog.desktop)
  pdf=$(pick org.gnome.Papers.desktop org.gnome.Evince.desktop)
  changed=0
  for t in image/png image/jpeg image/gif image/webp image/bmp image/tiff image/svg+xml image/x-icon; do
    cur=$(xdg-mime query default "$t" 2>/dev/null)
    if [[ -n $viewer ]] && { [[ -z $cur ]] || is_browser "$cur"; }; then xdg-mime default "$viewer" "$t"; changed=1; fi
  done
  cur=$(xdg-mime query default application/pdf 2>/dev/null)
  if [[ -n $pdf ]] && { [[ -z $cur ]] || is_browser "$cur"; }; then xdg-mime default "$pdf" application/pdf; changed=1; fi
  (( changed )) && echo "images: ${viewer:-unchanged}, PDFs: ${pdf:-unchanged}" || echo "already fine"
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
