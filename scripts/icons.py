"""Puts the close-up of the head into the small icon sizes and the logo inside the app.

The full picture turns into a smudge below 64 px, so the title bar, taskbar and Explorer
get the head instead. Run it after `npx tauri icon src-tauri/icons/source/app-icon.png`
from `app`, because that command writes every size from the full picture. Needs Pillow.
"""
from pathlib import Path

from PIL import Image, ImageDraw

ICONS = Path(__file__).resolve().parent.parent / "app" / "src-tauri" / "icons"
SOURCE = ICONS / "source" / "app-icon.png"
LOGO = ICONS.parent.parent / "src" / "logo.png"
# The helmet, eyes and mouth, in the source's 1024 px coordinates.
HEAD = (325, 48, 945, 668)
# Corner radius as a share of the width, matching the source.
RADIUS = 0.22
SMALL = (16, 20, 24, 32, 40, 48)
LARGE = (64, 256)


def rounded(image: Image.Image) -> Image.Image:
    size = image.width * 4
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, size - 1, size - 1), radius=int(size * RADIUS), fill=255)
    out = image.copy()
    out.putalpha(mask.resize(image.size, Image.LANCZOS))
    return out


def main() -> None:
    full = Image.open(SOURCE).convert("RGBA")
    # Fill the source's transparent corners with its background, so the crop never shows them.
    flat = Image.new("RGBA", full.size, full.getpixel((full.width // 2, 20)))
    flat.alpha_composite(full)
    head = rounded(flat.crop(HEAD))

    def sized(size: int) -> Image.Image:
        return (head if size < 64 else full).resize((size, size), Image.LANCZOS)

    frames = [sized(size) for size in SMALL + LARGE]
    frames[-1].save(ICONS / "icon.ico", sizes=[f.size for f in frames], append_images=frames[:-1])
    for name, size in (("32x32.png", 32), ("Square30x30Logo.png", 30), ("Square44x44Logo.png", 44)):
        sized(size).save(ICONS / name, optimize=True)
    # The logo beside the app's name, shown at 20 px, so 64 px stays sharp on high-DPI screens.
    head.resize((64, 64), Image.LANCZOS).save(LOGO, optimize=True)
    print("wrote icon.ico, the small PNGs and the interface logo")


if __name__ == "__main__":
    main()
