"""Generates the RC Desktop icon: the sidebar logo ("RC" on a blue gradient
tile) above a row of container crates.

    python assets/make_icon.py

Writes assets/rcdesktop.ico (16-256 px) and assets/rcdesktop.png (256 px).
Needs Pillow and the Segoe UI Bold font (Windows).
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).parent
SIZE = 1024  # drawn large, then downscaled for smooth edges
TOP = (0x60, 0xCD, 0xFF)  # Theme.primary
BOTTOM = (0x2F, 0x7F, 0xD8)
FONT = "C:/Windows/Fonts/segoeuib.ttf"


def gradient(size):
    """Diagonal top-left to bottom-right gradient."""
    img = Image.new("RGB", (size, size))
    px = img.load()
    for y in range(size):
        for x in range(size):
            t = (x + y) / (2 * (size - 1))
            px[x, y] = tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM))
    return img


def render(size=SIZE):
    margin = round(size * 0.04)
    radius = round(size * 0.22)

    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (margin, margin, size - margin, size - margin), radius=radius, fill=255
    )
    icon = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    icon.paste(gradient(size), (0, 0), mask)

    draw = ImageDraw.Draw(icon)

    # "RC", centered a little above the middle
    font = ImageFont.truetype(FONT, round(size * 0.42))
    draw.text((size / 2, size * 0.43), "RC", font=font, fill=(255, 255, 255, 255), anchor="mm")

    # A row of three container crates underneath
    box = round(size * 0.12)
    gap = round(size * 0.035)
    row = 3 * box + 2 * gap
    x = (size - row) / 2
    y = size * 0.70
    for i in range(3):
        x1 = x + i * (box + gap)
        draw.rounded_rectangle(
            (x1, y, x1 + box, y + box), radius=round(box * 0.2), fill=(255, 255, 255, 225 if i != 1 else 255)
        )
    return icon


def main():
    big = render()
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    big.resize((256, 256), Image.LANCZOS).save(HERE / "rcdesktop.png")
    big.save(HERE / "rcdesktop.ico", sizes=[(s, s) for s in sizes])
    print("wrote", HERE / "rcdesktop.ico", "and", HERE / "rcdesktop.png")


if __name__ == "__main__":
    main()
