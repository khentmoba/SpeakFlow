"""Generate SpeakFlow tray images and Windows icon from the app artwork.

Run with: uv run --with pillow python scripts/generate_tray_icons.py
RGBA files feed Tauri without adding a runtime image decoder.

Tray images come from `tray-source.png` and `tray-source-light.png`, the mark on
a transparent canvas in the dark and light palettes, so the notification area
shows the mark rather than the opaque app tile and stays legible on either
taskbar theme. The Windows icon comes from `icon.png`, the full app tile.
"""
from pathlib import Path

from PIL import Image, ImageDraw

ICONS = Path(__file__).resolve().parents[1] / "src-tauri" / "icons"
SIZES = (16, 20, 24, 32)


def main():
    for suffix, source in (("", "tray-source.png"), ("-light", "tray-source-light.png")):
        tray = Image.open(ICONS / source).convert("RGBA")
        for size in SIZES:
            idle = tray.resize((size, size), Image.Resampling.LANCZOS)
            for recording in (False, True):
                image = idle.copy()
                if recording:
                    draw = ImageDraw.Draw(image)
                    draw.ellipse((size * .72, size * .72, size * .98, size * .98),
                                 fill="#EF4444", outline="#000000", width=max(1, size // 16))
                name = f"tray-{size}" + ("-recording" if recording else "") + suffix
                image.save(ICONS / f"{name}.png")
                (ICONS / f"{name}.rgba").write_bytes(image.tobytes())
    app = Image.open(ICONS / "icon.png").convert("RGBA")
    app.save(ICONS / "icon.ico", sizes=[(s, s) for s in (16, 20, 24, 32, 48, 64, 128, 256)])


if __name__ == "__main__":
    main()
