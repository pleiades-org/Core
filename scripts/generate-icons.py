"""Regenerate Core's embedded .ico files from the artwork in crates/launcher/assets/source.

Requires Pillow. Run from any directory: python scripts/generate-icons.py
"""
from pathlib import Path

from PIL import Image

ASSETS = Path(__file__).resolve().parent.parent / "crates" / "launcher" / "assets"
SOURCE = ASSETS / "source"
APP_SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
TRAY_SIZES = [16, 20, 24, 32, 40, 48, 64]
# The glyph spans x 153-358, y 113-399 of the 512 px artwork. Tray slots are tiny, so crop
# to the glyph with a small margin instead of shrinking the whole padded square.
TRAY_CROP = (103, 104, 407, 408)


def save_icon(image: Image.Image, name: str, sizes: list[int]) -> None:
    image.save(ASSETS / name, format="ICO", sizes=[(size, size) for size in sizes])


def recolor(glyph: Image.Image, rgb: tuple[int, int, int]) -> Image.Image:
    colored = Image.new("RGBA", glyph.size, rgb + (0,))
    colored.putalpha(glyph.getchannel("A"))
    return colored


def main() -> None:
    save_icon(Image.open(SOURCE / "Core-dark.png").convert("RGBA"), "core.ico", APP_SIZES)
    glyph = Image.open(SOURCE / "Core.png").convert("RGBA").crop(TRAY_CROP)
    save_icon(recolor(glyph, (255, 255, 255)), "tray-white.ico", TRAY_SIZES)
    save_icon(recolor(glyph, (0, 0, 0)), "tray-black.ico", TRAY_SIZES)


if __name__ == "__main__":
    main()
