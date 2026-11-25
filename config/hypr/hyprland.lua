-- Hyprland config (Lua, for Hyprland 0.55+), installed by hypr-nameplate
-- Docs: https://wiki.hypr.land/configuring/
-- Hyprland reloads this file automatically when you save it.

local home   = os.getenv("HOME")
local bin    = home .. "/.config/nameplate/bin/"

local terminal    = "alacritty"
local fileManager = "nautilus --new-window"
local apps        = bin .. "panel apps"   -- Omarchy-style app grid (bin/panel)
local mainMenu    = bin .. "panel menu"

-- Colours: this block is rewritten by 'theme set NAME' (SUPER + CTRL + SHIFT + SPACE)
-- theme:begin colors
local accent = "rgba(7aa2f7ee)"
local muted  = "rgba(414868aa)"
local shadow = 0x661a1b26
local backdrop = "rgb(1a1b26)"
-- theme:end

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
  hl.exec_cmd(bin .. "screen-added --init")   -- screens already plugged in at login aren't "new"
  hl.exec_cmd(bin .. "display restore")       -- scale and resolution picked in the display panel
  hl.exec_cmd(bin .. "panel daemon")          -- keeps the panels ready, so bar clicks open instantly
  hl.exec_cmd(bin .. "dock")                  -- app dock along the bottom, tucked away while windows are open
  hl.exec_cmd(bin .. "hardware restore")      -- touchpad / touchscreen left off in the Hardware menu
  hl.exec_cmd("wl-paste --type text --watch cliphist store")    -- clipboard history
  hl.exec_cmd("wl-paste --type image --watch cliphist store")
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

  -- Window groups (SUPER + G), styled like Omarchy's groupbar
  group = {
    col = {
      border_active   = accent,
      border_inactive = muted,
    },
    groupbar = {
      font_size               = 12,
      font_family             = "monospace",
      font_weight_active      = "ultraheavy",
      font_weight_inactive    = "normal",
      indicator_height        = 1,
      indicator_gap           = 5,
      height                  = 22,
      gaps_in                 = 5,
      gaps_out                = 0,
      text_color              = "rgb(ffffff)",
      text_color_inactive     = "rgba(ffffff90)",
      col = {
        active   = "rgba(00000040)",
        inactive = "rgba(00000020)",
      },
      gradients               = true,
      gradient_rounding       = 0,
      gradient_round_only_edges = false,
    },
  },

  decoration = {
    rounding = 0, -- sharp corners, the Omarchy look. Try 8 for rounded.

    -- Omarchy 4 draws no shadows or blur: flat, and lighter on integrated graphics
    shadow = {
      enabled      = false,
      range        = 6,
      render_power = 3,
      color        = shadow,
    },

    blur = {
      enabled = false,
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

  -- The scrolling layout SUPER + L cycles through, at Omarchy's column width
  scrolling = {
    column_width = 0.49,
  },

  binds = {
    hide_special_on_workspace_change = true,
  },

  misc = {
    disable_hyprland_logo    = true,
    disable_splash_rendering = true,
    background_color         = backdrop,
    focus_on_activate        = true,
    key_press_enables_dpms   = true,
    mouse_move_enables_dpms  = true,
  },

  cursor = {
    hide_on_key_press = true,
    inactive_timeout  = 30,   -- hide the pointer after 30 s still, so it's not on the lock screen
  },

  ecosystem = {
    no_update_news  = true,
    no_donation_nag = true,
  },

  xwayland = {
    force_zero_scaling = true,
  },
})

-- Toggles that outlast a reload (bin/toggle): no gaps (SUPER + SHIFT + BACKSPACE)
-- and a square lone window (SUPER + CTRL + BACKSPACE)
do
  local state = home .. "/.local/state/nameplate/"
  local function flag(name)
    local f = io.open(state .. name, "r")
    if f then f:close() return true end
    return false
  end
  if flag("no-gaps") then
    hl.config({ general = { gaps_in = 0, gaps_out = 0, border_size = 0 } })
  end
  if flag("square") then
    hl.config({ layout = { single_window_aspect_ratio = "1 1" } })
  end
  if flag("no-animations") then
    hl.config({ animations = { enabled = false } })
  end
