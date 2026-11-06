# Tray theme

The notification-area mark on a Windows 11 light taskbar (`#F3F3F3`) and dark
taskbar (`#202020`), at the four sizes the tray picks from. The top row is the
single icon set before this change, the middle row is the new light set, and the
bottom row is the unchanged dark set.

Each tile is a committed `src-tauri/icons/tray-*.png` composited on the taskbar
background and scaled 10× with nearest-neighbour, so the pixels are the shipped
ones. The ratios are WCAG contrast for the solid pixels against that background:
the white caret is the worst case in the top row and the best in the bottom one.
