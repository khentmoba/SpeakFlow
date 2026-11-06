# SpeakFlow identity (v3)

The mark is the product in one gesture: a voice waveform that settles into a
text caret. Amber is the voice being captured; white is the text it becomes.
It is drawn from the same rules as the interface in `DESIGN.md`: black ground,
one chromatic colour, square caps, no shadows, no gradients.

## Files

| File | Size | Use |
| --- | --- | --- |
| `speakflow-icon.svg` | 512 × 512 | App icon, installer, Store tile. Full-bleed square, no transparent margin. |
| `speakflow-mark.svg` | 394 × 228 | Bare mark on dark surfaces. |
| `speakflow-mark-mono.svg` | 394 × 228 | Single colour via `currentColor`. Title bar, one-colour print. |
| `speakflow-tray.svg` | 512 × 512 | Transparent source for the notification-area icons. |
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

Known limit: Windows picks the taskbar theme, not the app. On a light taskbar
the white caret has no contrast and the amber waveform drops to about 1.6:1.
The mark still reads as an amber waveform, but the caret is lost. Fixing it
needs a light icon set and a theme listener in `setup_system_tray`; until then
this is the tradeoff.

## Regenerating rasters

The SVGs are the masters.

1. `pnpm tauri icon assets/branding-v3/speakflow-icon.svg` writes the app set to
   `src-tauri/icons/` — PNG sizes, `icon.ico`, `icon.icns`, Store and mobile
   assets.
2. `pnpm tauri icon assets/branding-v3/speakflow-tray.svg --png 512 --output <dir>`
   renders the transparent tray source. Copy the result to
   `src-tauri/icons/tray-source.png`.
3. `uv run --with pillow python scripts/generate_tray_icons.py` derives the four
   tray scales, their recording variants, and `icon.ico`.
4. Copy the master to the favicon paths: `speakflow-icon.svg` to
   `assets/icon.svg` and `src-tauri/icons/icon.svg`, and `src-tauri/icons/icon.png`
   to `assets/icon.png`. `tauri icon` does not write SVG.