end

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
hl.animation({ leaf = "fadeSwitch",    enabled = false })
-- Omarchy 4 switches workspaces instantly. For a short slide instead:
-- { leaf = "workspaces", enabled = true, speed = 2.5, bezier = "easeOutQuint", style = "slidefade 15%" }
hl.animation({ leaf = "workspaces",    enabled = false })

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
    numlock_by_default = true,

    touchpad = {
      natural_scroll     = false,
      clickfinger_behavior = true,
      scroll_factor      = 0.4,
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

-- Apps and menus (Omarchy's chords)
hl.bind(mod .. " + Return", hl.dsp.exec_cmd(terminal))
hl.bind(mod .. " + Space",  hl.dsp.exec_cmd(mainMenu))
hl.bind(mod .. " + ALT + Space", hl.dsp.exec_cmd(apps))
hl.bind(mod .. " + SHIFT + Return", hl.dsp.exec_cmd(bin .. "browser"))
hl.bind(mod .. " + SHIFT + B", hl.dsp.exec_cmd(bin .. "browser"))
hl.bind(mod .. " + SHIFT + ALT + B", hl.dsp.exec_cmd(bin .. "browser --private"))
hl.bind(mod .. " + SHIFT + F", hl.dsp.exec_cmd(fileManager))
hl.bind(mod .. " + K",      hl.dsp.exec_cmd(bin .. "keys"))
hl.bind(mod .. " + Escape", hl.dsp.exec_cmd(bin .. "panel menu system"))
hl.bind(mod .. " + CTRL + L", hl.dsp.exec_cmd("loginctl lock-session"))
hl.bind(mod .. " + CTRL + S",  hl.dsp.exec_cmd(bin .. "launch-screensaver"))
hl.bind(mod .. " + CTRL + Space", hl.dsp.exec_cmd(bin .. "panel wallpapers"))
hl.bind(mod .. " + CTRL + W", hl.dsp.exec_cmd(bin .. "panel wallpapers"))
hl.bind(mod .. " + CTRL + SHIFT + Space", hl.dsp.exec_cmd(bin .. "panel themes"))
hl.bind(mod .. " + CTRL + T", hl.dsp.exec_cmd(bin .. "panel themes"))
hl.bind(mod .. " + SHIFT + Space", hl.dsp.exec_cmd("pkill -SIGUSR1 waybar"))

-- Panels from the keyboard (same as clicking the bar icons)
hl.bind(mod .. " + CTRL + A", hl.dsp.exec_cmd(bin .. "panel sound"))
hl.bind(mod .. " + CTRL + B", hl.dsp.exec_cmd(bin .. "panel bluetooth"))
hl.bind(mod .. " + CTRL + D", hl.dsp.exec_cmd(bin .. "panel display"))
hl.bind(mod .. " + CTRL + N", hl.dsp.exec_cmd(bin .. "panel wifi"))
hl.bind(mod .. " + CTRL + P", hl.dsp.exec_cmd(bin .. "panel power"))
hl.bind(mod .. " + CTRL + V", hl.dsp.exec_cmd(bin .. "panel clipboard"))
hl.bind(mod .. " + CTRL + E", hl.dsp.exec_cmd(bin .. "panel emoji"))
hl.bind(mod .. " + CTRL + C", hl.dsp.exec_cmd(bin .. "panel menu trigger/capture"))
hl.bind(mod .. " + CTRL + O", hl.dsp.exec_cmd(bin .. "panel menu trigger/toggle"))
hl.bind(mod .. " + CTRL + H", hl.dsp.exec_cmd(bin .. "panel menu setup/hardware"))
hl.bind(mod .. " + CTRL + period", hl.dsp.exec_cmd(bin .. "transcode"))
hl.bind(mod .. " + CTRL + ALT + D", hl.dsp.exec_cmd(bin .. "panel calendar"))
hl.bind(mod .. " + CTRL + ALT + E", hl.dsp.exec_cmd(bin .. "panel calendar clock"))
hl.bind(mod .. " + SHIFT + T", hl.dsp.exec_cmd("alacritty --class nameplate-float --title Activity -e btop"))

-- SUPER + CTRL + 1..9: the bar's panels, left to right (menu, calendar, weather,
-- agents, Bluetooth, sound, network, display, power), like Omarchy's
for i, kind in ipairs({ "menu", "calendar", "weather", "agents", "bluetooth", "sound", "wifi", "display", "power" }) do
  hl.bind(mod .. " + CTRL + " .. i, hl.dsp.exec_cmd(bin .. "panel " .. kind))
end

-- Notices: the time, battery or weather as a notification (bin/notice)
hl.bind(mod .. " + CTRL + ALT + T", hl.dsp.exec_cmd(bin .. "notice time"))
hl.bind(mod .. " + CTRL + ALT + B", hl.dsp.exec_cmd(bin .. "notice battery"))
hl.bind(mod .. " + CTRL + ALT + W", hl.dsp.exec_cmd(bin .. "notice weather"))

-- Reminders (bin/reminder): set one ("20m stretch", "15:30 call home"), list, clear
hl.bind(mod .. " + CTRL + R",         hl.dsp.exec_cmd(bin .. "reminder set"))
hl.bind(mod .. " + CTRL + ALT + R",   hl.dsp.exec_cmd(bin .. "reminder list"))
hl.bind(mod .. " + CTRL + SHIFT + R", hl.dsp.exec_cmd(bin .. "reminder clear"))
hl.bind(mod .. " + CTRL + I", hl.dsp.exec_cmd(bin .. "stay-awake"))      -- no screensaver/lock (Omarchy's key)
hl.bind(mod .. " + CTRL + M", hl.dsp.exec_cmd("wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle; pkill -RTMIN+13 waybar"))

-- Notifications, on Omarchy 4.5's keys: act on the newest (opens its link or app),
-- pick one of its actions from the keyboard, dismiss, history, silence
hl.bind(mod .. " + comma",               hl.dsp.exec_cmd("makoctl invoke"))
hl.bind(mod .. " + ALT + comma",         hl.dsp.exec_cmd("makoctl dismiss"))
hl.bind(mod .. " + SHIFT + comma",       hl.dsp.exec_cmd("makoctl dismiss --all"))
hl.bind(mod .. " + CTRL + comma",        hl.dsp.exec_cmd("makoctl menu fuzzel --dmenu --prompt 'Action  '"))
hl.bind(mod .. " + SHIFT + ALT + comma", hl.dsp.exec_cmd(bin .. "panel menu trigger/notifications"))
hl.bind(mod .. " + CTRL + ALT + comma",  hl.dsp.exec_cmd(bin .. "dnd"))

-- Universal clipboard: SUPER + C / X / V / A copy, cut, paste and select all in
-- every app, terminals included (they're sent as CTRL/SHIFT + INSERT, which
-- terminals and GTK, Qt and Chrome apps all understand)
hl.bind(mod .. " + C", hl.dsp.send_shortcut({ mods = "CTRL",  key = "Insert" }))
hl.bind(mod .. " + X", hl.dsp.send_shortcut({ mods = "SHIFT", key = "Delete" }))
hl.bind(mod .. " + V", hl.dsp.send_shortcut({ mods = "SHIFT", key = "Insert" }))
hl.bind(mod .. " + A", hl.dsp.send_shortcut({ mods = "CTRL",  key = "A" }))

-- Quake console: a terminal that drops down over everything (bin/quake)
hl.bind(mod .. " + grave",         hl.dsp.exec_cmd(bin .. "quake"))
hl.bind(mod .. " + SHIFT + grave", hl.dsp.window.move({ workspace = "special:quake" }))

-- Screens: laptop screen off/on, mirroring, scale (bin/display); magnifier (bin/zoom)
hl.bind(mod .. " + CTRL + Delete",       hl.dsp.exec_cmd(bin .. "display laptop"))
hl.bind(mod .. " + CTRL + ALT + Delete", hl.dsp.exec_cmd(bin .. "display mirror"))
hl.bind(mod .. " + slash",               hl.dsp.exec_cmd(bin .. "display scale-next"))
hl.bind(mod .. " + CTRL + Z",            hl.dsp.exec_cmd(bin .. "zoom in"))
hl.bind(mod .. " + CTRL + ALT + Z",      hl.dsp.exec_cmd(bin .. "zoom out"))

-- Look: window transparency, gaps, a square lone window (bin/toggle)
hl.bind(mod .. " + BackSpace",          hl.dsp.window.set_prop({ prop = "opaque", value = "toggle" }))
hl.bind(mod .. " + SHIFT + BackSpace",  hl.dsp.exec_cmd(bin .. "toggle gaps"))
hl.bind(mod .. " + CTRL + BackSpace",   hl.dsp.exec_cmd(bin .. "toggle square"))

-- Windows
hl.bind(mod .. " + W",       hl.dsp.window.close())
hl.bind(mod .. " + Q",       hl.dsp.window.close())
hl.bind(mod .. " + F",       hl.dsp.window.fullscreen({ mode = "fullscreen" }))
hl.bind(mod .. " + ALT + F", hl.dsp.window.fullscreen({ mode = "maximized" }))
hl.bind(mod .. " + T",       hl.dsp.window.float({ action = "toggle" }))
hl.bind(mod .. " + J",       hl.dsp.layout("togglesplit"))
hl.bind(mod .. " + P",       hl.dsp.window.pseudo())
hl.bind(mod .. " + ALT + P", hl.dsp.window.pseudo())
hl.bind(mod .. " + O",       hl.dsp.exec_cmd(bin .. "window-pop"))
hl.bind(mod .. " + M",       hl.dsp.exec_cmd(bin .. "window-minimize"))
hl.bind(mod .. " + G",       hl.dsp.group.toggle())
hl.bind(mod .. " + ALT + G", hl.dsp.window.move({ out_of_group = true }))
hl.bind(mod .. " + CTRL + Left",  hl.dsp.group.prev())
hl.bind(mod .. " + CTRL + Right", hl.dsp.group.next())
hl.bind(mod .. " + ALT + Tab",    hl.dsp.group.next())
for i = 1, 5 do
  hl.bind(mod .. " + ALT + " .. i, hl.dsp.group.active({ index = i }))
end
hl.bind("CTRL + ALT + Delete",    hl.dsp.exec_cmd(bin .. "window-close-all"))
hl.bind(mod .. " + ALT + Home",   hl.dsp.exec_cmd(bin .. "window-width save"))
hl.bind(mod .. " + Home",         hl.dsp.exec_cmd(bin .. "window-width restore"))
hl.bind("ALT + Tab",         hl.dsp.window.cycle_next())
hl.bind("ALT + Tab",         hl.dsp.window.bring_to_top())
hl.bind("ALT + SHIFT + Tab", hl.dsp.window.cycle_next({ next = false }))
hl.bind("ALT + SHIFT + Tab", hl.dsp.window.alter_zorder({ mode = "bottom" }))

-- Focus and swap with the arrow keys
local directions = { left = "l", right = "r", up = "u", down = "d" }
for key, dir in pairs(directions) do
  hl.bind(mod .. " + " .. key,         hl.dsp.focus({ direction = dir }))
  hl.bind(mod .. " + SHIFT + " .. key, hl.dsp.window.swap({ direction = dir }))
  hl.bind(mod .. " + ALT + " .. key,   hl.dsp.window.move({ into_group = dir }))       -- join that group
  hl.bind(mod .. " + SHIFT + CTRL + " .. key, hl.dsp.window.move({ monitor = dir }))  -- window to that screen
  hl.bind(mod .. " + SHIFT + ALT + " .. key,  hl.dsp.workspace.move({ monitor = dir })) -- workspace to that screen
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
  hl.bind(mod .. " + SHIFT + ALT + " .. key, hl.dsp.window.move({ workspace = i, follow = false }))
end
hl.bind(mod .. " + Tab",         hl.dsp.focus({ workspace = "m+1" }))
hl.bind(mod .. " + SHIFT + Tab", hl.dsp.focus({ workspace = "m-1" }))
hl.bind(mod .. " + CTRL + Tab",  hl.dsp.focus({ workspace = "previous_per_monitor" }))
hl.bind(mod .. " + mouse_down",  hl.dsp.focus({ workspace = "m+1" }))
hl.bind(mod .. " + mouse_up",    hl.dsp.focus({ workspace = "m-1" }))

-- Workspace layout: dwindle -> scrolling -> floating (bin/workspace-layout).
-- Floating workspaces are where the dock lives, like Omarchy's.
hl.bind(mod .. " + L",       hl.dsp.exec_cmd(bin .. "workspace-layout"))
hl.bind(mod .. " + ALT + L", hl.dsp.exec_cmd(bin .. "workspace-layout back"))
hl.bind("XF86Display",       hl.dsp.exec_cmd(bin .. "workspace-layout"))

-- Scratchpad: a hidden workspace you can pop over anything
hl.bind(mod .. " + S",       hl.dsp.workspace.toggle_special("scratchpad"))
hl.bind(mod .. " + ALT + S", hl.dsp.window.move({ workspace = "special:scratchpad" }))

-- Move / resize with the mouse while holding SUPER
hl.bind(mod .. " + mouse:272", hl.dsp.window.drag(),   { mouse = true })
hl.bind(mod .. " + mouse:273", hl.dsp.window.resize(), { mouse = true })

-- Screenshots (saved to ~/Pictures/Screenshots and copied to the clipboard)
hl.bind("Print",         hl.dsp.exec_cmd(bin .. "screenshot region"))
hl.bind("SHIFT + Print", hl.dsp.exec_cmd(bin .. "screenshot full"))

-- Screen recording: same keys again (or click the red dot in the bar) to stop
hl.bind("ALT + Print",          hl.dsp.exec_cmd(bin .. "screenrecord"))
hl.bind("ALT + SHIFT + Print",  hl.dsp.exec_cmd(bin .. "screenrecord full"))
hl.bind("ALT + CTRL + Print",   hl.dsp.exec_cmd(bin .. "screenrecord full mic"))
-- Same thing for laptops where Print needs the Fn key
hl.bind(mod .. " + SHIFT + S", hl.dsp.exec_cmd(bin .. "screenshot region"))
hl.bind(mod .. " + SHIFT + P", hl.dsp.exec_cmd(bin .. "screenshot full"))
hl.bind(mod .. " + CTRL + ALT + C", hl.dsp.exec_cmd(bin .. "screenshot region"))   -- Omarchy's Print-less key

-- Colour picker and text from the screen (OCR), both straight to the clipboard
hl.bind(mod .. " + Print",        hl.dsp.exec_cmd(bin .. "colour-picker"))
hl.bind(mod .. " + CTRL + Print", hl.dsp.exec_cmd(bin .. "ocr"))

-- Volume, brightness and media keys (these also work on the lock screen)
-- Each shows a small level indicator afterwards (bin/osd)
local osd = " && " .. bin .. "osd "
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+" .. osd .. "volume"), { locked = true, repeating = true })
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd("wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-" .. osd .. "volume"),      { locked = true, repeating = true })
hl.bind("XF86AudioMute",        hl.dsp.exec_cmd("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle" .. osd .. "volume"),     { locked = true })
hl.bind("XF86AudioMicMute",     hl.dsp.exec_cmd("wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle" .. osd .. "mic"),      { locked = true })
hl.bind("XF86MonBrightnessUp",   hl.dsp.exec_cmd("brightnessctl -e4 -n2 set 5%+" .. osd .. "brightness"), { locked = true, repeating = true })
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd("brightnessctl -e4 -n2 set 5%-" .. osd .. "brightness"), { locked = true, repeating = true })
-- Finer and coarser steps, like Omarchy: ALT for 1%, SHIFT + brightness for max / min,
-- SHIFT + mute for the next sound output, ALT (+ SHIFT) + play for next (previous) track
hl.bind("ALT + XF86AudioRaiseVolume", hl.dsp.exec_cmd("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 1%+" .. osd .. "volume"), { locked = true, repeating = true })
hl.bind("ALT + XF86AudioLowerVolume", hl.dsp.exec_cmd("wpctl set-volume @DEFAULT_AUDIO_SINK@ 1%-" .. osd .. "volume"),      { locked = true, repeating = true })
hl.bind("ALT + XF86MonBrightnessUp",   hl.dsp.exec_cmd("brightnessctl -n2 set 1%+" .. osd .. "brightness"), { locked = true, repeating = true })
hl.bind("ALT + XF86MonBrightnessDown", hl.dsp.exec_cmd("brightnessctl -n2 set 1%-" .. osd .. "brightness"), { locked = true, repeating = true })
hl.bind("SHIFT + XF86MonBrightnessUp",   hl.dsp.exec_cmd("brightnessctl set 100%" .. osd .. "brightness"), { locked = true })
hl.bind("SHIFT + XF86MonBrightnessDown", hl.dsp.exec_cmd("brightnessctl -n2 set 1%" .. osd .. "brightness"), { locked = true })
hl.bind("SHIFT + XF86AudioMute", hl.dsp.exec_cmd(bin .. "audio-next"), { locked = true })
hl.bind("ALT + XF86AudioPlay",         hl.dsp.exec_cmd("playerctl next"),     { locked = true })
hl.bind("ALT + SHIFT + XF86AudioPlay", hl.dsp.exec_cmd("playerctl previous"), { locked = true })
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

