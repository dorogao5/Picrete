#!/usr/bin/env python3
"""Retry selected OCR pages when the source is available as page images."""

from __future__ import annotations

import argparse
import base64
import io
import os
from pathlib import Path

import requests

from retry_mistral_pages import ocr_image
from mistral_ocr_to_picrete import (
    download_or_decode_image,
    image_names,
    normalize_markdown,
    replace_images,
    replace_tables,
)


def write_page(output_dir: Path, page_number: int, page: dict) -> None:
    page_dir = output_dir / f"page_{page_number:04d}"
    image_dir = page_dir / "images"
    image_dir.mkdir(parents=True, exist_ok=True)
    images = image_names(page)
    for ordinal, (_, item) in enumerate(images):
        content = download_or_decode_image(
            next((item.get(key) for key in ("image_base64", "base64", "image_url", "url") if item.get(key)), None)
        )
        if not content:
            continue
        try:
            from PIL import Image

            with Image.open(io.BytesIO(content)) as image:
                image.convert("RGB").save(image_dir / f"{ordinal}.jpg", "JPEG", quality=92)
        except OSError:
            continue

    markdown = normalize_markdown(str(page.get("markdown") or ""))
    markdown = replace_tables(markdown, [x for x in (page.get("tables") or []) if isinstance(x, dict)])
    markdown = normalize_markdown(markdown)
    markdown = replace_images(markdown, images, None)
    (page_dir / "result.mmd").write_text(markdown + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("image_dir", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("pages", nargs="+", type=int)
    args = parser.parse_args()
    api_key = os.environ.get("MISTRAL_API_KEY")
    if not api_key:
        raise SystemExit("MISTRAL_API_KEY is required")
    headers = {"Authorization": f"Bearer {api_key}"}
    for page_number in args.pages:
        image_path = args.image_dir / f"page_{page_number:04d}.png"
        if not image_path.is_file():
            raise SystemExit(f"source page image not found: {image_path}")
        encoded = base64.b64encode(image_path.read_bytes()).decode("ascii")
        page = ocr_image(image_path, headers)
        write_page(args.output_dir, page_number, page)
        print(f"retried page={page_number}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
