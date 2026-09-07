#!/usr/bin/env python3
"""Run Mistral Document OCR and materialize the Picrete OCR directory format."""

from __future__ import annotations

import argparse
import base64
import html
import io
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

import requests


API = "https://api.mistral.ai/v1"
IMAGE_LINK_RE = re.compile(r"(!\[[^\]]*\])\(([^)]+)\)")
TABLE_LINK_RE = re.compile(r"\[([^\]]*)\]\(([^)]*tbl-[^)]+)\)")


def fail_response(response: requests.Response, action: str) -> None:
    if response.ok:
        return
    body = response.text[:2000]
    raise RuntimeError(f"Mistral {action} failed: HTTP {response.status_code}: {body}")


def upload_pdf(pdf_path: Path, headers: dict[str, str]) -> str:
    with pdf_path.open("rb") as source:
        response = requests.post(
            f"{API}/files",
            headers=headers,
            files={"file": (pdf_path.name, source, "application/pdf")},
            data={"purpose": "ocr", "visibility": "user"},
            timeout=300,
        )
    fail_response(response, "file upload")
    return response.json()["id"]


def signed_url(file_id: str, headers: dict[str, str]) -> str:
    response = requests.get(
        f"{API}/files/{file_id}/url",
        headers=headers,
        params={"expiry": 24},
        timeout=60,
    )
    fail_response(response, "signed URL request")
    return response.json()["url"]


def run_ocr(document_url: str, headers: dict[str, str], model: str) -> dict[str, Any]:
    payload = {
        "model": model,
        "document": {"type": "document_url", "document_url": document_url},
        "table_format": "html",
        "include_image_base64": True,
        "include_blocks": True,
        "confidence_scores_granularity": "block",
    }
    response = requests.post(
        f"{API}/ocr",
        headers={**headers, "Content-Type": "application/json"},
        json=payload,
        timeout=1800,
    )
    fail_response(response, "OCR")
    return response.json()


def parse_data_image(value: str) -> bytes | None:
    if value.startswith("data:"):
        _, encoded = value.split(",", 1)
        return base64.b64decode(encoded)
    try:
        return base64.b64decode(value, validate=True)
    except Exception:
        return None


def download_or_decode_image(value: Any) -> bytes | None:
    if not isinstance(value, str) or not value:
        return None
    decoded = parse_data_image(value)
    if decoded:
        return decoded
    if value.startswith(("http://", "https://")):
        response = requests.get(value, timeout=120)
        if response.ok:
            return response.content
    return None


def image_payload(item: dict[str, Any]) -> bytes | None:
    for key in ("image_base64", "base64", "image_url", "url"):
        value = item.get(key)
        result = download_or_decode_image(value)
        if result:
            return result
    return None


def table_id(item: dict[str, Any]) -> str:
    for key in ("id", "table_id", "name", "filename", "file_name"):
        value = item.get(key)
        if value:
            return str(value)
    return ""


def table_markup(item: dict[str, Any]) -> str | None:
    for key in ("html", "content", "table_html", "markdown", "text", "value"):
        value = item.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return None


def replace_tables(markdown: str, tables: list[dict[str, Any]]) -> str:
    by_id: dict[str, str] = {}
    for item in tables:
        markup = table_markup(item)
        if not markup:
            continue
        identifier = table_id(item)
        if identifier:
            by_id[identifier] = markup
            by_id[Path(identifier).name] = markup

    def replace(match: re.Match[str]) -> str:
        label, source = match.group(1), match.group(2)
        source_name = Path(source).name
        for candidate in (source, source_name, source_name.rsplit(".", 1)[0]):
            if candidate in by_id:
                return by_id[candidate]
        return f"{label}({source})"

    return TABLE_LINK_RE.sub(replace, markdown)