-- Omarchy's gentle transparency for terminals and these desktop apps
hl.window_rule({
  name  = "terminal-opacity",
  match = { class = "^(Alacritty|kitty|com\\.mitchellh\\.ghostty|foot|org\\.codeberg\\.dnkl\\.foot)$" },
  opacity = "0.985 0.96",
})

hl.window_rule({
  name  = "app-opacity",
  match = { class = "^(org\\.gnome\\.Nautilus|1[pP]assword|com\\.onepassword\\.OnePassword|folio)$" },
  opacity = "0.985 0.96",
})

-- Terminals scroll at Omarchy's touchpad rate (their own boost is 1.5;
-- Ghostty's is 0.2, because it already multiplies precise scrolling)
hl.window_rule({
  name  = "terminal-scroll",
  match = { class = "^(Alacritty|kitty|foot|org\\.codeberg\\.dnkl\\.foot)$" },
  scroll_touchpad = 1.5,
})

hl.window_rule({
  name  = "ghostty-scroll",
  match = { class = "^com\\.mitchellh\\.ghostty$" },
  scroll_touchpad = 0.2,
})

-- Popped-out windows (SUPER + O) are the one place with rounded corners
hl.window_rule({
  name  = "popped-window",
  match = { tag = "pop" },
  rounding = 8,
})

