#!/usr/bin/env python3
"""Merge reviewed CSV exports into the existing Sviridov JSON, preserving other tasks.

python3 scripts/enrich_sviridov.py --csv-root '/path/to/Задания Свиридова' \
  --bank-root tasks/Sviridov_tasks --output /path/to/enriched/Sviridov_tasks.json
Media is copied under bank-root/ocr_output/Sviridov_tasks/csv_images only with --apply.
The default is a dry run. The original JSON is never overwritten without --apply;
an existing output gets a timestamped backup. The report contains no solution text.
"""
import argparse
import collections
import csv
import hashlib
import json
from pathlib import Path
import re
import shutil
import time

COLUMNS = {'номер', 'задание', 'решение', 'тип', 'сложность', 'объем'}
ALLOWED = {
    'тип': {'качественное', 'расчетное', 'теория', 'уравнения_реакций', 'вывод_формулы', 'комбинированное', 'анализ_рисунка'},
    'сложность': {'легкая', 'средняя', 'сложная'},
    'объем': {'короткое', 'среднее', 'длинное'},
}
IMAGE = re.compile(r'!\[([^\]]*)\]\(([^)]+)\)')


def readable_math(value):
    """Move top-level prose out of math boxes so long Russian sentences can wrap.

    Only plain, top-level text nodes are extracted. Formula groups and their
    sub/superscript bases are kept intact; no chemistry content is rewritten.
    """
    def render(match):
        math = match.group(1)
        math = re.sub(r'\\text\{([^{}]*∫[^{}]*)\}',
                      lambda m: r'\text{' + m[1].replace('∫', r'}\int\text{') + '}', math)
        pieces, pending, offset = [], '', 0
        def flush():
            nonlocal pending
            if pending.strip():
                pieces.append('$' + pending.strip() + '$')
            pending = ''
        for text in re.finditer(r'\\text\{([^{}\\]*)\}', math):
            before = math[:text.start()]
            prose = text[1]
            # Do not extract text from a fraction, exponent or other nested group.
            if before.count('{') != before.count('}') or len(re.findall(r'[А-Яа-яЁё]{2,}', prose)) < 3:
                continue
            pending += math[offset:text.start()]
            tail = ''
            if re.match(r'\s*[_^]', math[text.end():]):
                trailing = re.search(r'([^\s]+\s*)$', prose)
                if not trailing:
                    continue
                tail, prose = trailing[0], prose[:trailing.start()]
            else:
                trailing = re.search(r'([A-Za-zΑ-ω]+\s*)$', prose)
                if trailing:
                    tail, prose = trailing[0], prose[:trailing.start()]
            flush()
            pieces.append(prose.replace('*', r'\*').replace('_', r'\_'))
            if tail:
                pending = r'\text{' + tail + '}'
            offset = text.end()
        pending += math[offset:]
        flush()
        return ''.join(pieces)
    return re.sub(r'\$([^$]+)\$', render, value)