def image_names(page: dict[str, Any]) -> list[tuple[str, dict[str, Any]]]:
    result = []
    for ordinal, item in enumerate(page.get("images") or []):
        if not isinstance(item, dict):
            continue
        identifier = str(item.get("id") or item.get("name") or f"img-{ordinal}.jpeg")
        result.append((identifier, item))
    return result


def replace_images(markdown: str, images: list[tuple[str, dict[str, Any]]], prefix: str | None) -> str:
    mapping: dict[str, str] = {}
    for ordinal, (identifier, _) in enumerate(images):
        target = f"{prefix}/images/{ordinal}.jpg" if prefix else f"images/{ordinal}.jpg"
        mapping[identifier] = target
        mapping[Path(identifier).name] = target
        mapping[f"images/{ordinal}.jpg"] = target

    def replace(match: re.Match[str]) -> str:
        label, source = match.group(1), match.group(2)
        source_name = Path(source).name
        target = mapping.get(source) or mapping.get(source_name)
        if target is None:
            match_number = re.search(r"(?:img|image)[-_]?(\d+)", source_name, re.I)
            if match_number:
                ordinal = int(match_number.group(1))
                if ordinal < len(images):
                    target = f"{prefix}/images/{ordinal}.jpg" if prefix else f"images/{ordinal}.jpg"
        return f"{label}({target or source})"

    return IMAGE_LINK_RE.sub(replace, markdown)


def normalize_markdown(markdown: str) -> str:
    text = html.unescape(markdown).strip()
    text = re.sub(r"^\s*\d+\s*```(?:markdown)?\s*", "", text, flags=re.I)
    text = re.sub(r"\s*```\s*$", "", text)
    # Mistral can emit both LaTeX dollar delimiters and the Picrete-style
    # delimiters. Convert the former even when they occur inside HTML cells.
    text = re.sub(
        r"(?<!\\)\$\$(.*?)(?<!\\)\$\$",
        lambda match: "\\[" + match.group(1) + "\\]",
        text,
        flags=re.S,
    )
    text = re.sub(
        r"(?<!\\)\$([^$\n]*?)(?<!\\)\$(?!\$)",
        lambda match: "\\(" + match.group(1) + "\\)",
        text,
    )
    # An occasional OCR hallucination leaves one unmatched dollar sign at the
    # end of an equation. It is never meaningful in this textbook corpus.
    text = re.sub(r"(?<!\\)\$(?!\$)", "", text)
    # Repair the reversed delimiters seen in a small number of OCR spans.
    text = re.sub(r"\\\)(r\s*[<>=]\s*r_0)\\\(", r"\\(\1\\)", text)
    text = re.sub(r"\\\)(r_0)\\\(", r"\\(\1\\)", text)
    text = re.sub(r"\\\)(E_0)\)", r"\\(\1\\)", text)
    # OCR can put the closing parenthesis of an inline expression before its
    # math delimiter on this page. The source uses e_1, e_2 and E_0 here.
    text = text.replace(r"(\(e_1^*)\)", r"(\(e_1\))")
    text = text.replace(r"(\(e_2^*) —", r"(\(e_2\)) —")
    text = text.replace(r"(\(E_0\).", r"(\(E_0\)).")
    return text.strip()


def page_confidence(page: dict[str, Any]) -> dict[str, Any]:
    confidence = page.get("confidence_scores") or {}
    blocks = page.get("blocks") or []
    block_scores = [
        block.get("confidence_scores", {}).get("average_content_confidence_score")
        for block in blocks
        if isinstance(block, dict)
        and isinstance(block.get("confidence_scores"), dict)
        and isinstance(block["confidence_scores"].get("average_content_confidence_score"), (int, float))
    ]
    return {
        "average_page_confidence_score": confidence.get("average_page_confidence_score"),
        "minimum_page_confidence_score": confidence.get("minimum_page_confidence_score"),
        "average_block_confidence_score": (sum(block_scores) / len(block_scores)) if block_scores else None,
        "minimum_block_confidence_score": min(block_scores) if block_scores else None,
        "blocks": len(blocks),
    }


