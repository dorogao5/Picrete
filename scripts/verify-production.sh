#!/usr/bin/env bash
set -euo pipefail

PICRETE_ORIGIN="${PICRETE_ORIGIN:-https://picrete.com}"
STUDIO_ORIGIN="${STUDIO_ORIGIN:-https://dev.picrete.com}"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_header() {
  local headers="$1"
  local name="$2"
  local pattern="$3"
  grep -Eiq "^${name}:[[:space:]]*${pattern}" "$headers" \
    || fail "${name} header at ${4} does not match ${pattern}"
}

check_json_value() {
  local url="$1"
  local key="$2"
  local expected="$3"
  curl --fail --silent --show-error --max-time 15 "$url" \
    | python3 -c 'import json,sys; data=json.load(sys.stdin); key,expected=sys.argv[1:]; value=data.get(key); raise SystemExit(0 if value == expected else f"{key}={value!r}, expected {expected!r}")' "$key" "$expected"
}

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

check_json_value "${PICRETE_ORIGIN}/healthz" status healthy
check_json_value "${PICRETE_ORIGIN}/readyz" status ready
check_json_value "${STUDIO_ORIGIN}/healthz" status ok

for origin in "$PICRETE_ORIGIN" "$STUDIO_ORIGIN"; do
  headers="$tmp_dir/$(printf '%s' "$origin" | tr -cd '[:alnum:]').headers"
  curl --fail --silent --show-error --location --max-time 15 --head "$origin/" > "$headers"
  assert_header "$headers" strict-transport-security 'max-age=31536000' "$origin"
  assert_header "$headers" x-content-type-options 'nosniff' "$origin"
  assert_header "$headers" x-frame-options 'DENY' "$origin"
  assert_header "$headers" referrer-policy 'strict-origin-when-cross-origin' "$origin"
  assert_header "$headers" permissions-policy 'camera=\(\)' "$origin"
  assert_header "$headers" content-security-policy "default-src 'self'" "$origin"
  assert_header "$headers" cache-control 'no-cache, no-store, must-revalidate' "$origin"
done

echo "Production smoke checks passed for $PICRETE_ORIGIN and $STUDIO_ORIGIN"
