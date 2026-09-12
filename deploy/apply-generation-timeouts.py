#!/usr/bin/env python3
"""Narrow, Git-sourced nginx generation timeout update; dry-run by default.

Run from the approved Picrete checkout on picrete:
  sudo python3 deploy/apply-generation-timeouts.py
  sudo python3 deploy/apply-generation-timeouts.py --apply --expect-digest DIGEST

Only the nested generation locations are inserted. All other bytes, ownership,
permissions and existing symlinks are preserved. Source blocks come from the
tracked .com/.ru templates, never a replacement production config. All files
are backed up privately before replacement. nginx -t runs before reload; on
failure the original files are restored. A second identical invocation is a
no-op. --self-test tests transforms without touching nginx or the filesystem.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


API_PARENT = 'location ^~ /api/v1/ {'
API_CHILD = 'location ~ ^/api/v1/courses/[^/]+/trainer/sets/generate/?$ {'
STUDIO_PARENT = 'location /api/ {'
STUDIO_CHILD = 'location = /api/internal/trainer/generate {'
TARGETS = (
    ('/etc/nginx/sites-enabled/picrete', API_PARENT, API_CHILD, 'nginx-picrete.com.conf'),
    ('/etc/nginx/sites-enabled/picrete.ru', API_PARENT, API_CHILD, 'nginx-picrete.ru.conf'),
    ('/etc/nginx/sites-enabled/picrete.ru', STUDIO_PARENT, STUDIO_CHILD, 'nginx-picrete.ru.conf'),
    ('/etc/nginx/sites-enabled/dev.picrete.com', STUDIO_PARENT, STUDIO_CHILD, 'nginx-picrete.ru.conf'),
)


def block(text, header):
    matches = list(re.finditer(r'(?m)^[ \t]*' + re.escape(header) + r'[ \t]*$', text))
    if len(matches) != 1:
        raise ValueError('Expected one unambiguous location: ' + header)
    start = matches[0].start()
    opening = text.index('{', matches[0].start(), matches[0].end())
    # Headers/bodies inspected here contain no quoted braces. Reject unexpected
    # syntax instead of treating this as a general nginx parser.
    depth = 1
    for end in range(opening + 1, len(text)):
        if text[end] == '{':
            depth += 1
        elif text[end] == '}':
            depth -= 1
            if depth == 0:
                return start, opening, end + 1
    raise ValueError('Unclosed nginx location')


def normalized(value):
    return '\n'.join(line.strip() for line in value.strip().splitlines())


def insert(text, parent, child, source):
    source_start, _, source_end = block(source, child)
    addition = source[source_start:source_end]
    if 'proxy_read_timeout 1260s;' not in addition or 'proxy_next_upstream off;' not in addition:
        raise ValueError('Approved source lacks generation timeout/retry guard')
    start, opening, end = block(text, parent)
    body = text[opening + 1:end - 1]
    if child in text:
        child_start, _, child_end = block(text, child)
        if not (opening < child_start < child_end < end):
            raise ValueError('Existing generation location is outside expected parent')
        if normalized(text[child_start:child_end]) != normalized(addition):
            raise ValueError('Existing generation location differs; review before changing it')
        return text
    # Existing nested locations may shadow a generation route; fail closed.
    if re.search(r'(?m)^\s*location\s', body):
        raise ValueError('Unexpected nested location; manual routing review required')
    indent = re.match(r'[ \t]*', text[start:]).group() + '    '
    lines = addition.strip().splitlines()
    snippet = '\n'.join(indent + line.strip() if n in (0, len(lines)-1)
                        else indent + '    ' + line.strip() for n, line in enumerate(lines))
    return text[:opening + 1] + '\n' + snippet + text[opening + 1:]


def replace(path, data, metadata):
    fd, name = tempfile.mkstemp(prefix='.picrete-timeout-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
            os.fchown(output.fileno(), metadata.st_uid, metadata.st_gid)
            os.fchmod(output.fileno(), metadata.st_mode & 0o7777)
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def nginx_test():
    return subprocess.run(['nginx', '-t'], stdout=subprocess.DEVNULL,
                          stderr=subprocess.DEVNULL).returncode == 0


def apply(originals, candidates, metadata):
    if not nginx_test():
        raise ValueError('Existing nginx configuration fails nginx -t; nothing changed')
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    backup = Path(tempfile.mkdtemp(prefix=f'nginx-generation-{stamp}-', dir='/srv/picrete/backups'))
    changed = [p for p in candidates if candidates[p] != originals[p]]
    for index, path in enumerate(changed):
        if path.read_bytes() != originals[path]:
            raise ValueError('Configuration changed during preparation; rerun dry-run')
        shutil.copyfile(path, backup / f'{index}-{path.name}')
        (backup / f'{index}-{path.name}').chmod(0o600)
    (backup / 'paths.json').write_text(json.dumps([str(p) for p in changed], indent=2) + '\n')
    (backup / 'paths.json').chmod(0o600)
    installed = []
    try:
        for path in changed:
            replace(path, candidates[path], metadata[path])
            installed.append(path)
        if not nginx_test():
            raise ValueError('Candidate nginx -t failed')
        subprocess.run(['systemctl', 'reload', 'nginx'], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except Exception:
        for path in installed:
            replace(path, originals[path], metadata[path])
        restored = nginx_test()
        if restored:
            restored = subprocess.run(['systemctl', 'reload', 'nginx'], stdout=subprocess.DEVNULL,
                                      stderr=subprocess.DEVNULL).returncode == 0
        raise ValueError(f'Activation failed; original files restored; reload_ok={restored}; backup={backup}') from None
    print(json.dumps({'status': 'applied', 'backup': str(backup), 'nginx_test': 'passed'}))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--expect-digest')
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    if args.self_test:
        if args.apply:
            raise ValueError('self-test cannot apply')
        for _, parent, child, source in TARGETS:
            template = (root / source).read_text()
            a, _, b = block(template, child)
            before = template[:a] + template[b:]
            after = insert(before, parent, child, template)
            assert insert(after, parent, child, template) == after
            x, _, y = block(after, child)
            # Apart from the inserted location, all bytes are preserved.
            assert after[:x - 1] + after[y:] == before
        print('PASS: narrow insertion, unrelated bytes preserved, second run no-op; no live changes')
        return
    originals, candidates, metadata = {}, {}, {}
    for name, parent, child, source in TARGETS:
        path = Path(name).resolve(strict=True)
        if path not in originals:
            originals[path] = path.read_bytes()
            candidates[path] = originals[path]
            metadata[path] = path.stat()
        candidates[path] = insert(candidates[path].decode(), parent, child, (root / source).read_text()).encode()
    signature = hashlib.sha256(json.dumps(
        {str(p): [hashlib.sha256(originals[p]).hexdigest(), hashlib.sha256(candidates[p]).hexdigest()]
         for p in originals}, sort_keys=True).encode()).hexdigest()
    changed = [str(p) for p in originals if originals[p] != candidates[p]]
    print(json.dumps({'mode': 'apply' if args.apply else 'dry-run', 'digest': signature,
                      'changed_files': changed, 'locations': 4, 'timeout_seconds': 1260}, indent=2), flush=True)
    if args.apply and changed:
        if os.geteuid() != 0 or args.expect_digest != signature:
            raise ValueError('Apply requires root and exact --expect-digest from reviewed dry-run')
        apply(originals, candidates, metadata)


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # No nginx config contents or arbitrary subprocess output in errors.
        print(f'Stopped: {error if isinstance(error, ValueError) else type(error).__name__}')
        raise SystemExit(1)
