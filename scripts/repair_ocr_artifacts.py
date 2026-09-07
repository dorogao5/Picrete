#!/usr/bin/env python3
"""Apply reviewed OCR repairs and rebuild the unified derived files."""

from __future__ import annotations

import json
import re
import shutil
import sys
from pathlib import Path

ROOT = Path("/Users/doroga/Documents/projects/picrete/LLM+RAG/data")
OCR_ROOT = ROOT / "ocr_output"
CHEMRAG = Path("/Users/doroga/Downloads/ChemRAG")
LIBRARY = ROOT / "unified_ocr_library"

sys.path.insert(0, str(Path(__file__).resolve().parent))
from build_unified_ocr_library import DocumentSpec, write_derived_files  # noqa: E402


CHANGES = {
    "Tretyakov_3_1": (OCR_ROOT / "Tretyakov_3_1", OCR_ROOT / "Tretyakov_3_1.index.json", {6, 15, 102, 144, 160, 244, 276, 285}),
    "Tretyakov_3_2": (OCR_ROOT / "Tretyakov_3_2", OCR_ROOT / "Tretyakov_3_2.index.json", {35, 127, 255, 305}),
    "Tretyakov_elements_1_legacy": (OCR_ROOT / "Tretyakov_1", OCR_ROOT / "Tretyakov_1.index.json", {119, 147, 225, 256, 294, 367, 416}),
    "Tretyakov_elements_2_legacy": (OCR_ROOT / "Tretyakov_2", OCR_ROOT / "Tretyakov_2.index.json", {11, 81, 134, 205, 210, 334, 459, 563, 615}),
    "Lab_practice": (CHEMRAG / "ocr_output/Lab_practice", CHEMRAG / "data/Lab_practice.index.enriched.json", {3, 42, 47}),
    "Sviridov_tasks": (CHEMRAG / "ocr_output/Sviridov_tasks", CHEMRAG / "data/Sviridov_tasks.index.enriched.json", {1, 2, 3, 69, 77, 128, 306}),
    "Ahmetov_general_inorganic_chemistry": (CHEMRAG / "ocr_output/ahmetov", CHEMRAG / "data/ahmetov.index.enriched.json", {15, 16, 19, 28, 31, 52, 74, 88, 91, 99, 105, 117, 131, 140, 164, 167, 182, 197, 205, 215, 221, 222, 233, 269, 271, 275, 276, 292, 305, 326, 340, 341}),
}


def replace_text(path: Path, replacements: list[tuple[str, str]]) -> None:
    text = path.read_text(encoding="utf-8")
    for old, new in replacements:
        if old not in text:
            if new in text:
                continue
            raise RuntimeError(f"expected OCR fragment not found in {path}: {old[:80]!r}")
        text = text.replace(old, new)
    path.write_text(text, encoding="utf-8")


def replace_regex(path: Path, pattern: str, replacement: str, *, count: int = 1) -> None:
    text = path.read_text(encoding="utf-8")
    updated, replacements = re.subn(pattern, replacement, text, count=count, flags=re.S)
    if replacements == 0:
        if replacement in text:
            return
        raise RuntimeError(f"expected OCR pattern not found in {path}: {pattern[:120]!r}")
    path.write_text(updated, encoding="utf-8")


