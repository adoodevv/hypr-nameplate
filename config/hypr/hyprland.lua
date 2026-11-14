-- Hyprland config (Lua, for Hyprland 0.55+), installed by hypr-nameplate
-- Docs: https://wiki.hypr.land/configuring/
-- Hyprland reloads this file automatically when you save it.

local home   = os.getenv("HOME")
local bin    = home .. "/.config/nameplate/bin/"

local terminal    = "alacritty"
local fileManager = "nautilus --new-window"
local menu        = "pkill fuzzel || fuzzel"

-- Tokyo Night
local accent = "rgba(7aa2f7ee)"
local muted  = "rgba(414868aa)"

------------------
---- MONITORS ----
------------------

-- scale = 1 keeps a 1080p laptop panel sharp. Try 1.25 if text feels small.
hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1 })

-------------------
---- AUTOSTART ----
-------------------

hl.on("hyprland.start", function()
  hl.exec_cmd("dbus-update-activation-environment --systemd --all")
  hl.exec_cmd(bin .. "wallpaper")      -- double-click the desktop to change it
  hl.exec_cmd("waybar")
  hl.exec_cmd("mako")
  hl.exec_cmd("hypridle")
  hl.exec_cmd(bin .. "battery-watch")
  hl.exec_cmd("playerctld daemon")   -- lets the bar control whichever player was used last
  hl.exec_cmd("systemctl --user start hyprpolkitagent")
end)

-----------------------
---- ENVIRONMENT ------
-----------------------

hl.env("XCURSOR_SIZE", "24")
hl.env("HYPRCURSOR_SIZE", "24")
hl.env("ELECTRON_OZONE_PLATFORM_HINT", "auto")
hl.env("MOZ_ENABLE_WAYLAND", "1")
hl.env("PATH", home .. "/.local/bin:" .. (os.getenv("PATH") or "/usr/local/bin:/usr/bin:/bin"))

-----------------------
---- LOOK AND FEEL ----
-----------------------

hl.config({
  general = {
    gaps_in     = 5,
    gaps_out    = 10,
    border_size = 2,
    col = {
      active_border   = accent,
      inactive_border = muted,
    },
    resize_on_border = true,
    allow_tearing    = false,
    layout           = "dwindle",
  },

  decoration = {
    rounding = 0, -- sharp corners, the Omarchy look. Try 8 for rounded.

    shadow = {
      enabled      = true,
      range        = 6,
      render_power = 3,
      color        = 0x661a1b26,
    },

    -- Kept light for integrated graphics. Set enabled = false if it ever feels slow.
    blur = {
      enabled = true,
      size    = 4,
      passes  = 1,
    },
  },

  animations = {
    enabled = true,
  },

  dwindle = {
    preserve_split = true,
    force_split    = 2, -- new windows open to the right / below
  },

  misc = {
    disable_hyprland_logo    = true,
    disable_splash_rendering = true,
    background_color         = "rgb(1a1b26)",
    focus_on_activate        = true,
    key_press_enables_dpms   = true,
    mouse_move_enables_dpms  = true,
  },

  cursor = {
    hide_on_key_press = true,
  },

  ecosystem = {
    no_update_news  = true,
    no_donation_nag = true,
  },

  xwayland = {
    force_zero_scaling = true,
  },
})

-- Animation curves and timings follow Omarchy's defaults (speed = tenths of a second).
hl.curve("easeOutQuint",   { type = "bezier", points = { { 0.23, 1 },    { 0.32, 1 } } })
hl.curve("easeInOutCubic", { type = "bezier", points = { { 0.65, 0.05 }, { 0.36, 1 } } })
hl.curve("linear",         { type = "bezier", points = { { 0, 0 },       { 1, 1 } } })
hl.curve("almostLinear",   { type = "bezier", points = { { 0.5, 0.5 },   { 0.75, 1.0 } } })
hl.curve("quick",          { type = "bezier", points = { { 0.15, 0 },    { 0.1, 1 } } })

