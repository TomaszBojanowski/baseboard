#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Pictures of the window alone for the website and README, made from the
# release screenshots in website/screenshots/<version>/ (docs/PROJEKT.md
# §10.7, D74). The release PNGs, which metainfo uses, show the window with
# its shadow; these show it as the openExpress website does: cut out of the
# shadow, without an edge, with transparent rounded corners (15 px at scale
# 1). The website and README draw no frame; the website adds its own shadow.
#
#   python3 -I build-aux/window-screenshots.py website/screenshots/<version> \
#       --scale SCALE --edge PIXELS [--out DIR]
#
#   --scale  the scale the screenshots were taken at (0.2.0: 1.25)
#   --edge   pixels to cut off each side of the window: the light edge that
#            a window has without a compositor (0.2.0: 6; with one: 0)
#   --out    where to write (default: the screenshots' own folder)
#
# Writes <name>.webp, 1400 px wide, for every <name>.png (the website), and
# readme/<name>.png at the window's size at scale 1 for the screenshots
# README shows. Needs Pillow; README pictures go through oxipng when it is
# installed. Never overwrites a file.

import argparse
import pathlib
import shutil
import subprocess
import sys

from PIL import Image, ImageDraw

RADIUS = 15  # window corners at scale 1, as on the openExpress pictures
SITE_WIDTH = 1400
SITE_QUALITY = 90
README_SHOTS = ("overview-en", "overview-dark-en", "fans-en", "fans-dark-en")
SUPERSAMPLE = 4


def window_box(path, img, edge):
    """The opaque window in the screenshot, without its edge."""
    if img.mode != "RGBA":
        sys.exit(f"{path.name}: {img.mode}, expected RGBA (the window with its shadow)")
    box = img.getchannel("A").point(lambda a: 255 if a == 255 else 0).getbbox()
    if box is None:
        sys.exit(f"{path.name}: no opaque window")
    left, top, right, bottom = box
    return (left + edge, top + edge, right - edge, bottom - edge)


def rounded(img, box, size, scale):
    """The window cut out at box, resized to size, with transparent rounded corners."""
    window = img.crop(box).convert("RGB")
    if window.size != size:
        window = window.resize(size, Image.LANCZOS)
    per_point = size[0] / (box[2] - box[0]) * scale  # output pixels per pixel at scale 1
    big = (size[0] * SUPERSAMPLE, size[1] * SUPERSAMPLE)
    mask = Image.new("L", big, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, big[0] - 1, big[1] - 1),
                                           radius=round(RADIUS * per_point * SUPERSAMPLE), fill=255)
    window.putalpha(mask.resize(size, Image.BOX))  # BOX: no ringing outside the corners
    return window


def new_file(path):
    if path.exists():
        sys.exit(f"{path} exists; this never overwrites a file")
    return path


def main():
    parser = argparse.ArgumentParser(description="Pictures of the window alone for the website and README.")
    parser.add_argument("shots", type=pathlib.Path)
    parser.add_argument("--scale", type=float, required=True)
    parser.add_argument("--edge", type=int, required=True)
    parser.add_argument("--out", type=pathlib.Path)
    args = parser.parse_args()
    out = args.out or args.shots
    sources = sorted(args.shots.glob("*.png"))
    if not sources:
        sys.exit(f"{args.shots}: no PNG")

    boxes = {}
    for path in sources:
        with Image.open(path) as img:
            box = window_box(path, img, args.edge)
        boxes[path] = box
    sizes = {(b[2] - b[0], b[3] - b[1]) for b in boxes.values()}
    if len(sizes) != 1:
        sys.exit(f"windows of different sizes: {sorted(sizes)}")
    width, height = sizes.pop()
    site = (SITE_WIDTH, round(SITE_WIDTH * height / width))
    readme = (round(width / args.scale), round(height / args.scale))

    (out / "readme").mkdir(parents=True, exist_ok=True)
    readme_files = []
    for path in sources:
        with Image.open(path) as img:
            img.load()
            web = new_file(out / f"{path.stem}.webp")
            rounded(img, boxes[path], site, args.scale).save(web, "WEBP", quality=SITE_QUALITY, method=6)
            print(f"{web}: {site[0]}×{site[1]}")
            if path.stem in README_SHOTS:
                png = new_file(out / "readme" / path.name)
                rounded(img, boxes[path], readme, args.scale).save(png, "PNG")
                readme_files.append(png)
                print(f"{png}: {readme[0]}×{readme[1]}")

    if shutil.which("oxipng"):
        subprocess.run(["oxipng", "-q", "-o", "max", "--strip", "safe", *map(str, readme_files)], check=True)
    else:
        print("oxipng not found: run oxipng -o max --strip safe on the README pictures", file=sys.stderr)


if __name__ == "__main__":
    main()