def apply_manual_repairs() -> None:
    p6 = OCR_ROOT / "Tretyakov_3_1/page_0006/result.mmd"
    replace_text(
        p6,
        [
            ("(4f¹⁵d¹⁶s²)", r"\(4f^{1}5d^{1}6s^{2}\)"),
            ("(4f³⁶s²)", r"\(4f^{3}6s^{2}\)"),
            ("(4f¹⁴⁵d¹⁶s²)", r"\(4f^{14}5d^{1}6s^{2}\)"),
        ],
    )
    replace_text(
        OCR_ROOT / "Tretyakov_3_1/page_0015/result.mmd",
        [("E*, B (pH 0)", "E°, В (pH 0)"), ("E*, B (pH 14)", "E°, В (pH 14)")],
    )

    p276 = OCR_ROOT / "Tretyakov_3_1/page_0276/result.mmd"
    p276.write_text(
        r"""![Рис. 5.14а. Область устойчивости гаусманнита](images/0.jpg)

![Рис. 5.14б. Кристаллическая структура гаусманнита](images/1.jpg)

![Рис. 5.14в. Форма кристаллов гаусманнита](images/2.jpg)

![Форма кристаллов гаусманнита](images/3.jpg)

![Форма кристаллов гаусманнита](images/4.jpg)

<center>Рис. 5.14. Гаусманнит: \(a\) — область устойчивости: парциальное давление кислорода \((p_{\mathrm{O_2}},\ \text{атм})\) — температура; \(б\) — кристаллическая структура; \(в\) — форма кристаллов</center>

ми. Это значение энергии стабилизации кристаллическим полем уступает только иону \(\mathrm{Cr^{3+}}\) (158 кДж/моль). Поэтому все шпинели трехвалентного марганца относят к нормальным.

Оксид \(\mathrm{Mn_3O_4}\) образуется при термическом распаде диоксида, который при температуре \(550^{\circ}\mathrm{C}\) переходит в \(\mathrm{Mn_2O_3}\), а при \(950^{\circ}\mathrm{C}\) — в \(\mathrm{Mn_3O_4}\). Термический распад сульфата \(\mathrm{MnSO_4}\) выше \(1000^{\circ}\mathrm{C}\) или его сплавление с поташом на воздухе дают гаусманнит в виде красных кристаллов. Это вещество также является продуктом окисления гидроксида марганца(II) на воздухе.

Многие восстановители, например водород и углерод, способны переводить \(\mathrm{Mn_3O_4}\) лишь в низший оксид \(\mathrm{MnO}\). Полное восстановление оксида \(\mathrm{Mn_3O_4}\) происходит только в присутствии железа, которое растворяет марганец, образуя твердый раствор.
""",
        encoding="utf-8",
    )

    replace_text(
        OCR_ROOT / "Tretyakov_3_2/page_0255/result.mmd",
        [
            ("[Zn(CH<sub>3</sub>)(N(C<sub>6</sub>H<sub>5</sub>))]<sub>2</sub>", "[Zn(CH<sub>3</sub>)(N(C<sub>6</sub>H<sub>5</sub>)<sub>3</sub>)]<sub>2</sub>"),
            ("[Cd(S<sub>2</sub>CN(C<sub>2</sub>H<sub>5</sub>)<sub>2</sub>]<sub>2</sub>]<sub>2</sub>", "[Cd(S<sub>2</sub>CN(C<sub>2</sub>H<sub>5</sub>)<sub>2</sub>)<sub>2</sub>]<sub>2</sub>"),
            ("[Hg{N(C<sub>2</sub>H<sub>4</sub>N(CH<sub>3</sub>)<sub>2</sub>)<sub>3</sub>}]<sup>I</sup>]", "[Hg{N(C<sub>2</sub>H<sub>4</sub>N(CH<sub>3</sub>)<sub>2</sub>)<sub>3</sub>}I]"),
        ],
    )
    replace_text(
        OCR_ROOT / "Tretyakov_2/page_0205/result.mmd",
        [("стальных зубов", "металлических зубов"), ("Метабозат натрия", "Метаборат натрия")],
    )
    replace_text(
        OCR_ROOT / "Tretyakov_3_1/page_0144/result.mmd",
        [
            ("<sup>111</sup>", "<sup>III</sup>"),
            ("<sup>11</sup>", "<sup>II</sup>"),
            ("<sup>1</sup>", "<sup>I</sup>"),
            ("mpem-C<sub>4</sub>H<sub>9</sub>", "трет-C<sub>4</sub>H<sub>9</sub>"),
        ],
    )
    replace_text(
        OCR_ROOT / "Tretyakov_3_2/page_0035/result.mmd",
        [("E*(M³⁺/M⁰)", "E°(M³⁺/M⁰)"), ("E'(M²⁺/M⁰)", "E°(M²⁺/M⁰)")],
    )
    replace_text(
        CHEMRAG / "ocr_output/ahmetov/page_0099/result.mmd",
        [
            ("<td>Li<br/>☐</td><td>Ba<br/>☐</td><td colspan=", "<td>Li<br/>☐</td><td>Be<br/>☐</td><td colspan="),
            ("<td>Po<br/>☐</td><td>Al<br/>☐</td><td>Rb<br/>O</td>", "<td>Po<br/>☐</td><td>At<br/>☐</td><td>Rn<br/>O</td>"),
        ],
    )
    replace_text(
        CHEMRAG / "ocr_output/ahmetov/page_0164/result.mmd",
        [
            ("3 \\mathrm{H} \\mathrm{NO}_2", "3 \\mathrm{HNO}_2"),
            ("2 \\mathrm{Na} \\mathrm{NO}_2", "2 \\mathrm{NaNO}_2"),
            ("5 \\mathrm{Na} \\mathrm{NO}_2", "5 \\mathrm{NaNO}_2"),
            ("5 \\mathrm{Na} \\mathrm{NO}_3", "5 \\mathrm{NaNO}_3"),
            ("\\sigma_2^2 \\sigma_2^2 \\pi_{x,y}^4 \\pi_{x,y}^{4.4}", "\\sigma_s^2 \\sigma_z^2 \\pi_{x,y}^4 \\pi_{x,y}^{*4}"),
            ("разлагается на NO₂⁻ и O₂", "разлагается на NO₂ и O₂"),
            ("Триоксонитрат (V)-ион NO₂⁻", "Триоксонитрат (V)-ион NO₃⁻"),
            ("HNO₃⁻", "HNO₃"),
        ],
    )
    replace_text(
        CHEMRAG / "ocr_output/ahmetov/page_0182/result.mmd",
        [
            ("четырем (sp²-гибридизация), трем (sp²-гибридизация)", "четырем (sp³-гибридизация), трем (sp²-гибридизация)"),
            ("Тетраодрическое", "Тетраэдрическое"),
        ],
    )

    # Mistral occasionally emits a diagram as a remote Codecogs/Imgur URL.
    # Keep the corresponding crop beside the page so the library is self-contained.
    replace_text(
        CHEMRAG / "ocr_output/ahmetov/page_0131/result.mmd",
        [
            (
                r'<img alt="Cl Cl" src="https://latex.codecogs.com/svg.latex?\chemfig{*6((-O)-(-Cl)-(-H)=(-Cl)-)}"/>',
                '<img alt="Структура молекул Cl2O и Cl3N" src="images/structure_chlorine_compounds.jpg"/>',
            ),
            (
                r'<img alt="Cl Cl" src="https://latex.codecogs.com/svg.latex?\chemfig{*6((-N)-(-Cl)-(-Cl)-)}"/>',
                '<img alt="Структура молекул Cl2O и Cl3N" src="images/structure_chlorine_compounds.jpg"/>',
            ),
        ],
    )
    replace_text(
        CHEMRAG / "ocr_output/ahmetov/page_0182/result.mmd",
        [
            (
                r'<img alt="Diagram of 2p with sp² and 2s and sp² and 2p" src="https://i.imgur.com/8XJzK9l.png"/>',
                '<img alt="Гибридизация орбиталей углерода" src="images/carbon_hybridization.jpg"/>',
            ),
        ],
    )
    replace_text(
        CHEMRAG / "ocr_output/ahmetov/page_0164/result.mmd",
        [
            (
                r"""\[
\left[ \begin{array}{c} \mathrm{O} \\ \mathrm{O} \end{array} \right]^{\mathrm{N}} \mathrm{O} \Big]^{\cdot}
\]""",
                "![Структура нитрит-иона](images/nitrite_structure.jpg)",
            ),
            (
                r"""\[
\left[ \mathrm{O} - \mathrm{N} \mathrm{O} \right]^{\cdot}
\]""",
                "![Структура нитрат-иона](images/nitrate_structure.jpg)",
            ),
        ],
    )
    replace_regex(
        CHEMRAG / "ocr_output/ahmetov/page_0164/result.mmd",
        r"<table><tr><th>Характер гибридизации.*?</table>",
        "![Гибридизация орбиталей азота и пространственная конфигурация соединений](images/hybridization_nitrogen.jpg)",
    )

    # These two reaction schemes are diagrams. The OCR text is not reliable enough
    # to reconstruct their geometry, so retain the exact source crop.
    replace_regex(
        OCR_ROOT / "Tretyakov_3_2/page_0127/result.mmd",
        r"\\\[\s*\\begin\{array\}\{c\}.*?\\\]",
        "![Схемы замещения в комплексах платины](images/synthesis_schemes.jpg)",
        count=2,
    )

    # Remove leaked Mistral protocol markers from the legacy Sviridov pages.
    for page in (69, 77, 128):
        replace_regex(
            CHEMRAG / "ocr_output/Sviridov_tasks" / f"page_{page:04d}/result.mmd",
            r"\s*<\|ref\|>.*\Z",
            "",
        )
    replace_regex(
        CHEMRAG / "ocr_output/Sviridov_tasks/page_0306/result.mmd",
        r"<\|[^>]+\|>|<｜[^>]+｜>",
        "",
        count=0,
    )


