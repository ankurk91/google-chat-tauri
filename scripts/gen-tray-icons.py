#!/usr/bin/env python3
"""Generate the tray and unread-count icon sets.

Run from the repo root:  python3 scripts/gen-tray-icons.py

Produces, under src-tauri/icons/tray/:
  {normal,badge,offline}-{16,32}.png  copied from the electron app, unchanged
  count-16/{1..9,9plus}.png           Windows taskbar OVERLAY icons: a standalone
                                      red disc with a white digit. Windows draws
                                      these over the app's own taskbar button.
  count-32/{1..9,9plus}.png           Linux tray icons: the normal artwork with a
                                      count bubble composited into the corner.
                                      (Built on `normal`, not `badge` -- the badge
                                      artwork already carries a red dot, and two
                                      red blobs read as noise at 32px.)
                                      This is the primary unread indicator on
                                      Linux, because Window::set_badge_count is a
                                      no-op there without libunity (see the port
                                      notes on the Linux badge).

Uses Pillow rather than ImageMagick so the script runs on a machine with no
`magick` binary. The output is committed, so this only needs re-running if the
source artwork changes.
"""

import pathlib
import shutil

from PIL import Image, ImageDraw, ImageFont

ELECTRON = pathlib.Path("/home/ankurk/projects/rub/google-chat-electron/resources/icons")
OUT = pathlib.Path("src-tauri/icons/tray")
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"

RED = (234, 67, 53, 255)  # Google red
WHITE = (255, 255, 255, 255)

LABELS = [str(n) for n in range(1, 10)] + ["9+"]


def name_for(label):
    return "9plus" if label == "9+" else label


def fitted_font(draw, label, box, start):
    """Largest font size whose rendered label fits within `box` pixels."""
    size = start
    while size > 5:
        font = ImageFont.truetype(FONT, size)
        l, t, r, b = draw.textbbox((0, 0), label, font=font)
        if (r - l) <= box and (b - t) <= box:
            return font
        size -= 1
    return ImageFont.truetype(FONT, 6)


def draw_bubble(img, label, cx, cy, radius, text_box):
    """Draw a red disc centred on (cx, cy) with `label` centred inside it."""
    draw = ImageDraw.Draw(img)
    draw.ellipse([cx - radius, cy - radius, cx + radius, cy + radius], fill=RED)

    font = fitted_font(draw, label, text_box, radius * 2)
    l, t, r, b = draw.textbbox((0, 0), label, font=font)
    draw.text((cx - (r + l) / 2, cy - (b + t) / 2), label, font=font, fill=WHITE)


def copy_base_icons():
    for kind in ("normal", "badge", "offline"):
        for size in (16, 32):
            shutil.copyfile(ELECTRON / kind / f"{size}.png", OUT / f"{kind}-{size}.png")


def gen_overlay_icons():
    """Windows taskbar overlay: standalone disc filling the whole 16x16."""
    d = OUT / "count-16"
    d.mkdir(parents=True, exist_ok=True)
    for label in LABELS:
        img = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
        draw_bubble(img, label, 8, 8, 8, 13)
        img.save(d / f"{name_for(label)}.png")


def gen_tray_count_icons():
    """Linux tray: normal artwork + a count bubble in the bottom-right corner."""
    d = OUT / "count-32"
    d.mkdir(parents=True, exist_ok=True)
    base = Image.open(ELECTRON / "normal" / "32.png").convert("RGBA")
    for label in LABELS:
        img = base.copy()
        # Punch a transparent ring around the bubble so it stays legible on top
        # of the artwork.
        ImageDraw.Draw(img).ellipse([12, 12, 32, 32], fill=(0, 0, 0, 0))
        draw_bubble(img, label, 22, 22, 9, 15)
        img.save(d / f"{name_for(label)}.png")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    copy_base_icons()
    gen_overlay_icons()
    gen_tray_count_icons()
    print(f"wrote {len(list(OUT.rglob('*.png')))} icons to {OUT}")


if __name__ == "__main__":
    main()
