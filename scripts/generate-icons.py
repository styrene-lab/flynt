#!/usr/bin/env python3
"""Generate Flynt application icons from the canonical exported artwork."""

from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
APP_ASSETS = ROOT / "crates/flynt-app/assets"
SOURCE_PNG = APP_ASSETS / "icon-source.png"
MOBILE_ICONSETS = [
    ROOT / "crates/flynt-mobile/assets/AppIcon.appiconset",
    ROOT / "crates/flynt-mobile/assets/Assets.xcassets/AppIcon.appiconset",
]

IOS_SIZES = {
    "icon-20.png": 20,
    "icon-29.png": 29,
    "icon-40.png": 40,
    "icon-58.png": 58,
    "icon-60.png": 60,
    "icon-76.png": 76,
    "icon-80.png": 80,
    "icon-87.png": 87,
    "icon-120.png": 120,
    "icon-152.png": 152,
    "icon-167.png": 167,
    "icon-180.png": 180,
    "icon-1024.png": 1024,
}

IOS_CONTENTS = {
    "images": [
        {"size": "20x20", "idiom": "iphone", "filename": "icon-40.png", "scale": "2x"},
        {"size": "20x20", "idiom": "iphone", "filename": "icon-60.png", "scale": "3x"},
        {"size": "29x29", "idiom": "iphone", "filename": "icon-58.png", "scale": "2x"},
        {"size": "29x29", "idiom": "iphone", "filename": "icon-87.png", "scale": "3x"},
        {"size": "40x40", "idiom": "iphone", "filename": "icon-80.png", "scale": "2x"},
        {"size": "40x40", "idiom": "iphone", "filename": "icon-120.png", "scale": "3x"},
        {"size": "60x60", "idiom": "iphone", "filename": "icon-120.png", "scale": "2x"},
        {"size": "60x60", "idiom": "iphone", "filename": "icon-180.png", "scale": "3x"},
        {"size": "20x20", "idiom": "ipad", "filename": "icon-20.png", "scale": "1x"},
        {"size": "20x20", "idiom": "ipad", "filename": "icon-40.png", "scale": "2x"},
        {"size": "29x29", "idiom": "ipad", "filename": "icon-29.png", "scale": "1x"},
        {"size": "29x29", "idiom": "ipad", "filename": "icon-58.png", "scale": "2x"},
        {"size": "40x40", "idiom": "ipad", "filename": "icon-40.png", "scale": "1x"},
        {"size": "40x40", "idiom": "ipad", "filename": "icon-80.png", "scale": "2x"},
        {"size": "76x76", "idiom": "ipad", "filename": "icon-76.png", "scale": "1x"},
        {"size": "76x76", "idiom": "ipad", "filename": "icon-152.png", "scale": "2x"},
        {"size": "83.5x83.5", "idiom": "ipad", "filename": "icon-167.png", "scale": "2x"},
        {"size": "1024x1024", "idiom": "ios-marketing", "filename": "icon-1024.png", "scale": "1x"},
    ],
    "info": {"version": 1, "author": "flynt"},
}


def load_source() -> Image.Image:
    image = Image.open(SOURCE_PNG).convert("RGBA")
    if image.size != (1024, 1024):
        raise SystemExit(f"{SOURCE_PNG} must be 1024x1024; got {image.size[0]}x{image.size[1]}")
    return crop_export_margin(image).convert("RGB")


def crop_export_margin(image: Image.Image) -> Image.Image:
    background = image.getpixel((0, 0))[:3]
    pixels = image.load()
    xs: list[int] = []
    ys: list[int] = []
    for y in range(image.height):
        for x in range(image.width):
            r, g, b, a = pixels[x, y]
            if a and max(abs(r - background[0]), abs(g - background[1]), abs(b - background[2])) > 12:
                xs.append(x)
                ys.append(y)

    if not xs:
        return image

    left = max(0, min(xs) - 8)
    top = max(0, min(ys) - 8)
    right = min(image.width, max(xs) + 9)
    bottom = min(image.height, max(ys) + 9)
    cropped = image.crop((left, top, right, bottom))

    side = max(cropped.size)
    square = Image.new("RGBA", (side, side), background + (255,))
    square.alpha_composite(cropped, ((side - cropped.width) // 2, (side - cropped.height) // 2))
    return square.resize((1024, 1024), Image.Resampling.LANCZOS)


def save_png(source: Image.Image, path: Path, size: int) -> None:
    resample = Image.Resampling.LANCZOS
    image = source if source.size == (size, size) else source.resize((size, size), resample)
    image.save(path, optimize=True)


def build_icns(source: Image.Image) -> None:
    iconset = APP_ASSETS / "Flynt.iconset"
    if iconset.exists():
        shutil.rmtree(iconset)
    iconset.mkdir()
    for base in (16, 32, 128, 256, 512):
        save_png(source, iconset / f"icon_{base}x{base}.png", base)
        save_png(source, iconset / f"icon_{base}x{base}@2x.png", base * 2)
    subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(APP_ASSETS / "icon.icns")], check=True)
    shutil.rmtree(iconset)


def main() -> None:
    source = load_source()
    save_png(source, APP_ASSETS / "icon.png", 1024)
    save_png(source, APP_ASSETS / "icon-256.png", 256)
    build_icns(source)

    for iconset in MOBILE_ICONSETS:
        iconset.mkdir(parents=True, exist_ok=True)
        for filename, size in IOS_SIZES.items():
            save_png(source, iconset / filename, size)
        (iconset / "Contents.json").write_text(json.dumps(IOS_CONTENTS, indent=2) + "\n")


if __name__ == "__main__":
    main()
