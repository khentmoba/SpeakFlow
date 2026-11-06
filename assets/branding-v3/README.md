# SpeakFlow identity (v3)

The mark is the product in one gesture: a voice waveform that settles into a
text caret. Amber is the voice being captured; white is the text it becomes.
It is drawn from the same rules as the interface in `DESIGN.md`: black ground,
one chromatic colour, square caps, no shadows, no gradients.

This folder replaces `assets/branding/` (copper) and `assets/branding-v2`
(navy), both removed once v3 shipped. Git history keeps them.

## Files

| File | Size | Use |
| --- | --- | --- |
| `speakflow-icon.svg` | 512 × 512 | App icon, installer, Store tile. Full-bleed square, no transparent margin. |
| `speakflow-mark.svg` | 394 × 228 | Bare mark on dark surfaces. |
| `speakflow-mark-mono.svg` | 394 × 228 | Single colour via `currentColor`. Title bar, one-colour print. |
| `speakflow-tray.svg` | 512 × 512 | Transparent source for the notification-area icons. |
| `speakflow-tray-light.svg` | 512 × 512 | Same, in the light-taskbar palette. |
| `speakflow-logo-horizontal.svg` | 600 × 160 | Lockup for dark surfaces. |
| `speakflow-logo-horizontal-light.svg` | 600 × 160 | Lockup for white and light neutral surfaces. |
| `speakflow-logo-vertical.svg` | 480 × 480 | Centred lockup for dark surfaces, splash and about screens. |
| `speakflow-logo-vertical-light.svg` | 480 × 480 | Centred lockup for light surfaces. |

## Colour

| Token | Dark | Light |
| --- | --- | --- |
| Voice (waveform) | `#FFB020` | `#A86200` |
| Text (caret, wordmark) | `#FFFFFF` | `#0A0A0A` |
| Ground | `#070707` | `#FFFFFF` |
| Hairline | `#1A1A1A` | `#D4D4CE` |
| Secondary type | `#8A8A8A` | `#565656` |

The amber never gains a second hue. On light surfaces the caret and wordmark go
to ink and the waveform to the darkened amber; do not leave a white caret on
white.

## Rules

- **Clear space** on all sides is half the caret height (100 units at master
  scale), measured from the mark's bounding box.
- **Minimum size** is 16 px for the icon and 24 px for the lockups. Below 24 px
  the wordmark is illegible; use the mark alone.
- **Square.** The icon tile has 0 radius, matching every control in the app. Do
  not round it, add a drop shadow, or place it on a gradient.
- **Do not** redraw the waveform with more or fewer than three extrema, change
  the stroke weight, or close the gap before the caret. The gap is what keeps
  the caret readable at 16 px.
- **Do not** set the wordmark in another face or drop the tracking. The lockups
  use Bahnschrift SemiBold at 0.16 em tracking, the same engraved voice as the
  interface's micro-labels. Bahnschrift ships with Windows; for print, external
  sites, or anywhere the font is not guaranteed, convert the text to outlines
  before export.

## Tray

The tray uses the mark on transparency rather than the opaque app tile, so the
notification area shows the waveform and not a black square. The recording dot
sits at the bottom right, clear of the caret, with a black ring.

Windows draws the notification area in the theme the user picked for Windows,
which can differ from the app theme. The tray therefore ships two palettes:
`tray_icon::taskbar_is_light` reads
`HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\SystemUsesLightTheme`
and picks the set. The dark set keeps the white caret; the light set swaps it
for ink `#0A0A0A` and darkens the waveform to `#A86200`, which holds both above
3:1 on a light taskbar. `setup_system_tray` re-reads the setting whenever the
recording state changes and every 60 seconds, because Windows does not report a
taskbar theme change through Tauri's window theme.

"Show accent color on Start and taskbar" (`ColorPrevalence`) replaces that
background with the accent colour, but Windows only offers the setting in dark
mode: in light mode it is grayed out and the taskbar keeps the light theme,
whatever `ColorPrevalence` still says. So a light taskbar always takes the
light set. In dark mode the accent decides, and `taskbar_is_light` follows its
relative luminance, splitting at 0.3 — where the dark set's white caret reaches
the 3:1 floor. A bright accent such as gold therefore takes the light set even
though Windows is dark, because the white caret measured 1.7:1 on it. The
accent is `DWM\ColorizationColor` (ARGB), with `DWM\AccentColor` (ABGR) as
fallback.

Known limit: the mark is two flat colours, so an accent in the middle of the
luminance range leaves the waveform below 3:1 whichever set is chosen. The
caret clears 3:1 against the accent itself, and it is the element that carries
the mark; a single-colour mark would hold the whole silhouette, at the cost of
the amber. The taskbar is also acrylic over the wallpaper, and the accent
arrives with the transparency `ColorizationColor` carries, so the effective
background is a blend the registry does not describe.

## Regenerating rasters

The SVGs are the masters.

1. `pnpm tauri icon assets/branding-v3/speakflow-icon.svg` writes the app set to
   `src-tauri/icons/` — PNG sizes, `icon.ico`, `icon.icns`, Store and mobile
   assets.
2. `pnpm tauri icon assets/branding-v3/speakflow-tray.svg --png 512 --output <dir>`
   and the same command with `speakflow-tray-light.svg` render the two
   transparent tray sources. Copy the results to
   `src-tauri/icons/tray-source.png` and `src-tauri/icons/tray-source-light.png`.
3. `uv run --with pillow python scripts/generate_tray_icons.py` derives both tray
   sets, their recording variants, and `icon.ico`.
4. Copy the master to the favicon paths: `speakflow-icon.svg` to
   `assets/icon.svg` and `src-tauri/icons/icon.svg`, and `src-tauri/icons/icon.png`
   to `assets/icon.png`. `tauri icon` does not write SVG.
