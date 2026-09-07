#!/usr/bin/env python3
"""Build a non-destructive, canonical catalog of the chemistry OCR assets."""

from __future__ import annotations

import hashlib
import json
import re
import shutil
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any


DATA_ROOT = Path("/Users/doroga/Documents/projects/picrete/LLM+RAG/data")
OCR_ROOT = DATA_ROOT / "ocr_output"
LIBRARY_ROOT = DATA_ROOT / "unified_ocr_library"
CHEMRAG_ROOT = Path("/Users/doroga/Downloads/ChemRAG")
ARCHIVE_ROOT = DATA_ROOT / "archives"

IMAGE_LINK_RE = re.compile(r"(!\[[^\]]*\])\(([^)]+)\)")
MOJIBAKE_MARKERS = "ÃÐÑÒÓÔÕÖØÚÛÜÝÞàáâãäåæçèéêëìíîïðñòóôõö÷øùúûüýþ"


@dataclass(frozen=True)
class DocumentSpec:
    doc_id: str
    title: str
    source_pdf: Path | None
    raw_dir: Path
    index_json: Path | None = None
    full_mmd: Path | None = None
    quality_json: Path | None = None
    rebuild: bool = False
    legacy: bool = False
    source_note: str = ""


def ensure_new(path: Path) -> None:
    if path.exists() or path.is_symlink():
        raise RuntimeError(f"Refusing to overwrite existing path: {path}")


def link_or_copy(source: Path, destination: Path, *, copy: bool = False) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    ensure_new(destination)
    if copy:
        if source.is_dir():
            shutil.copytree(source, destination, symlinks=True)
        else:
            shutil.copy2(source, destination)
        return
    destination.symlink_to(source, target_is_directory=source.is_dir())


def fix_mojibake(value: str) -> str:
    """Repair the common UTF-8-as-Windows-1252 corruption in old OCR text."""
    text = value
    for _ in range(2):
        marker_count = sum(text.count(char) for char in MOJIBAKE_MARKERS)
        if marker_count < 3:
            break
        try:
            candidate = text.encode("latin1").decode("utf-8")
        except (UnicodeEncodeError, UnicodeDecodeError):
            break
        candidate_count = sum(candidate.count(char) for char in MOJIBAKE_MARKERS)
        if candidate_count >= marker_count:
            break
        text = candidate
    return text


def page_local_markdown(markdown: str) -> str:
    def replace(match: re.Match[str]) -> str:
        label, source = match.groups()
        if source.startswith(("http://", "https://", "data:")):
            return match.group(0)
        if "/images/" in source:
            return f"{label}(images/{source.rsplit('/images/', 1)[1]})"
        if source.startswith("images/"):
            return match.group(0)
        return match.group(0)

    return IMAGE_LINK_RE.sub(replace, markdown)


def canonical_image_path(source: str, doc_id: str, page: int) -> str:
    if source.startswith(("http://", "https://", "data:")):
        return source
    name = source.rsplit("/images/", 1)[-1]
    if name == source:
        name = Path(source).name
    return f"ocr_output/{doc_id}/page_{page:04d}/images/{name}"


def full_image_path(source: str, doc_id: str, page: int) -> str:
    if source.startswith(("http://", "https://", "data:")):
        return source
    name = source.rsplit("/images/", 1)[-1]
    if name == source:
        name = Path(source).name
    return f"{doc_id}/page_{page:04d}/images/{name}"


def full_markdown(markdown: str, doc_id: str, page: int, asset_root: Path | None = None) -> str:
    def replace(match: re.Match[str]) -> str:
        label, source = match.groups()
        target = full_image_path(source, doc_id, page)
        if asset_root is not None and not (asset_root / target).exists():
            fallback = f"{doc_id}/page_{page:04d}/result_with_boxes.jpg"
            if (asset_root / fallback).exists():
                return f"{label}({fallback})"
            return f"<!-- OCR image unavailable: {source} -->"
        return f"{label}({target})"

    return IMAGE_LINK_RE.sub(replace, markdown)


