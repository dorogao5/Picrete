#!/usr/bin/env python3
"""Repair and rebuild a materialized Mistral OCR output without API calls."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from mistral_ocr_to_picrete import normalize_markdown, suspicious


IMAGE_LINK_RE = re.compile(r"(!\[[^\]]*\])\(([^)]+)\)")
EMBEDDED_IMAGE_RE = re.compile(
    r'<img\s+alt="([^"]*)"\s+src="https://i\.imgur\.com/[^\"]+"\s*/?>'
)

# Mistral sometimes emits placeholder imgur URLs for diagrams embedded in a
# table. These pages have been cropped from the source PDF into page images.
LOCAL_TABLE_IMAGE_COUNTS = {
    127: 2,
    176: 6,
    177: 8,
    181: 4,
    182: 4,
    191: 6,
    196: 5,
    206: 7,
    304: 7,
}


def qualify_image_links(markdown: str, doc_id: str, page_number: int) -> str:
    prefix = f"{doc_id}/page_{page_number:04d}"

    def replace(match: re.Match[str]) -> str:
        label, source = match.group(1), match.group(2)
        if source.startswith("images/"):
            source = f"{prefix}/{source}"
        return f"{label}({source})"

    return IMAGE_LINK_RE.sub(replace, markdown)


def localize_embedded_images(markdown: str, page_number: int) -> str:
    limit = LOCAL_TABLE_IMAGE_COUNTS.get(page_number, 0)
    ordinal = 0

    def replace(match: re.Match[str]) -> str:
        nonlocal ordinal
        if ordinal >= limit:
            return match.group(0)
        alt = match.group(1) or f"table image {ordinal}"
        result = f"![{alt}](images/table-{ordinal}.jpg)"
        ordinal += 1
        return result

    return EMBEDDED_IMAGE_RE.sub(replace, markdown)


def apply_source_corrections(markdown: str, page_number: int, doc_id: str) -> str:
    if page_number == 274:
        # The source uses the generic element symbol Э in this reaction;
        # OCR-3 returned the set-existence glyph instead.
        markdown = markdown.replace(r"\exists", r"\mathrm{Э}")
    if page_number == 315:
        for source, corrected in {
            "метаiodной": "метапериодной",
            "Ортоiodную": "Ортопериодную",
            "ортоiodной": "ортопериодной",
            "ортоiodная": "ортопериодная",
            "ортоiodат": "ортопериодат",
        }.items():
            markdown = markdown.replace(source, corrected)
    if doc_id == "Eryomin_physical_chemistry_tasks" and page_number == 246:
        markdown = markdown.replace(r"\mathrmл", r"\mathrm{л}")
    return markdown


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("output_dir", type=Path)
    args = parser.parse_args()
    output_dir = args.output_dir.resolve()
    if not output_dir.is_dir():
        raise SystemExit(f"OCR directory not found: {output_dir}")

    doc_id = output_dir.name
    records = []
    full_parts = []
    quality_pages = []
    for page_dir in sorted(output_dir.glob("page_[0-9][0-9][0-9][0-9]")):
        page_number = int(page_dir.name.rsplit("_", 1)[1])
        page_path = page_dir / "result.mmd"
        markdown = normalize_markdown(page_path.read_text(encoding="utf-8"))
        markdown = localize_embedded_images(markdown, page_number)
        markdown = apply_source_corrections(markdown, page_number, doc_id)
        page_path.write_text(markdown + "\n", encoding="utf-8")
        full_markdown = qualify_image_links(markdown, doc_id, page_number)
        full_parts.append(f"\n\n<!-- PAGE {page_number} -->\n\n{full_markdown}\n")
        findings = suspicious(markdown)
        if findings:
            quality_pages.append({"page": page_number, "findings": findings})
        records.append(
            {
                "doc_id": doc_id,
                "page": page_number,
                "markdown": markdown,
                "images": [
                    f"ocr_output/{doc_id}/{page_dir.name}/images/{image.name}"
                    for image in sorted((page_dir / "images").glob("*.jpg"))
                ],
                "boxed_image": f"ocr_output/{doc_id}/{page_dir.name}/result_with_boxes.jpg",
            }
        )

    index_path = output_dir.parent / f"{doc_id}.index.json"
    old_index = json.loads(index_path.read_text(encoding="utf-8")) if index_path.exists() else []
    old_by_page = {int(record["page"]): record for record in old_index}
    for record in records:
        old = old_by_page.get(record["page"], {})
        record["images"] = sorted(set(old.get("images", []) + record["images"]))
        record["boxed_image"] = old.get("boxed_image", record["boxed_image"])

    index_path.write_text(json.dumps(records, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    full_path = output_dir.parent / f"{doc_id}.full.mmd"
    full_path.write_text("".join(full_parts), encoding="utf-8")

    quality_path = output_dir.parent / f"{doc_id}.quality.json"
    quality = json.loads(quality_path.read_text(encoding="utf-8")) if quality_path.exists() else {}
    quality["pages_returned"] = len(records)
    quality["image_references"] = sum(len(record["images"]) for record in records)
    quality["suspicious_pages"] = quality_pages
    quality_path.write_text(json.dumps(quality, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"repaired pages={len(records)} suspicious={len(quality_pages)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
