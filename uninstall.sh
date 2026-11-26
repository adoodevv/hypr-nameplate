#!/bin/bash
# hypr-nameplate uninstaller. Removes the configs this project installed and puts back
# the ones you had before. Other files in those folders, and the installer's backups,
# are left alone. Packages are too; the commands to remove them are printed at the end.
set -u
CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
APP="$CFG/nameplate"
APPS="$HOME/.local/share/applications"
ask() { local a; read -r -p "$1 [y/N] " a; [[ $a == [yY]* ]]; }

[[ $EUID -eq 0 ]] && { echo "Run this as your normal user, not with sudo."; exit 1; }

# The configs the installer wrote ($APP/managed lists them; older installs didn't keep a list)
files=()
if [[ -f $APP/managed ]]; then
  while IFS= read -r line; do files+=("${line:66}"); done < "$APP/managed"
else
  for f in hypr/hyprland.lua hypr/hyprlock.conf hypr/hypridle.conf waybar/config.jsonc \
           waybar/style.css fuzzel/fuzzel.ini mako/config alacritty/alacritty.toml \
           alacritty/screensaver.toml starship.toml xdg-desktop-portal/hyprland-portals.conf; do
    files+=("$CFG/$f")
  done
fi

original() {   # the backup holding what you had before the first install, if any
  [[ -f $APP/originals ]] && awk -F'\t' -v d="$1" '$1 == d { print $2; exit }' "$APP/originals"
}

echo "This removes:"
printf '  %s\n' "${files[@]/#$HOME/\~}"
echo "  ~/.config/nameplate, the screensaver autostart entry and the prompt lines in ~/.bashrc"
echo "Files you had before installing are put back. Your other files in those folders,"
echo "and the installer's backups (*.bak-DATE), are kept."
ask "Continue?" || exit 0

pkill -f "[b]in/idle-watch" 2>/dev/null
pkill -f "nameplate/bin/panel daemon" 2>/dev/null
pkill -f "nameplate/bin/(dock|nameplate-shell dock)$" 2>/dev/null
pkill -f "nameplate/bin/(wallpaper|nameplate-shell wallpaper)$" 2>/dev/null
pkill -f "nameplate/bin/(battery-watch|nameplate battery-watch)" 2>/dev/null
# Put GNOME's fixed-width font back and drop the "monospace" override (bin/font)
if [[ -f $APP/monospace-before ]] && command -v gsettings >/dev/null; then
  gsettings set org.gnome.desktop.interface monospace-font-name "$(cat "$APP/monospace-before")"
fi
rm -f "$CFG/fontconfig/conf.d/60-hypr-nameplate-monospace.conf"

for f in "${files[@]}"; do
  rm -f "$f"
  orig="$(original "$f")"
  if [[ -n $orig && -f $orig ]]; then
    mv "$orig" "$f"
    echo "restored ${f/#$HOME/\~} (your version from before the install)"
  fi
done
[[ -L $CFG/waybar/config && $(readlink "$CFG/waybar/config") == config.jsonc ]] && rm -f "$CFG/waybar/config"

# Launcher entries: delete the copies the installer made; in a copy you already had,
# just undo its edit (and drop it if that leaves it identical to the system's own)
pairs=("org.gnome.Settings.desktop:/usr/share/applications/org.gnome.Settings.desktop"
       "spotify_spotify.desktop:/var/lib/snapd/desktop/applications/spotify_spotify.desktop"
       "spotify.desktop:/usr/share/applications/spotify.desktop")
for f in "$APPS"/notion*.desktop; do
  [[ -f $f ]] && pairs+=("${f##*/}:/var/lib/snapd/desktop/applications/${f##*/}")
done
for pair in "${pairs[@]}"; do
  dst="$APPS/${pair%%:*}" sys="${pair#*:}"
  [[ -f $dst ]] || continue
  if grep -qsxF "$dst" "$APP/created"; then
    rm -f "$dst"; continue
  fi
  sed -i -e 's#^Exec=env XDG_CURRENT_DESKTOP=GNOME #Exec=#' \
         -e 's# --enable-features=UseOzonePlatform --ozone-platform=wayland##' \
         -e 's# --ozone-platform=x11##' "$dst"
  cmp -s "$dst" "$sys" && rm -f "$dst"
done

if [[ -f /etc/apt/apt.conf.d/99-hypr-nameplate-proton-tray ]]; then
  echo "Removing the Proton VPN tray fix's apt hook (needs sudo; Proton keeps the fixed line until it updates)"
  sudo rm -f /etc/apt/apt.conf.d/99-hypr-nameplate-proton-tray /usr/local/lib/hypr-nameplate/fix-proton-tray
fi

rm -rf "$APP"
rm -rf "$HOME/.cache/nameplate/rust-target"   # where install.sh built the Rust program
rm -f "$CFG/autostart/nameplate-screensaver.desktop"
for d in hypr waybar fuzzel mako alacritty xdg-desktop-portal; do
  rmdir "$CFG/$d" 2>/dev/null   # only if nothing else is left in it
done

if grep -q '^# hypr-nameplate prompt$' "$HOME/.bashrc" 2>/dev/null; then
  cp -a "$HOME/.bashrc" "$HOME/.bashrc.bak-uninstall"
  sed -i '/^# hypr-nameplate prompt$/,+2d' "$HOME/.bashrc"
  echo "Removed the prompt lines (backup: ~/.bashrc.bak-uninstall)"
fi

backups="$(find "$CFG" -maxdepth 3 -name '*.bak-[0-9]*' 2>/dev/null | wc -l)"
if (( backups )); then
  cat <<MSG

Kept $backups backups the installer made (files ending in .bak-DATE under ~/.config).
To see them:    find ~/.config -maxdepth 3 -name '*.bak-[0-9]*'
To delete them: find ~/.config -maxdepth 3 -name '*.bak-[0-9]*' -delete
MSG
fi

cat <<'MSG'

Configs removed. To remove the packages as well:

  sudo apt remove hyprland hyprlock hypridle waybar xdg-desktop-portal-hyprland \
                  hyprpolkitagent fuzzel mako-notifier swaybg hyprsunset
  sudo apt install ppa-purge && sudo ppa-purge ppa:cppiber/hyprland

The second line puts Ubuntu's own package versions back (including libinput).
Alacritty, Starship, the Nerd Font and terminaltexteffects are left installed:
  sudo apt remove alacritty; pipx uninstall terminaltexteffects
  rm ~/.local/bin/starship; rm -rf ~/.local/share/fonts/JetBrainsMonoNerd
MSG
