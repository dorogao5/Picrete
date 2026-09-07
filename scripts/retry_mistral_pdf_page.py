#!/usr/bin/env python3
"""Re-run one PDF page through Mistral's document OCR page selector."""

from __future__ import annotations

import argparse
import os
from pathlib import Path

import requests

from mistral_ocr_to_picrete import API, signed_url, upload_pdf
from retry_mistral_pages import materialize_page


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("pdf", type=os.path.abspath)
    parser.add_argument("output_dir", type=os.path.abspath)
    parser.add_argument("page", type=int, help="one-based PDF page number")
    args = parser.parse_args()
    api_key = os.environ.get("MISTRAL_API_KEY")
    if not api_key:
        raise SystemExit("MISTRAL_API_KEY is required")
    headers = {"Authorization": f"Bearer {api_key}"}
    file_id = upload_pdf(Path(args.pdf), headers)
    try:
        url = signed_url(file_id, headers)
        response = requests.post(
            f"{API}/ocr",
            headers={**headers, "Content-Type": "application/json"},
            json={
                "model": "mistral-ocr-2512",
                "document": {"type": "document_url", "document_url": url},
                "pages": str(args.page - 1),
                "table_format": "html",
                "include_image_base64": True,
                "include_blocks": True,
                "confidence_scores_granularity": "block",
            },
            timeout=600,
        )
        if not response.ok:
            raise RuntimeError(f"Mistral PDF retry failed: HTTP {response.status_code}: {response.text[:1000]}")
        pages = response.json().get("pages") or []
        if not pages:
            raise RuntimeError("Mistral PDF retry returned no pages")
        materialize_page(Path(args.pdf), Path(args.output_dir), args.page, pages[0])
        print(f"retried PDF page={args.page}", flush=True)
    finally:
        requests.delete(f"{API}/files/{file_id}", headers=headers, timeout=60)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