-- The screensaver always covers the whole screen and appears without animation
hl.window_rule({
  name  = "screensaver",
  match = { class = "^nameplate-screensaver$" },
  fullscreen = true,
  no_anim    = true,
})

-- The Quake console (SUPER + GRAVE) lives on its own special workspace,
-- a full-width strip along the top
hl.window_rule({
  name  = "quake-console",
  match = { class = "^nameplate-quake$" },
  workspace = "special:quake",
  float = true,
  size  = "monitor_w*0.98 monitor_h*0.45",
  move  = "monitor_w*0.01 monitor_h*0.02",
})

-- Small utility windows float in the middle of the screen, at Omarchy's
-- standard float size
hl.window_rule({
  name  = "floating-utilities",
  match = { class = "^(nameplate-float|org\\.pulseaudio\\.pavucontrol|pavucontrol|xdg-desktop-portal-gtk)$" },
  float  = true,
  center = true,
  size   = "875 600",
})

-- 1Password floats too (and never appears in screen shares)
hl.window_rule({
  name  = "onepassword",
  match = { class = "^(1[pP]assword|com\\.onepassword\\.OnePassword)$" },
  float = true,
  center = true,
  size = "875 600",
  no_screen_share = true,
})

hl.layer_rule({ name = "bar-blur",      match = { namespace = "waybar" },        blur = true, ignore_alpha = 0.3 })
hl.layer_rule({ name = "launcher-blur", match = { namespace = "launcher" },      blur = true, ignore_alpha = 0.3 })
hl.layer_rule({ name = "notify-blur",   match = { namespace = "notifications" }, blur = true, ignore_alpha = 0.3 })

