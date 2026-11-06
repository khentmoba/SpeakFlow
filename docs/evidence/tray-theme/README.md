# Tray theme

The notification-area mark on a Windows 11 light taskbar (`#F3F3F3`) and dark
taskbar (`#202020`), at the four sizes the tray picks from. The top row is the
single icon set before this change, the middle row is the new light set, and the
bottom row is the unchanged dark set.

Each tile is a committed `src-tauri/icons/tray-*.png` composited on the taskbar
background and scaled 10× with nearest-neighbour, so the pixels are the shipped
ones. The ratios are WCAG contrast for the solid pixels against that background:
the white caret is the worst case in the top row and the best in the bottom one.

## Accent taskbar

`accent.png` covers Personalization → "Show accent color on Start and taskbar".
Windows only offers that setting in dark mode, so the accent can only replace a
dark taskbar. The rows are the default blue accent (unchanged), then the gold
accent before and after the change: the theme alone picks the dark set, and the
accent's luminance picks the light one. Those ratios use the palette colours the
512px tray sources are filled with, so they are the design values and not
resampling noise. The flat `#0078D4` and `#FFB900` patches are the accent
without the acrylic blend Windows puts over the wallpaper.