def render_page_as_jpeg(pdf_path: Path, page: int, destination: Path) -> bool:
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="picrete_page_") as temp_dir:
        prefix = Path(temp_dir) / "page"
        result = subprocess.run(
            [
                "pdftoppm",
                "-f",
                str(page),
                "-l",
                str(page),
                "-jpeg",
                "-r",
                "150",
                str(pdf_path),
                str(prefix),
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            check=False,
        )
        if result.returncode != 0:
            return False
        rendered = sorted(Path(temp_dir).glob("page-*.jpg"))
        if not rendered:
            return False
        shutil.copy2(rendered[0], destination)
        return True


def source_page_dir(raw_dir: Path, page: int) -> Path:
    return raw_dir / f"page_{page:04d}"


def materialize_legacy_pages(
    raw_dir: Path,
    destination_dir: Path,
    records: list[dict[str, Any]],
    source_pdf: Path | None,
) -> list[int]:
    generated_pages: list[int] = []
    for record in records:
        page = int(record["page"])
        target_page = destination_dir / f"page_{page:04d}"
        target_page.mkdir(parents=True, exist_ok=True)
        markdown = page_local_markdown(fix_mojibake(str(record.get("markdown") or "")).strip())
        (target_page / "result.mmd").write_text(markdown + "\n", encoding="utf-8")

        raw_page = source_page_dir(raw_dir, page)
        raw_box = raw_page / "result_with_boxes.jpg"
        target_box = target_page / "result_with_boxes.jpg"
        if target_box.exists() or target_box.is_symlink():
            pass
        elif raw_box.is_file():
            target_box.symlink_to(raw_box)
        elif source_pdf is not None and render_page_as_jpeg(source_pdf, page, target_box):
            generated_pages.append(page)

        image_names = []
        for image_path in record.get("images") or []:
            image_names.append(Path(str(image_path)).name)
        for match in IMAGE_LINK_RE.finditer(markdown):
            source = match.group(2)
            if not source.startswith(("http://", "https://", "data:")):
                image_names.append(Path(source).name)
        for image_name in sorted(set(image_names)):
            target_image = target_page / "images" / image_name
            target_image.parent.mkdir(parents=True, exist_ok=True)
            raw_image = raw_page / "images" / image_name
            if target_image.exists() or target_image.is_symlink():
                continue
            if raw_image.is_file():
                target_image.symlink_to(raw_image)
            elif raw_box.is_file():
                target_image.symlink_to(raw_box)
            elif source_pdf is not None:
                if render_page_as_jpeg(source_pdf, page, target_image):
                    generated_pages.append(page)
    return sorted(set(generated_pages))


def normalize_records(records: list[dict[str, Any]], doc_id: str) -> tuple[list[dict[str, Any]], list[str]]:
    normalized: list[dict[str, Any]] = []
    external_images: list[str] = []
    for original in records:
        page = int(original["page"])
        markdown = fix_mojibake(str(original.get("markdown") or "")).strip()
        images = [canonical_image_path(str(path), doc_id, page) for path in original.get("images") or []]
        boxed = original.get("boxed_image")
        boxed_image = (
            f"ocr_output/{doc_id}/page_{page:04d}/result_with_boxes.jpg"
            if boxed
            else None
        )
        for match in IMAGE_LINK_RE.finditer(markdown):
            source = match.group(2)
            if source.startswith(("http://", "https://")):
                external_images.append(source)
        normalized.append(
            {
                "doc_id": doc_id,
                "page": page,
                "markdown": page_local_markdown(markdown),
                "images": images,
                "boxed_image": boxed_image,
            }
        )
    normalized.sort(key=lambda record: int(record["page"]))
    return normalized, sorted(set(external_images))


def write_derived_files(
    root: Path,
    spec: DocumentSpec,
    records: list[dict[str, Any]],
    source_records: Path,
) -> dict[str, Any]:
    normalized, external_images = normalize_records(records, spec.doc_id)
    full_parts = []
    for record in normalized:
        page = int(record["page"])
        full_parts.append(
            f"\n\n<!-- PAGE {page} -->\n\n"
            f"{full_markdown(record['markdown'], spec.doc_id, page, root)}\n"
        )
    (root / f"{spec.doc_id}.full.mmd").write_text("".join(full_parts), encoding="utf-8")
    (root / f"{spec.doc_id}.index.json").write_text(
        json.dumps(normalized, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )

    source_model = None
    if source_records.suffix == ".json":
        try:
            payload = json.loads(source_records.read_text(encoding="utf-8"))
            if isinstance(payload, dict):
                source_model = payload.get("model")
        except (OSError, json.JSONDecodeError):
            pass
    quality = {
        "doc_id": spec.doc_id,
        "model": source_model or "legacy-source-index",
        "pages_returned": len(normalized),
        "image_references": sum(len(record["images"]) for record in normalized),
        "table_objects": None,
        "suspicious_pages": [],
        "external_image_references": external_images,
        "source_index": str(source_records),
    }
    (root / f"{spec.doc_id}.quality.json").write_text(
        json.dumps(quality, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return {
        "pages": len(normalized),
        "images": quality["image_references"],
        "external_image_references": len(external_images),
        "source_index": str(source_records),
    }


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def relevant_files() -> list[Path]:
    roots = [Path("/Users/doroga/Downloads"), Path("/Users/doroga/Documents")]
    tokens = (
        "tretyakov",
        "третьяк",
        "sviridov",
        "свирид",
        "eryomin",
        "еремин",
        "lab_practice",
        "практикум",
        "ahmetov",
        "ахмет",
        "неорган",
    )
    candidates: set[Path] = set()
    for root in roots:
        if not root.exists():
            continue
        for pattern in ("*.full.mmd", "*.pdf"):
            for path in root.rglob(pattern):
                if LIBRARY_ROOT in path.parents or ARCHIVE_ROOT in path.parents:
                    continue
                lowered = str(path).lower()
                if pattern == "*.full.mmd" or any(token in lowered for token in tokens):
                    candidates.add(path)
    return sorted(candidates)


def write_duplicate_report(destination: Path) -> dict[str, Any]:
    files = [path for path in relevant_files() if path.is_file()]
    by_hash: dict[str, list[Path]] = {}
    for path in files:
        try:
            by_hash.setdefault(sha256(path), []).append(path)
        except OSError:
            continue
    duplicate_groups = [paths for paths in by_hash.values() if len(paths) > 1]
    lines = [
        "# Exact duplicate report",
        "",
        "This report is informational. No source files were deleted or moved.",
        "",
        "## Exact duplicate groups",
    ]
    if not duplicate_groups:
        lines.append("No exact duplicate files were found in the scanned relevant set.")
    else:
        for paths in sorted(duplicate_groups, key=lambda group: str(group[0])):
            digest = sha256(paths[0])
            lines.append(f"\nSHA256 `{digest}`")
            lines.extend(f"- `{path}`" for path in paths)
    lines.extend(
        [
            "",
            "## Known same-size variants",
            "- Official Tretyakov volume 1 PDFs in `Downloads` and `Downloads/archive` have the same size and page count but different SHA256 values; both originals are retained.",
            "- Official Tretyakov volume 3 book 2 PDFs in `Downloads` and `Downloads/archive` have the same page count but different SHA256 values; the archive copy is used as the canonical source.",
        ]
    )
    destination.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return {
        "scanned_files": len(files),
        "exact_duplicate_groups": len(duplicate_groups),
        "exact_duplicate_files": sum(len(group) for group in duplicate_groups),
    }


def build_archive(specs: list[DocumentSpec]) -> Path:
    ARCHIVE_ROOT.mkdir(parents=True, exist_ok=True)
    archive_path = ARCHIVE_ROOT / "Tretyakov_official_volumes_1_2.tar.gz"
    ensure_new(archive_path)
    t1 = next(spec for spec in specs if spec.doc_id == "Tretyakov_1_official")
    t2 = next(spec for spec in specs if spec.doc_id == "Tretyakov_2_official")
    stage_parent = Path(tempfile.mkdtemp(prefix="picrete_tretyakov_archive_"))
    stage = stage_parent / "Tretyakov_official_volumes_1_2"
    try:
        (stage / "ocr_output").mkdir(parents=True)
        (stage / "sources").mkdir(parents=True)
        readme = """# Третьяков, Неорганическая химия: тома 1–2

Архив содержит исходные PDF и полный OCR-результат в формате Picrete.

Структура:

- `sources/Tretyakov_1/source.pdf` и `sources/Tretyakov_2/source.pdf` — исходные PDF;
- `ocr_output/Tretyakov_1_official/` и `ocr_output/Tretyakov_2_official/` — страницы, изображения и `result_with_boxes.jpg`;
- рядом с каталогами OCR лежат `*.full.mmd`, `*.index.json` и `*.quality.json`.

Ссылки на изображения внутри MMD относительные и рассчитаны на корень архива.
Задачник Ерёмина в этот архив намеренно не включен.
"""
        (stage / "README.md").write_text(readme, encoding="utf-8")
        for spec in (t1, t2):
            assert spec.source_pdf is not None
            source_dir = stage / "sources" / spec.doc_id.removesuffix("_official")
            source_dir.mkdir(parents=True)
            shutil.copy2(spec.source_pdf, source_dir / "source.pdf")
            shutil.copytree(spec.raw_dir, stage / "ocr_output" / spec.doc_id, symlinks=True)
            for suffix in ("full.mmd", "index.json", "quality.json"):
                source = OCR_ROOT / f"{spec.doc_id}.{suffix}"
                shutil.copy2(source, stage / "ocr_output" / f"{spec.doc_id}.{suffix}")
        manifest = {
            "archive": archive_path.name,
            "documents": [
                {
                    "doc_id": spec.doc_id,
                    "source_pdf": str(spec.source_pdf),
                    "source_sha256": sha256(spec.source_pdf) if spec.source_pdf else None,
                    "pages": json.loads((OCR_ROOT / f"{spec.doc_id}.quality.json").read_text(encoding="utf-8")).get("pages_returned"),
                }
                for spec in (t1, t2)
            ],
        }
        (stage / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        subprocess.run(["tar", "-czf", str(archive_path), "-C", str(stage_parent), stage.name], check=True)
    finally:
        shutil.rmtree(stage_parent, ignore_errors=True)
    return archive_path


def main() -> int:
    specs = [
        DocumentSpec(
            "Tretyakov_1_official",
            "Третьяков, Неорганическая химия, том 1",
            Path("/Users/doroga/Downloads/archive/Nkh_Neorganicheskaya_Khimia_T_1_Pod_Red_Yu_d_Tretyakova.pdf"),
            OCR_ROOT / "Tretyakov_1_official",
            OCR_ROOT / "Tretyakov_1_official.index.json",
            OCR_ROOT / "Tretyakov_1_official.full.mmd",
            OCR_ROOT / "Tretyakov_1_official.quality.json",
        ),
        DocumentSpec(
            "Tretyakov_2_official",
            "Третьяков, Неорганическая химия, том 2",
            Path("/Users/doroga/Downloads/archive/Nkh_Neorganicheskaya_Khimia_T_2_Pod_Red_Yu_d_Tretyakova.pdf"),
            OCR_ROOT / "Tretyakov_2_official",
            OCR_ROOT / "Tretyakov_2_official.index.json",
            OCR_ROOT / "Tretyakov_2_official.full.mmd",
            OCR_ROOT / "Tretyakov_2_official.quality.json",
        ),
        DocumentSpec(
            "Tretyakov_3_1",
            "Третьяков, Неорганическая химия, том 3, книга 1",
            Path("/Users/doroga/Downloads/archive/Nkh_Neorganicheskaya_khimia_T_3_Kn_1_Pod_red_Yu_D_Tretyakova.pdf"),
            OCR_ROOT / "Tretyakov_3_1",
            OCR_ROOT / "Tretyakov_3_1.index.json",
            OCR_ROOT / "Tretyakov_3_1.full.mmd",
            None,
            rebuild=True,
        ),
        DocumentSpec(
            "Tretyakov_3_2",
            "Третьяков, Неорганическая химия, том 3, книга 2",
            Path("/Users/doroga/Downloads/archive/Nkh_Neorganicheskaya_khimia_T_3_kn_2_Pod_red_Yu_D_Tretyakova.pdf"),
            OCR_ROOT / "Tretyakov_3_2",
            OCR_ROOT / "Tretyakov_3_2.index.json",
            OCR_ROOT / "Tretyakov_3_2.full.mmd",
            None,
            rebuild=True,
        ),
        DocumentSpec(
            "Tretyakov_elements_1_legacy",
            "Третьяков, старый распарсенный двухтомник «Химия элементов», том 1",
            Path("/Users/doroga/Documents/projects/picrete/LLM+RAG/donttouch/Третьяков_1.pdf"),
            OCR_ROOT / "Tretyakov_1",
            OCR_ROOT / "Tretyakov_1.index.json",
            None,
            None,
            rebuild=True,
            legacy=True,
        ),
        DocumentSpec(
            "Tretyakov_elements_2_legacy",
            "Третьяков, старый распарсенный двухтомник «Химия элементов», том 2",
            Path("/Users/doroga/Documents/projects/picrete/LLM+RAG/donttouch/Третьяков_2.pdf"),
            OCR_ROOT / "Tretyakov_2",
            OCR_ROOT / "Tretyakov_2.index.json",
            None,
            None,
            rebuild=True,
            legacy=True,
        ),
        DocumentSpec(
            "Sviridov_tasks",
            "Свиридов, задачник по общей и неорганической химии",
            Path("/Users/doroga/Downloads/Sviridov_tasks.pdf"),
            CHEMRAG_ROOT / "ocr_output/Sviridov_tasks",
            CHEMRAG_ROOT / "data/Sviridov_tasks.index.enriched.json",
            None,
            None,
            rebuild=True,
            legacy=True,
        ),
        DocumentSpec(
            "Eryomin_physical_chemistry_tasks",
            "Ерёмин, задачник по физической химии",
            Path("/Users/doroga/Downloads/Задачник_Еремин_В_В_,_Каргов_С_И_,_Успенская_И_А_,_Кузьменко_Н_Е.pdf"),
            OCR_ROOT / "Eryomin_physical_chemistry_tasks",
            OCR_ROOT / "Eryomin_physical_chemistry_tasks.index.json",
            OCR_ROOT / "Eryomin_physical_chemistry_tasks.full.mmd",
            OCR_ROOT / "Eryomin_physical_chemistry_tasks.quality.json",
        ),
        DocumentSpec(
            "Lab_practice",
            "Свиридов и др., Введение в лабораторный практикум по неорганической химии",
            Path("/Users/doroga/Downloads/Lab_practice.pdf"),
            CHEMRAG_ROOT / "ocr_output/Lab_practice",
            CHEMRAG_ROOT / "data/Lab_practice.index.enriched.json",
            None,
            None,
            rebuild=True,
            legacy=True,
        ),
        DocumentSpec(
            "Ahmetov_general_inorganic_chemistry",
            "Ахметов, Общая и неорганическая химия",
            None,
            CHEMRAG_ROOT / "ocr_output/ahmetov",
            CHEMRAG_ROOT / "data/ahmetov.index.enriched.json",
            None,
            None,
            rebuild=True,
            legacy=True,
            source_note="Исходный PDF не найден в доступных каталогах; сохранен полный OCR-набор и исходные индексы ChemRAG.",
        ),
    ]

    if any(spec.source_pdf is not None and not spec.source_pdf.is_file() for spec in specs):
        missing = [str(spec.source_pdf) for spec in specs if spec.source_pdf is not None and not spec.source_pdf.is_file()]
        raise RuntimeError("Missing source PDFs:\n" + "\n".join(missing))
    for spec in specs:
        if not spec.raw_dir.is_dir():
            raise RuntimeError(f"Missing OCR directory: {spec.raw_dir}")
        if spec.index_json is not None and not spec.index_json.is_file():
            raise RuntimeError(f"Missing OCR index: {spec.index_json}")

    ensure_new(LIBRARY_ROOT)
    (LIBRARY_ROOT / "ocr_output").mkdir(parents=True)
    (LIBRARY_ROOT / "sources").mkdir()
    (LIBRARY_ROOT / "extras").mkdir()

    manifest_documents: list[dict[str, Any]] = []
    for spec in specs:
        canonical_raw_dir = LIBRARY_ROOT / "ocr_output" / spec.doc_id
        if spec.doc_id in {
            "Lab_practice",
            "Ahmetov_general_inorganic_chemistry",
            "Tretyakov_elements_2_legacy",
        }:
            # This old export lacks page 1 in its raw tree. Build a small
            # canonical page tree so the repaired page 1 can be added without
            # modifying the original ChemRAG directory.
            canonical_raw_dir.mkdir(parents=True)
        else:
            link_or_copy(spec.raw_dir, canonical_raw_dir)
        if spec.source_pdf is not None:
            source_dir = LIBRARY_ROOT / "sources" / spec.doc_id
            source_dir.mkdir()
            link_or_copy(spec.source_pdf, source_dir / "source.pdf")
        else:
            source_dir = LIBRARY_ROOT / "sources" / spec.doc_id
            source_dir.mkdir()
            (source_dir / "SOURCE_MISSING.md").write_text(
                spec.source_note + "\n", encoding="utf-8"
            )

        if spec.rebuild:
            assert spec.index_json is not None
            records = json.loads(spec.index_json.read_text(encoding="utf-8"))
            if spec.doc_id in {
                "Lab_practice",
                "Ahmetov_general_inorganic_chemistry",
                "Tretyakov_elements_2_legacy",
            }:
                # The enriched Lab index contains page 1, while the old raw OCR
                # directory starts at page 2. Materialize repaired page text and
                # preserve existing box/image assets through symlinks.
                generated = materialize_legacy_pages(
                    spec.raw_dir,
                    canonical_raw_dir,
                    records,
                    spec.source_pdf,
                )
            else:
                generated = []
            derived = write_derived_files(LIBRARY_ROOT / "ocr_output", spec, records, spec.index_json)
            if spec.doc_id in {
                "Lab_practice",
                "Ahmetov_general_inorganic_chemistry",
                "Tretyakov_elements_2_legacy",
            }:
                derived["generated_asset_pages"] = generated
        else:
            assert spec.full_mmd is not None and spec.index_json is not None
            link_or_copy(spec.full_mmd, LIBRARY_ROOT / "ocr_output" / f"{spec.doc_id}.full.mmd")
            link_or_copy(spec.index_json, LIBRARY_ROOT / "ocr_output" / f"{spec.doc_id}.index.json")
            if spec.quality_json is not None:
                link_or_copy(spec.quality_json, LIBRARY_ROOT / "ocr_output" / f"{spec.doc_id}.quality.json")
            derived = {
                "pages": len(json.loads(spec.index_json.read_text(encoding="utf-8"))),
                "images": None,
                "external_image_references": 0,
            }

        if spec.index_json is not None and spec.legacy:
            metadata_dir = LIBRARY_ROOT / "extras" / "source_indexes" / spec.doc_id
            metadata_dir.mkdir(parents=True)
            for source_index in sorted(spec.index_json.parent.glob(f"{spec.index_json.stem.split('.index')[0]}.index.*")):
                link_or_copy(source_index, metadata_dir / source_index.name)

        manifest_documents.append(
            {
                "doc_id": spec.doc_id,
                "title": spec.title,
                "source_pdf": str(spec.source_pdf) if spec.source_pdf else None,
                "source_sha256": sha256(spec.source_pdf) if spec.source_pdf else None,
                "source_status": "present" if spec.source_pdf else "missing",
                "raw_ocr_directory": str(spec.raw_dir),
                "canonical_ocr_directory": f"ocr_output/{spec.doc_id}",
                "canonical_full_mmd": f"ocr_output/{spec.doc_id}.full.mmd",
                "canonical_index_json": f"ocr_output/{spec.doc_id}.index.json",
                "canonical_quality_json": f"ocr_output/{spec.doc_id}.quality.json",
                "legacy_source_format": spec.legacy,
                **derived,
            }
        )

    link_or_copy(
        Path("/Users/doroga/Documents/projects/rust-picrete/Picrete/tasks/Sviridov_tasks/Sviridov_tasks.json"),
        LIBRARY_ROOT / "extras/Sviridov_tasks.cleaned.json",
    )
    link_or_copy(
        Path("/Users/doroga/Documents/projects/rust-picrete/Picrete/tasks/Sviridov_tasks/ocr_output/Sviridov_tasks/addition.pdf"),
        LIBRARY_ROOT / "extras/Sviridov_addition.pdf",
    )

    duplicate_stats = write_duplicate_report(LIBRARY_ROOT / "duplicate_report.md")
    manifest = {
        "name": "Picrete unified chemistry OCR library",
        "format": "Picrete OCR: per-page result.mmd, result_with_boxes.jpg, images/, full.mmd, index.json, quality.json",
        "non_destructive": True,
        "documents": manifest_documents,
        "duplicate_scan": duplicate_stats,
        "notes": [
            "Canonical entries use symlinks for existing large OCR trees to avoid creating unnecessary copies; originals remain untouched.",
            "The separate Tretyakov volumes 1–2 archive is created alongside this library.",
            "Eryomin is present in the unified library but excluded from the Tretyakov 1–2 archive.",
            "The old Tretyakov elements volumes are intentionally labeled legacy and are not the official three-volume Tretyakov series.",
        ],
    }
    (LIBRARY_ROOT / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    readme = """# Единое собрание химических учебников Picrete

Каталог собран без удаления и перемещения исходных файлов. Большие уже готовые OCR-каталоги подключены символическими ссылками, поэтому в одном месте видны все материалы без создания лишних гигабайт копий.

## Где что лежит

- `ocr_output/` — единая структура Picrete: каталоги страниц, `result.mmd`, картинки, `result_with_boxes.jpg`, `*.full.mmd`, `*.index.json`, `*.quality.json`;
- `sources/` — исходные PDF, по одному на найденный набор;
- `extras/` — очищенный единый JSON Свиридова, приложение и исходные индексы ChemRAG;
- `manifest.json` — сопоставление исходника и OCR;
- `duplicate_report.md` — найденные точные дубликаты и варианты с одинаковым числом страниц.

Ахметов включен как OCR-набор, но его исходный PDF на компьютере не найден. Похожие PDF намеренно не подставлялись.

Третьяков 1–2 официальной трехтомной серии отдельно упакованы в `../archives/Tretyakov_official_volumes_1_2.tar.gz`.
"""
    (LIBRARY_ROOT / "README.md").write_text(readme, encoding="utf-8")

    archive_path = build_archive(specs)
    print(json.dumps({"library": str(LIBRARY_ROOT), "archive": str(archive_path)}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