--------------------------------
---- FLOATING WORKSPACES -------
--------------------------------
-- SUPER + L cycles a workspace through dwindle, scrolling and floating
-- (bin/workspace-layout keeps the choice under ~/.local/state/nameplate).
-- Hyprland has no floating layout of its own, so floating is applied to the
-- windows: the ones the mode floated are tagged, and only those are tiled
-- again when the mode ends. Windows on a floating workspace carry a second
-- tag, so carried-in windows and dialogs behave the same. Omarchy also gives
-- floating workspaces titlebars from a Hyprland plugin; that plugin is not
-- packaged for Ubuntu, so floating windows here keep their own frames.

local layout_dir   = home .. "/.local/state/nameplate/workspace-layouts"
local layout_modes = {}
local LAYOUT_FLOATED   = "nameplate-mode-floated"
local LAYOUT_WORKSPACE = "nameplate-floating-workspace"
local LAYOUT_OPENED    = "nameplate-mode-opened"
local LAYOUT_OPEN_RULE = "nameplate-floating-open-"
local layout_cascade = 0
local layout_open_rules = 0
local layout_setup_done = false

local function layout_is_floating(id)
  id = tonumber(id)
  return id ~= nil and id >= 1 and layout_modes[tostring(id)] == "floating"
end

local function layout_any_floating()
  for _, mode in pairs(layout_modes) do
    if mode == "floating" then return true end
  end
  return false