def render_page(pdf_path: Path, page_number: int, destination: Path) -> Path:
    destination.parent.mkdir(parents=True, exist_ok=True)
    prefix = destination.parent / f".source_page_{page_number:04d}"
    existing = sorted(destination.parent.glob(f"{prefix.name}-*.png"))
    if existing:
        return existing[0]
    subprocess.run(
        [
            "pdftoppm",
            "-f",
            str(page_number),
            "-l",
            str(page_number),
            "-png",
            "-r",
            "150",
            str(pdf_path),
            str(prefix),
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )
    rendered = sorted(destination.parent.glob(f"{prefix.name}-*.png"))
    if not rendered:
        raise RuntimeError(f"pdftoppm did not render page {page_number}")
    return rendered[0]


def boxed_page(pdf_path: Path, page_number: int, page: dict[str, Any], destination: Path) -> None:
    png_path = render_page(pdf_path, page_number, destination)
    try:
        from PIL import Image, ImageDraw
    except ImportError as exc:
        raise RuntimeError("Pillow is required to create result_with_boxes.jpg") from exc

    with Image.open(png_path) as image:
        image = image.convert("RGB")
        draw = ImageDraw.Draw(image)
        dimensions = page.get("dimensions") or {}
        source_width = float(dimensions.get("width") or image.width)
        source_height = float(dimensions.get("height") or image.height)
        scale_x = image.width / source_width
        scale_y = image.height / source_height
        colors = {
            "text": (30, 144, 255),
            "title": (255, 140, 0),
            "table": (220, 20, 60),
            "image": (34, 139, 34),
            "equation": (148, 0, 211),
            "caption": (0, 128, 128),
        }
        for block in page.get("blocks") or []:
            if not isinstance(block, dict):
                continue
            try:
                left = float(block["top_left_x"]) * scale_x
                top = float(block["top_left_y"]) * scale_y
                right = float(block["bottom_right_x"]) * scale_x
                bottom = float(block["bottom_right_y"]) * scale_y
            except (KeyError, TypeError, ValueError):
                continue
            kind = str(block.get("type") or "text")
            color = colors.get(kind, (100, 100, 100))
            draw.rectangle((left, top, right, bottom), outline=color, width=2)
        image.save(destination, "JPEG", quality=88, optimize=True)
    png_path.unlink(missing_ok=True)


def suspicious(markdown: str) -> list[str]:
    findings = []
    probe = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", markdown)
    probe = re.sub(r"\\\(.*?\\\)|\\\[.*?\\\]", "", probe, flags=re.S)
    probe = re.sub(r"\S*_\S*", "", probe)
    if re.search(r"[A-Za-z]{3,}[А-Яа-яЁё]{1,}|[А-Яа-яЁё]{1,}[A-Za-z]{3,}", probe):
        findings.append("mixed_cyrillic_latin")
    if markdown.count("<table") != markdown.count("</table>"):
        findings.append("unbalanced_table_tags")
    if markdown.count("<tr") != markdown.count("</tr>"):
        findings.append("unbalanced_row_tags")
    if markdown.count("<td") != markdown.count("</td>"):
        findings.append("unbalanced_cell_tags")
    if markdown.count("\\(") != markdown.count("\\)"):
        findings.append("unbalanced_inline_math")
    if markdown.count("\\[") != markdown.count("\\]"):
        findings.append("unbalanced_display_math")
    return findings


def materialize(pdf_path: Path, output_dir: Path, response: dict[str, Any]) -> None:
    pages = sorted(response.get("pages") or [], key=lambda page: int(page.get("index", 0)))
    if not pages:
        raise RuntimeError("Mistral returned no pages")

    output_dir.mkdir(parents=True, exist_ok=True)
    doc_id = output_dir.name
    records = []
    full_parts = []
    quality_pages = []
    all_image_count = 0
    all_table_count = 0

    for page in pages:
        page_number = int(page["index"]) + 1
        page_dir = output_dir / f"page_{page_number:04d}"
        image_dir = page_dir / "images"
        image_dir.mkdir(parents=True, exist_ok=True)
        images = image_names(page)
        saved_images: list[int] = []
        for ordinal, (_, item) in enumerate(images):
            content = image_payload(item)
            if not content:
                continue
            try:
                from PIL import Image
                with Image.open(io.BytesIO(content)) as image:
                    image.convert("RGB").save(image_dir / f"{ordinal}.jpg", "JPEG", quality=92, optimize=True)
                saved_images.append(ordinal)
            except Exception as exc:
                print(f"warning: page {page_number} image {ordinal} not saved: {exc}", file=sys.stderr)

        page_markdown = normalize_markdown(str(page.get("markdown") or ""))
        page_markdown = replace_tables(page_markdown, [x for x in (page.get("tables") or []) if isinstance(x, dict)])
        # Tables are returned separately by the API, so normalize once more
        # after inserting their HTML into the page markdown.
        page_markdown = normalize_markdown(page_markdown)
        page_markdown = replace_images(page_markdown, images, None)
        (page_dir / "result.mmd").write_text(page_markdown + "\n", encoding="utf-8")

        boxed_path = page_dir / "result_with_boxes.jpg"
        boxed_page(pdf_path, page_number, page, boxed_path)

        full_markdown = replace_images(page_markdown, images, f"{doc_id}/page_{page_number:04d}")
        full_parts.append(f"\n\n<!-- PAGE {page_number} -->\n\n{full_markdown}\n")
        image_paths = [
            f"ocr_output/{doc_id}/page_{page_number:04d}/images/{ordinal}.jpg"
            for ordinal in saved_images
        ]
        all_image_count += len(image_paths)
        all_table_count += len(page.get("tables") or [])
        findings = suspicious(page_markdown)
        if findings:
            quality_pages.append(
                {
                    "page": page_number,
                    "findings": findings,
                    "confidence": page_confidence(page),
                }
            )
        records.append(
            {
                "doc_id": doc_id,
                "page": page_number,
                "markdown": page_markdown,
                "images": image_paths,
                "boxed_image": f"ocr_output/{doc_id}/page_{page_number:04d}/result_with_boxes.jpg",
            }
        )

    (output_dir.parent / f"{doc_id}.full.mmd").write_text("".join(full_parts), encoding="utf-8")
    (output_dir.parent / f"{doc_id}.index.json").write_text(
        json.dumps(records, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    (output_dir.parent / f"{doc_id}.quality.json").write_text(
        json.dumps(
            {
                "doc_id": doc_id,
                "model": response.get("model"),
                "usage_info": response.get("usage_info"),
                "pages_returned": len(pages),
                "image_references": all_image_count,
                "table_objects": all_table_count,
                "suspicious_pages": quality_pages,
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("pdf", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("--model", default=os.environ.get("MISTRAL_OCR_MODEL", "mistral-ocr-2512"))
    args = parser.parse_args()
    api_key = os.environ.get("MISTRAL_API_KEY")
    if not api_key:
        raise SystemExit("MISTRAL_API_KEY is required")
    if not args.pdf.is_file():
        raise SystemExit(f"PDF not found: {args.pdf}")
    headers = {"Authorization": f"Bearer {api_key}"}
    file_id = upload_pdf(args.pdf, headers)
    print(f"uploaded file_id={file_id}", flush=True)
    try:
        url = signed_url(file_id, headers)
        print("running OCR", flush=True)
        response = run_ocr(url, headers, args.model)
        print(f"received pages={len(response.get('pages') or [])}", flush=True)
        materialize(args.pdf, args.output_dir, response)
    finally:
        delete_response = requests.delete(f"{API}/files/{file_id}", headers=headers, timeout=60)
        if not delete_response.ok:
            print(f"warning: could not delete uploaded file {file_id}", file=sys.stderr)
    print(f"written {args.output_dir}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