hl.animation({ leaf = "global",        enabled = true, speed = 8,    bezier = "default" })
hl.animation({ leaf = "border",        enabled = true, speed = 4.31, bezier = "easeOutQuint" })
hl.animation({ leaf = "windows",       enabled = true, speed = 3.03, bezier = "easeOutQuint" })
hl.animation({ leaf = "windowsIn",     enabled = true, speed = 3.28, bezier = "easeOutQuint", style = "popin 87%" })
hl.animation({ leaf = "windowsOut",    enabled = true, speed = 1.19, bezier = "linear",       style = "popin 87%" })
hl.animation({ leaf = "fadeIn",        enabled = true, speed = 1.38, bezier = "almostLinear" })
hl.animation({ leaf = "fadeOut",       enabled = true, speed = 1.17, bezier = "almostLinear" })
hl.animation({ leaf = "fade",          enabled = true, speed = 2.42, bezier = "quick" })
hl.animation({ leaf = "layers",        enabled = true, speed = 3.05, bezier = "easeOutQuint" })
hl.animation({ leaf = "layersIn",      enabled = true, speed = 3.2,  bezier = "easeOutQuint", style = "fade" })
hl.animation({ leaf = "layersOut",     enabled = true, speed = 1.2,  bezier = "linear",       style = "fade" })
hl.animation({ leaf = "fadeLayersIn",  enabled = true, speed = 1.43, bezier = "almostLinear" })
hl.animation({ leaf = "fadeLayersOut", enabled = true, speed = 1.11, bezier = "almostLinear" })
-- Omarchy switches workspaces instantly. A short slide is easier to follow when you're new to tiling;
-- change to { leaf = "workspaces", enabled = false } for the Omarchy behaviour.
hl.animation({ leaf = "workspaces",    enabled = true, speed = 2.5,  bezier = "easeOutQuint", style = "slidefade 15%" })

---------------
---- INPUT ----
---------------

hl.config({
  input = {
    kb_layout    = "us",
    follow_mouse = 1,
    sensitivity  = 0,
    repeat_rate  = 40,
    repeat_delay = 250,

    touchpad = {
      natural_scroll       = true,
      tap_to_click         = true,
      disable_while_typing = true,
    },
  },
})

-- Three-finger swipe left/right switches workspace
hl.gesture({ fingers = 3, direction = "horizontal", action = "workspace" })

---------------------
---- KEYBINDINGS ----
---------------------
-- Full list: press SUPER + K, or read ~/.config/nameplate/KEYS.txt

local mod = "SUPER"

-- Apps and menus
hl.bind(mod .. " + Return", hl.dsp.exec_cmd(terminal))
hl.bind(mod .. " + Space",  hl.dsp.exec_cmd(menu))
hl.bind(mod .. " + E",      hl.dsp.exec_cmd(fileManager))
hl.bind(mod .. " + K",      hl.dsp.exec_cmd(bin .. "keys"))
hl.bind(mod .. " + Escape", hl.dsp.exec_cmd(bin .. "power-menu"))
hl.bind(mod .. " + L",      hl.dsp.exec_cmd("loginctl lock-session"))
hl.bind(mod .. " + CTRL + S",  hl.dsp.exec_cmd(bin .. "launch-screensaver"))
hl.bind(mod .. " + CTRL + Space", hl.dsp.exec_cmd(bin .. "wallpaper next"))
hl.bind(mod .. " + CTRL + SHIFT + Space", hl.dsp.exec_cmd(bin .. "panel wallpapers"))
hl.bind(mod .. " + SHIFT + B", hl.dsp.exec_cmd("pkill -SIGUSR1 waybar"))
hl.bind(mod .. " + P",         hl.dsp.exec_cmd(bin .. "display-menu"))

-- Windows
hl.bind(mod .. " + W",       hl.dsp.window.close())
hl.bind(mod .. " + Q",       hl.dsp.window.close())
hl.bind(mod .. " + F",       hl.dsp.window.fullscreen({ mode = "fullscreen" }))
hl.bind(mod .. " + ALT + F", hl.dsp.window.fullscreen({ mode = "maximized" }))
hl.bind(mod .. " + T",       hl.dsp.window.float({ action = "toggle" }))
hl.bind(mod .. " + J",       hl.dsp.layout("togglesplit"))
hl.bind(mod .. " + ALT + P", hl.dsp.window.pseudo())
hl.bind("ALT + Tab",         hl.dsp.window.cycle_next())

-- Focus and swap with the arrow keys
local directions = { left = "l", right = "r", up = "u", down = "d" }
for key, dir in pairs(directions) do
  hl.bind(mod .. " + " .. key,         hl.dsp.focus({ direction = dir }))
  hl.bind(mod .. " + SHIFT + " .. key, hl.dsp.window.swap({ direction = dir }))
end

-- Resize the focused window
hl.bind(mod .. " + minus", hl.dsp.window.resize({ x = -100, y = 0, relative = true }), { repeating = true })
hl.bind(mod .. " + equal", hl.dsp.window.resize({ x = 100,  y = 0, relative = true }), { repeating = true })
hl.bind(mod .. " + SHIFT + minus", hl.dsp.window.resize({ x = 0, y = -100, relative = true }), { repeating = true })
hl.bind(mod .. " + SHIFT + equal", hl.dsp.window.resize({ x = 0, y = 100,  relative = true }), { repeating = true })

-- Workspaces 1-10
for i = 1, 10 do
  local key = i % 10
  hl.bind(mod .. " + " .. key,         hl.dsp.focus({ workspace = i }))
  hl.bind(mod .. " + SHIFT + " .. key, hl.dsp.window.move({ workspace = i }))