end

local function layout_set_floating(window, floating)
  hl.dispatch(hl.dsp.window.float({ action = floating and "on" or "off", window = window }))
end

local function layout_set_tag(window, tag, on)
  hl.dispatch(hl.dsp.window.tag({ tag = (on and "+" or "-") .. tag, window = window }))
end

local function layout_claimed()
  local addresses = {}
  for _, window in ipairs(hl.get_windows({ tag = LAYOUT_FLOATED })) do
    addresses[window.address] = true
  end
  return addresses
end

-- A window the mode floats opens at a comfortable size in the middle of its
-- monitor, stepped from the last so floated windows do not stack exactly.
local function layout_place(window)
  local monitor = window.monitor
  if not monitor or not monitor.width then return end
  local width  = math.floor(monitor.width / monitor.scale)
  local height = math.floor(monitor.height / monitor.scale)
  if (monitor.transform or 0) % 2 == 1 then width, height = height, width end
  local reserved = monitor.reserved or {}
  width  = width - (reserved.left or 0) - (reserved.right or 0)
  height = height - (reserved.top or 0) - (reserved.bottom or 0)
  hl.dispatch(hl.dsp.window.resize({
    x = math.min(1100, math.floor(width * 0.64)),
    y = math.min(760, math.floor(height * 0.68)),
    relative = false, window = window,
  }))
  hl.dispatch(hl.dsp.window.center({ window = window }))
  layout_cascade = (layout_cascade + 1) % 5
  hl.dispatch(hl.dsp.window.move({
    x = layout_cascade * 24 - 48, y = layout_cascade * 24 - 48,
    relative = true, window = window,
  }))