def merge(csv_root, bank_root):
    paragraphs = json.loads((bank_root / 'Sviridov_tasks.json').read_text())
    index = {t['number']: t for p in paragraphs for t in p['tasks']}
    by_paragraph = {p['paragraph']: p for p in paragraphs}
    assert len(index) == sum(len(p['tasks']) for p in paragraphs), 'Duplicate numbers in base bank'
    report = {'csv_rows': 0, 'updated': [], 'added': [], 'duplicates': [], 'images': [], 'replaced_cross_references': [], 'classifications': {}}
    seen, copies = {}, {}
    for path in sorted(csv_root.rglob('*.csv')):
        with path.open(encoding='utf-8-sig', newline='') as stream:
            reader = csv.DictReader(stream)
            if not COLUMNS.issubset(reader.fieldnames or []):
                raise ValueError(f'{path}: missing columns {COLUMNS - set(reader.fieldnames or [])}')
            for row in reader:
                report['csv_rows'] += 1
                row = {k: v.strip() for k, v in row.items() if k in COLUMNS}
                number = row['номер']
                if not re.fullmatch(r'\d+\.\d+', number) or not row['задание']:
                    raise ValueError(f'{path}: invalid task {number}')
                for key, allowed in ALLOWED.items():
                    if row[key] not in allowed:
                        raise ValueError(f'{number}: unknown {key}: {row[key]}')
                if number in seen:
                    previous = seen[number]
                    # The supplied export repeats 8.9 with the optional word "равна".
                    # Accept only this benign wording difference with identical solution/labels.
                    canon = lambda r: {k: v.replace(' равна ', ' ') if k == 'задание' else v for k, v in r.items()}
                    if canon(row) != canon(previous):
                        raise ValueError(f'Conflicting duplicate {number}; resolve explicitly before importing')
                    report['duplicates'].append(number)
                    continue
                seen[number] = row
                text = row['задание']
                solution = row['решение'] or None
                task = index.get(number)
                if task is None:
                    paragraph = by_paragraph.get(number.split('.')[0])
                    if paragraph is None:
                        raise ValueError(f'{number}: no matching paragraph')
                    task = {'number': number, 'answer': '', 'images': []}
                    paragraph['tasks'].append(task)
                    index[number] = task
                    report['added'].append(number)
                else:
                    report['updated'].append(number)
                images = IMAGE.findall(text)
                if solution and IMAGE.search(solution):
                    raise ValueError(f'{number}: solution image needs explicit placement support')
                if images:
                    new_images = []
                    for _, raw in images:
                        image = (path.parent / raw).resolve()
                        if not image.is_relative_to(path.parent.resolve()) or not image.is_file():
                            raise ValueError(f'{number}: missing or unsafe image {raw}')
                        data = image.read_bytes()
                        if not (data.startswith(b'\x89PNG\r\n\x1a\n') or data.startswith(b'\xff\xd8\xff')):
                            raise ValueError(f'{number}: invalid image {raw}')
                        digest = hashlib.sha256(data).hexdigest()
                        relative = f'csv_images/{digest}{image.suffix.lower()}'
                        copies[relative] = image
                        new_images.append(relative)
                        report['images'].append({'number': number, 'file': raw, 'sha256': digest, 'previous': task.get('images', [])})
                    task['images'] = new_images
                    text = IMAGE.sub('', text).strip()
                old_answer = task.get('answer', '')
                if solution and any(marker in old_answer.lower() for marker in ['см. решение', 'см. ответ', 'см. пояснение']):
                    task['legacy_answer'] = old_answer
                    task['answer'] = ''
                    report['replaced_cross_references'].append(number)
                task.update(text=readable_math(text), solution=readable_math(solution) if solution else None, task_type=row['тип'], difficulty=row['сложность'], volume=row['объем'], topic=path.parent.name)
    if not seen:
        raise ValueError('No CSV tasks found')
    media_root = bank_root / 'ocr_output/Sviridov_tasks'
    for paragraph in paragraphs:
        paragraph['tasks'].sort(key=lambda t: tuple(map(int, t['number'].split('.'))))
        for task in paragraph['tasks']:
            for raw in task.get('images', []):
                relative = raw.removeprefix('ocr_output/Sviridov_tasks/')
                image = (media_root / relative).resolve()
                if not image.is_relative_to(media_root.resolve()):
                    raise ValueError(f'Unsafe bank image: {raw}')
                if relative not in copies and not image.is_file():
                    raise ValueError(f'Missing existing bank image: {raw}')
    report['unique_csv_tasks'] = len(seen)
    report['total_bank_tasks'] = len(index)
    report['with_solution'] = sum(bool(t.get('solution')) for t in index.values())
    report['classifications'] = {k: dict(collections.Counter(r[k] for r in seen.values())) for k in ALLOWED}
    return paragraphs, copies, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--csv-root', type=Path, required=True)
    parser.add_argument('--bank-root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    data, copies, report = merge(args.csv_root, args.bank_root)
    if args.apply:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        if args.output.exists():
            shutil.copy2(args.output, args.output.with_name(args.output.name + f'.{time.time_ns()}.bak'))
        for relative, source in copies.items():
            target = args.bank_root / 'ocr_output/Sviridov_tasks' / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
        temporary = args.output.with_suffix('.tmp')
        temporary.write_text(json.dumps(data, ensure_ascii=False, indent=4) + '\n')
        temporary.replace(args.output)
        args.output.with_suffix('.report.json').write_text(json.dumps(report, ensure_ascii=False, indent=4) + '\n')
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