def update_source_index(index_path: Path, raw_dir: Path, pages: set[int]) -> list[dict]:
    records = json.loads(index_path.read_text(encoding="utf-8"))
    known_pages = {int(record["page"]) for record in records}
    for page in sorted(pages - known_pages):
        page_dir = raw_dir / f"page_{page:04d}"
        if not (page_dir / "result.mmd").is_file():
            raise RuntimeError(f"cannot add missing indexed page {page}: {page_dir}")
        records.append(
            {
                "doc_id": raw_dir.name,
                "page": page,
                "markdown": "",
                "images": [],
                "boxed_image": (
                    f"ocr_output/{raw_dir.name}/page_{page:04d}/result_with_boxes.jpg"
                    if (page_dir / "result_with_boxes.jpg").is_file()
                    else None
                ),
            }
        )
    for record in records:
        page = int(record["page"])
        if page not in pages:
            continue
        page_dir = raw_dir / f"page_{page:04d}"
        record["markdown"] = (page_dir / "result.mmd").read_text(encoding="utf-8")
        image_names = sorted(path.name for path in (page_dir / "images").glob("*") if path.is_file())
        if image_names:
            old_images = record.get("images") or []
            prefix = str(old_images[0]).rsplit("/page_", 1)[0] if old_images else f"ocr_output/{raw_dir.name}"
            record["images"] = [f"{prefix}/page_{page:04d}/images/{name}" for name in image_names]
        else:
            record["images"] = []
    records.sort(key=lambda item: int(item["page"]))
    index_path.write_text(json.dumps(records, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return records


def sync_canonical_pages(doc_id: str, raw_dir: Path, pages: set[int]) -> None:
    destination = LIBRARY / "ocr_output" / doc_id
    if destination.is_symlink():
        return
    for page in pages:
        source_page = raw_dir / f"page_{page:04d}"
        target_page = destination / f"page_{page:04d}"
        target_page.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_page / "result.mmd", target_page / "result.mmd")
        for source_image in sorted((source_page / "images").glob("*")):
            if not source_image.is_file():
                continue
            target_image = target_page / "images" / source_image.name
            target_image.parent.mkdir(parents=True, exist_ok=True)
            if target_image.exists() or target_image.is_symlink():
                target_image.unlink()
            shutil.copy2(source_image, target_image)


def rewrite_existing_raw_full(raw_dir: Path, records: list[dict]) -> None:
    full_path = raw_dir.parent / f"{raw_dir.name}.full.mmd"
    if not full_path.is_file():
        return
    parts = []
    for record in sorted(records, key=lambda item: int(item["page"])):
        parts.append(f"\n\n<!-- PAGE {int(record['page'])} -->\n\n{str(record.get('markdown') or '').strip()}\n")
    full_path.write_text("".join(parts), encoding="utf-8")


def main() -> int:
    apply_manual_repairs()
    for doc_id, (raw_dir, index_path, pages) in CHANGES.items():
        records = update_source_index(index_path, raw_dir, pages)
        sync_canonical_pages(doc_id, raw_dir, pages)
        rewrite_existing_raw_full(raw_dir, records)
        spec = DocumentSpec(doc_id, doc_id, None, raw_dir, index_path, rebuild=True, legacy=True)
        write_derived_files(LIBRARY / "ocr_output", spec, records, index_path)
        print(f"updated {doc_id}: {len(pages)} pages")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