end
hl.bind(mod .. " + Tab",         hl.dsp.focus({ workspace = "e+1" }))
hl.bind(mod .. " + SHIFT + Tab", hl.dsp.focus({ workspace = "e-1" }))
hl.bind(mod .. " + mouse_down",  hl.dsp.focus({ workspace = "e+1" }))
hl.bind(mod .. " + mouse_up",    hl.dsp.focus({ workspace = "e-1" }))

-- Scratchpad: a hidden workspace you can pop over anything
hl.bind(mod .. " + S",       hl.dsp.workspace.toggle_special("scratchpad"))
hl.bind(mod .. " + ALT + S", hl.dsp.window.move({ workspace = "special:scratchpad" }))

-- Move / resize with the mouse while holding SUPER
hl.bind(mod .. " + mouse:272", hl.dsp.window.drag(),   { mouse = true })
hl.bind(mod .. " + mouse:273", hl.dsp.window.resize(), { mouse = true })

-- Screenshots (saved to ~/Pictures/Screenshots and copied to the clipboard)
hl.bind("Print",         hl.dsp.exec_cmd(bin .. "screenshot region"))
hl.bind("SHIFT + Print", hl.dsp.exec_cmd(bin .. "screenshot full"))
-- Same thing for laptops where Print needs the Fn key
hl.bind(mod .. " + SHIFT + S", hl.dsp.exec_cmd(bin .. "screenshot region"))
hl.bind(mod .. " + SHIFT + P", hl.dsp.exec_cmd(bin .. "screenshot full"))

-- Volume, brightness and media keys (these also work on the lock screen)
-- Each shows a small level indicator afterwards (bin/osd)
local osd = " && " .. bin .. "osd "
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+" .. osd .. "volume"), { locked = true, repeating = true })
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd("wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-" .. osd .. "volume"),      { locked = true, repeating = true })
hl.bind("XF86AudioMute",        hl.dsp.exec_cmd("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle" .. osd .. "volume"),     { locked = true })
hl.bind("XF86AudioMicMute",     hl.dsp.exec_cmd("wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle" .. osd .. "mic"),      { locked = true })
hl.bind("XF86MonBrightnessUp",   hl.dsp.exec_cmd("brightnessctl -e4 -n2 set 5%+" .. osd .. "brightness"), { locked = true, repeating = true })
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd("brightnessctl -e4 -n2 set 5%-" .. osd .. "brightness"), { locked = true, repeating = true })
hl.bind("XF86AudioNext",  hl.dsp.exec_cmd("playerctl next"),       { locked = true })
hl.bind("XF86AudioPause", hl.dsp.exec_cmd("playerctl play-pause"), { locked = true })
hl.bind("XF86AudioPlay",  hl.dsp.exec_cmd("playerctl play-pause"), { locked = true })
hl.bind("XF86AudioPrev",  hl.dsp.exec_cmd("playerctl previous"),   { locked = true })

--------------------------------
---- WINDOW AND LAYER RULES ----
--------------------------------

hl.window_rule({
  name  = "suppress-maximize-events",
  match = { class = ".*" },
  suppress_event = "maximize",
})

hl.window_rule({
  name  = "fix-xwayland-drags",
  match = { class = "^$", title = "^$", xwayland = true, float = true, fullscreen = false, pin = false },
  no_focus = true,
})

-- Slightly see-through terminal
hl.window_rule({
  name  = "terminal-opacity",
  match = { class = "^Alacritty$" },
  opacity = "0.97 0.92",
})

-- The screensaver always covers the whole screen and appears without animation
hl.window_rule({
  name  = "screensaver",
  match = { class = "^nameplate-screensaver$" },
  fullscreen = true,
  no_anim    = true,
})

-- Small utility windows float in the middle of the screen
hl.window_rule({
  name  = "floating-utilities",
  match = { class = "^(nameplate-float|org\\.pulseaudio\\.pavucontrol|pavucontrol)$" },
  float  = true,
  center = true,
  size   = "900 600",
})

hl.layer_rule({ name = "bar-blur",      match = { namespace = "waybar" },        blur = true, ignore_alpha = 0.3 })
hl.layer_rule({ name = "launcher-blur", match = { namespace = "launcher" },      blur = true, ignore_alpha = 0.3 })
hl.layer_rule({ name = "notify-blur",   match = { namespace = "notifications" }, blur = true, ignore_alpha = 0.3 })

-------------------------
---- EXTERNAL SCREENS ----
-------------------------

-- A new screen extends the desktop to the right (the monitor rule at the top).
-- Point to SUPER + P for mirroring or using one screen.
hl.on("monitor.added", function(m)
  hl.exec_cmd("notify-send -a Display 'Screen connected' 'Press SUPER + P to mirror or change the layout'")
end)

-- When a screen is unplugged, go back to the config's layout. This also brings the
-- laptop screen back if it was switched off with "External screen only".
hl.on("monitor.removed", function(m)
  hl.exec_cmd("hyprctl reload")
end)
