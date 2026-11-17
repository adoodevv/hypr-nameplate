#!/bin/bash
# hypr-nameplate uninstaller. Removes the configs this project installed.
# Packages are left alone; the commands to remove them are printed at the end.
set -u
CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
ask() { local a; read -r -p "$1 [y/N] " a; [[ $a == [yY]* ]]; }

[[ $EUID -eq 0 ]] && { echo "Run this as your normal user, not with sudo."; exit 1; }

echo "This deletes:"
echo "  $CFG/nameplate  $CFG/hypr  $CFG/waybar  $CFG/fuzzel  $CFG/mako"
echo "  $CFG/alacritty  $CFG/starship.toml  the screensaver autostart entry"
echo "  $CFG/xdg-desktop-portal/hyprland-portals.conf"
echo "  and the prompt lines in ~/.bashrc"
echo "Backups made by the installer (*.bak-DATE) inside those folders go too."
ask "Continue?" || exit 0

pkill -f "[b]in/idle-watch" 2>/dev/null
pkill -f "nameplate/bin/panel daemon" 2>/dev/null
# Put GNOME's fixed-width font back and drop the "monospace" override (bin/font)
if [[ -f $CFG/nameplate/monospace-before ]] && command -v gsettings >/dev/null; then
  gsettings set org.gnome.desktop.interface monospace-font-name "$(cat "$CFG/nameplate/monospace-before")"
fi
rm -f "$CFG/fontconfig/conf.d/60-hypr-nameplate-monospace.conf"
rm -rf "$CFG/nameplate" "$CFG/hypr" "$CFG/waybar" "$CFG/fuzzel" "$CFG/mako" "$CFG/alacritty"
rm -f "$HOME/.local/share/applications/org.gnome.Settings.desktop"
rm -f "$HOME/.local/share/applications/spotify_spotify.desktop" "$HOME/.local/share/applications/spotify.desktop"
rm -f "$HOME"/.local/share/applications/notion*.desktop
if [[ -f /etc/apt/apt.conf.d/99-hypr-nameplate-proton-tray ]]; then
  echo "Removing the Proton VPN tray fix's apt hook (needs sudo; Proton keeps the fixed line until it updates)"
  sudo rm -f /etc/apt/apt.conf.d/99-hypr-nameplate-proton-tray /usr/local/lib/hypr-nameplate/fix-proton-tray
fi
rm -f "$CFG/starship.toml" "$CFG/autostart/nameplate-screensaver.desktop" "$CFG/xdg-desktop-portal/hyprland-portals.conf"

if grep -q '^# hypr-nameplate prompt$' "$HOME/.bashrc" 2>/dev/null; then
  cp -a "$HOME/.bashrc" "$HOME/.bashrc.bak-uninstall"
  sed -i '/^# hypr-nameplate prompt$/,+2d' "$HOME/.bashrc"
  echo "Removed the prompt lines (backup: ~/.bashrc.bak-uninstall)"
fi

cat <<'MSG'

Configs removed. To remove the packages as well:

  sudo apt remove hyprland hyprlock hypridle waybar xdg-desktop-portal-hyprland \
                  hyprpolkitagent fuzzel mako-notifier swaybg
  sudo apt install ppa-purge && sudo ppa-purge ppa:cppiber/hyprland

The second line puts Ubuntu's own package versions back (including libinput).
Alacritty, Starship, the Nerd Font and terminaltexteffects are left installed:
  sudo apt remove alacritty; pipx uninstall terminaltexteffects
  rm ~/.local/bin/starship; rm -rf ~/.local/share/fonts/JetBrainsMonoNerd
MSG
