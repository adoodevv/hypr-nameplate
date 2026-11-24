-- Kanagawa's Hyprland extras (from Omarchy's themes/kanagawa/hyprland.lua),
-- translated to this project's plain hl.* config. Loaded by hyprland.lua
-- after the base rules while this theme is current (bin/theme).

-- Kanagawa's backdrop is too strong for the default opacity.
hl.window_rule({
  name  = "theme-terminal-opacity",
  match = { class = "^(Alacritty|kitty|com\\.mitchellh\\.ghostty|foot|org\\.codeberg\\.dnkl\\.foot)$" },
  opacity = "0.99 0.985",
})
