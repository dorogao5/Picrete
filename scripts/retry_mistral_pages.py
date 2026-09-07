#!/usr/bin/env python3
"""Re-run selected PDF pages through Mistral OCR using high-resolution images."""

from __future__ import annotations

import argparse
import base64
import json
import io
import os
import subprocess
from pathlib import Path

import requests

from mistral_ocr_to_picrete import (
    API,
    boxed_page,
    download_or_decode_image,
    image_names,
    normalize_markdown,
    replace_images,
    replace_tables,
)


def render_page(pdf_path: Path, page_number: int) -> Path:
    temp_dir = Path("/tmp/pdfs/ocr-check/mistral-retry")
    temp_dir.mkdir(parents=True, exist_ok=True)
    prefix = temp_dir / f"{pdf_path.stem}-{page_number:04d}"
    existing = sorted(temp_dir.glob(f"{prefix.name}-*.png"))
    if not existing:
        subprocess.run(
            [
                "pdftoppm",
                "-f",
                str(page_number),
                "-l",
                str(page_number),
                "-png",
                "-r",
                "300",
                str(pdf_path),
                str(prefix),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
    rendered = sorted(temp_dir.glob(f"{prefix.name}-*.png"))
    if not rendered:
        raise RuntimeError(f"pdftoppm did not render page {page_number}")
    return rendered[0]


def ocr_image(image_path: Path, headers: dict[str, str]) -> dict:
    encoded = base64.b64encode(image_path.read_bytes()).decode("ascii")
    response = requests.post(
        f"{API}/ocr",
        headers={**headers, "Content-Type": "application/json"},
        json={
            "model": "mistral-ocr-2512",
            "document": {"type": "image_url", "image_url": f"data:image/png;base64,{encoded}"},
            "table_format": "html",
            "include_image_base64": True,
            "include_blocks": True,
            "confidence_scores_granularity": "block",
        },
        timeout=600,
    )
    if not response.ok:
        raise RuntimeError(f"Mistral retry failed: HTTP {response.status_code}: {response.text[:1000]}")
    pages = response.json().get("pages") or []
    if not pages:
        raise RuntimeError("Mistral retry returned no pages")
    return pages[0]


def materialize_page(pdf_path: Path, output_dir: Path, page_number: int, page: dict) -> None:
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
            pass

    markdown = normalize_markdown(str(page.get("markdown") or ""))
    markdown = replace_tables(markdown, [x for x in (page.get("tables") or []) if isinstance(x, dict)])
    markdown = normalize_markdown(markdown)
    markdown = replace_images(markdown, images, None)
    (page_dir / "result.mmd").write_text(markdown + "\n", encoding="utf-8")
    boxed_page(pdf_path, page_number, page, page_dir / "result_with_boxes.jpg")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("pdf", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("pages", nargs="+", type=int)
    args = parser.parse_args()
    api_key = os.environ.get("MISTRAL_API_KEY")
    if not api_key:
        raise SystemExit("MISTRAL_API_KEY is required")
    headers = {"Authorization": f"Bearer {api_key}"}
    for page_number in args.pages:
        image_path = render_page(args.pdf, page_number)
        page = ocr_image(image_path, headers)
        materialize_page(args.pdf, args.output_dir, page_number, page)
        print(f"retried page={page_number}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