end

-- Every tiled window is claimed before any is floated: floating one member
-- of a group floats the rest, and a single pass would strand those.
local function layout_float_windows(windows)
  local claiming = {}
  for _, window in ipairs(windows) do
    layout_set_tag(window, LAYOUT_WORKSPACE, true)
    if not window.floating and (window.fullscreen or 0) == 0 then
      layout_set_tag(window, LAYOUT_FLOATED, true)
      table.insert(claiming, window)
    end
  end
  for _, window in ipairs(claiming) do
    layout_set_floating(window, true)
    layout_place(window)
  end
end

local function layout_tile_window(window, mine)
  layout_set_tag(window, LAYOUT_WORKSPACE, false)
  if not mine[window.address] or window.pinned then return end
  layout_set_tag(window, LAYOUT_FLOATED, false)
  layout_set_floating(window, false)
end

local function layout_arrange(windows, workspace)
  local floating, mine = {}, layout_claimed()
  for _, window in ipairs(windows) do
    local id = tonumber(workspace and workspace.id or (window.workspace and window.workspace.id))
    if id and id >= 1 then
      if layout_is_floating(id) then
        table.insert(floating, window)
      else
        layout_tile_window(window, mine)
      end
    end
  end
  layout_float_windows(floating)
end

-- Windows opening on a floating workspace float from their first frame, at
-- the mode's size. Apps with float rules of their own are left to those.
local LAYOUT_OPEN_W = "min(1100,monitor_w*0.64)"
local LAYOUT_OPEN_H = "min(760,monitor_h*0.68)"
local LAYOUT_OWN_FLOAT = "(nameplate-float|org\\.pulseaudio\\.pavucontrol|pavucontrol"
  .. "|xdg-desktop-portal-gtk|1[pP]assword|com\\.onepassword\\.OnePassword)"

