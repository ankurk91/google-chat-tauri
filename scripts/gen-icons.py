#!/usr/bin/env python3
"""Fetch Google Chat's current icons and derive every icon this app ships.

    python3 scripts/gen-icons.py        # from the repo root

Source of truth is Google Chat's own PWA manifest, which is where the app finds
its favicons. That gets us the *current* mark -- the icons inherited from
google-chat-electron were the retired 2020 design, and gstatic's
`productlogos/chat_2020q4` path still serves that old one, so the manifest is
the only reliable place to look.

Handily, Google publishes two variants, which map exactly onto what the tray
needs: `..._no_dot_Npx.png` (idle) and `..._dot_Npx.png` (unread).

Outputs, under src-tauri/icons/:
    source-1024.png          app-icon source; feed to `pnpm tauri icon`
    tray/normal-{16,32}      idle
    tray/badge-{16,32}       unread, Google's own dot variant
    tray/offline-{16,32}     desaturated: shown before the page reports in
    tray/count-16/{1..9,9plus}   Windows taskbar overlay icons

The tray itself only ever shows idle/unread/offline -- the count lives in the
window title and, on macOS/Windows, the dock badge or taskbar overlay. A digit
rendered at 16-32px is hard to read and duplicates the title.

Only re-run this when Google changes the artwork; the output is committed.
"""

import io
import json
import pathlib
import sys
import urllib.request

from PIL import Image, ImageDraw, ImageFont, ImageEnhance

MANIFEST = "https://chat.google.com/manifest.json"
ICONS = pathlib.Path("src-tauri/icons")
TRAY = ICONS / "tray"
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"

RED = (234, 67, 53, 255)
WHITE = (255, 255, 255, 255)
LABELS = [str(n) for n in range(1, 10)] + ["9+"]


def fetch(url):
    with urllib.request.urlopen(url, timeout=30) as r:
        return r.read()


def load_google_icons():
    """Return {size: Image} for the idle and unread variants."""
    manifest = json.loads(fetch(MANIFEST))
    srcs = [i["src"] for i in manifest["icons"]]
    if not srcs:
        sys.exit("manifest listed no icons")

    # The manifest only lists the idle variant; the unread one sits beside it
    # under the same versioned directory.
    idle_tpl = srcs[0].replace("_16px.png", "_{}px.png")
    unread_tpl = idle_tpl.replace("_no_dot_", "_dot_")
    print(f"  source: {idle_tpl}")

    def grab(tpl, size):
        return Image.open(io.BytesIO(fetch(tpl.format(size)))).convert("RGBA")

    sizes = [16, 32, 256]
    return (
        {s: grab(idle_tpl, s) for s in sizes},
        {s: grab(unread_tpl, s) for s in sizes},
    )


def name_for(label):
    return "9plus" if label == "9+" else label


def fitted_font(draw, label, box, start):
    size = start
    while size > 5:
        font = ImageFont.truetype(FONT, size)
        l, t, r, b = draw.textbbox((0, 0), label, font=font)
        if (r - l) <= box and (b - t) <= box:
            return font
        size -= 1
    return ImageFont.truetype(FONT, 6)


def draw_bubble(img, label, cx, cy, radius, text_box):
    draw = ImageDraw.Draw(img)
    draw.ellipse([cx - radius, cy - radius, cx + radius, cy + radius], fill=RED)
    font = fitted_font(draw, label, text_box, radius * 2)
    l, t, r, b = draw.textbbox((0, 0), label, font=font)
    draw.text((cx - (r + l) / 2, cy - (b + t) / 2), label, font=font, fill=WHITE)


def main():
    TRAY.mkdir(parents=True, exist_ok=True)
    print("fetching Google Chat icons...")
    idle, unread = load_google_icons()

    # App icon. 256 is the largest Google publishes and there is no SVG, so the
    # 512/1024 entries in the generated set are upscaled; everything at or below
    # 256 (which is all the deb and tray use) is pixel-exact.
    idle[256].resize((1024, 1024), Image.LANCZOS).save(ICONS / "source-1024.png")

    for size in (16, 32):
        idle[size].save(TRAY / f"normal-{size}.png")
        unread[size].save(TRAY / f"badge-{size}.png")
        # "Not connected yet": the same mark, drained of colour.
        grey = ImageEnhance.Color(idle[size]).enhance(0.0)
        ImageEnhance.Brightness(grey).enhance(0.85).save(TRAY / f"offline-{size}.png")

    # Windows taskbar overlay: a standalone disc, drawn over the app's own icon
    # by the shell.
    d = TRAY / "count-16"
    d.mkdir(exist_ok=True)
    for label in LABELS:
        img = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
        draw_bubble(img, label, 8, 8, 8, 13)
        img.save(d / f"{name_for(label)}.png")

    print(f"wrote {len(list(TRAY.rglob('*.png')))} tray icons + source-1024.png")


if __name__ == "__main__":
    main()
