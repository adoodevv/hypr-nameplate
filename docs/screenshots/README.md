# Screenshots

The main README uses these files:

| File | Shows |
|---|---|
| `desktop.png` | Tiled terminals under the top bar, in the default Tokyo Night theme (the hero image) |
| `menu.png` | The main menu (SUPER + SPACE) open over the same desktop |
| `screensaver.gif` | The name animating on the screensaver |

Take them at 1366x768 or larger on an empty workspace, in the default theme and
wallpaper, with nothing personal on screen (media playing shows in the calendar,
tray icons and btop show your apps).

```bash
~/.config/nameplate/bin/theme set tokyo-night --keep-wallpaper
~/.config/nameplate/bin/wallpaper set ~/.config/nameplate/wallpaper.png
grim docs/screenshots/desktop.png                       # the whole screen
~/.config/nameplate/bin/panel menu; sleep 1.5; grim docs/screenshots/menu.png

# the screensaver as a GIF (sudo apt install wf-recorder ffmpeg)
~/.config/nameplate/bin/launch-screensaver & sleep 1
wf-recorder -f /tmp/saver.mp4 & sleep 12; pkill -INT wf-recorder
ffmpeg -i /tmp/saver.mp4 -vf "fps=15,scale=960:-1" docs/screenshots/screensaver.gif
```

Put your own theme and wallpaper back afterwards.