local function layout_sync_open_rules()
  local ids = {}
  for id, mode in pairs(layout_modes) do
    local n = tonumber(id)
    if n and n >= 1 and mode == "floating" then table.insert(ids, n) end
  end
  table.sort(ids)
  for i, id in ipairs(ids) do
    local step = layout_cascade * 24 - 48
    hl.window_rule({
      name = LAYOUT_OPEN_RULE .. i,
      enabled = true,
      match = {
        workspace = string.format("r[%d-%d]", id, id),
        modal = false,
        class = "negative:" .. LAYOUT_OWN_FLOAT,
      },
      float = true,
      size = LAYOUT_OPEN_W .. " " .. LAYOUT_OPEN_H,
      move = string.format("(monitor_w-%s)/2+(%d) (monitor_h-%s)/2+(%d)",
                           LAYOUT_OPEN_W, step, LAYOUT_OPEN_H, step),
      tag = "+" .. LAYOUT_OPENED,
    })
  end
  for i = #ids + 1, layout_open_rules do
    hl.window_rule({ name = LAYOUT_OPEN_RULE .. i, enabled = false })
  end
  layout_open_rules = math.max(layout_open_rules, #ids)
end

-- A rule's tag leaves with the rule, so a window the open rule floated is
-- claimed for good as it opens, and tiled again when the mode ends.
local function layout_claim_opened(window)
  for _, tag in ipairs(window.tags or {}) do
    if tag == LAYOUT_OPENED or tag == LAYOUT_OPENED .. "*" then
      layout_set_tag(window, LAYOUT_FLOATED, true)
      layout_cascade = (layout_cascade + 1) % 5
      layout_sync_open_rules()
      return
    end
  end
end

-- A maximized window has a floating workspace to itself until another opens.
local function layout_unmaximize_others(window)
  local id = window.workspace and window.workspace.id
  if not layout_is_floating(id) then return end
  for _, other in ipairs(hl.get_workspace_windows(tostring(id))) do
    if other.address ~= window.address and (other.fullscreen or 0) == 1 then
      hl.dispatch(hl.dsp.window.fullscreen({ mode = "maximized", action = "unset", window = other }))
    end
  end
end

-- Overlapping windows are placed with the pointer: click to focus there.
local function layout_update_input()
  local workspace = hl.get_active_workspace()
  local floating = workspace ~= nil and layout_is_floating(workspace.id)
  hl.config({ input = { follow_mouse = floating and 0 or 1 } })
end

-- What floating workspaces need on top of the flat look: shadows to tell
-- overlapping windows apart, and full opacity over whatever is beneath.
local function layout_setup()
  if layout_setup_done then return end
  layout_setup_done = true
  local ok, shadow_on = pcall(function() return hl.get_config("decoration.shadow.enabled") end)
  if not (ok and shadow_on == true) then
    hl.config({ decoration = { shadow = {
      enabled = true, range = 16, render_power = 3, color = "rgba(00000035)",
    } } })
    hl.window_rule({ name = "floating-shadow-off", match = { class = ".*" }, no_shadow = true })
    hl.window_rule({ name = "floating-shadow-on", match = { tag = LAYOUT_WORKSPACE }, no_shadow = false })
  end
  hl.window_rule({
    name = "floating-workspace-opacity",
    match = { tag = LAYOUT_WORKSPACE },
    opacity = "1.0 override 1.0 override",
  })
end

-- Called by bin/workspace-layout (SUPER + L) through `hyprctl eval`.
function _G.np_workspace_mode(id, mode)
  id = tonumber(id)
  if not id or id < 1 then return end
  if mode ~= "dwindle" and mode ~= "scrolling" and mode ~= "floating" then return end
  if mode ~= "floating" then
    hl.workspace_rule({ workspace = tostring(id), layout = mode })
  end
  layout_modes[tostring(id)] = mode
  if layout_any_floating() then layout_setup() end
  layout_arrange(hl.get_workspace_windows(tostring(id)))
  layout_sync_open_rules()
  layout_update_input()
end

hl.on("window.open", function(window)
  layout_claim_opened(window)
  layout_unmaximize_others(window)
  layout_arrange({ window })
end)

-- A move is still being committed when this fires; let it land first.
hl.on("window.move_to_workspace", function(window, workspace)
  hl.timer(function() layout_arrange({ window }, workspace) end,
           { type = "oneshot", timeout = 1 })
end)

hl.on("window.fullscreen", function(window)
  hl.timer(function() layout_arrange({ window }) end, { type = "oneshot", timeout = 1 })
end)

hl.on("workspace.active", layout_update_input)
hl.on("monitor.focused", layout_update_input)

-- Restore the saved modes, then bring open windows in line with them.
do
  local pipe = io.popen("find '" .. layout_dir .. "' -maxdepth 1 -type f 2>/dev/null")
  if pipe then
    for path in pipe:lines() do
      local id = path:match("(%d+)$")
      local file = id and io.open(path, "r") or nil
      if file then
        local mode = (file:read("*l") or ""):gsub("%s+", "")
        file:close()
        if mode == "dwindle" or mode == "scrolling" or mode == "floating" then
          layout_modes[id] = mode
          if mode ~= "floating" then
            hl.workspace_rule({ workspace = id, layout = mode })
          end
        end
      end
    end
    pipe:close()
  end
  layout_sync_open_rules()
  if layout_any_floating() or #hl.get_windows({ tag = LAYOUT_FLOATED }) > 0
      or #hl.get_windows({ tag = LAYOUT_WORKSPACE }) > 0 then
    layout_setup()
    layout_arrange(hl.get_windows())
    layout_update_input()
  end
end

-- Per-theme Hyprland extras (bin/theme writes this from the theme's own
-- hyprland.lua: Lumon's shadows, Solitude's rounding, and so on).
do
  local extra = home .. "/.config/nameplate/theme-hyprland.lua"
  local handle = io.open(extra, "r")
  if handle then
    handle:close()
    dofile(extra)
  end
end

-------------------------
---- EXTERNAL SCREENS ----
-------------------------

-- A new screen extends the desktop to the right (the monitor rule at the top).
-- Point to SUPER + CTRL + D for mirroring or using one screen.
-- (bin/screen-added ignores the laptop screen being re-detected on wake-up or lid open)
hl.on("monitor.added", function(m)
  hl.exec_cmd(bin .. "screen-added")
end)

-- When a screen is unplugged, go back to the config's layout. This also brings the
-- laptop screen back if it was switched off with "External screen only".
hl.on("monitor.removed", function(m)
  hl.exec_cmd("hyprctl reload")
end)
