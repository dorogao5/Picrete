# Physical chemistry deployment audit and release runbook

Audit: 2026-09-12, approximately 10:27 server time. **No deployment performed.**

Deployment handoff: the user subsequently authorized Git-only deployment after
providing the exact three candidate SHAs and confirming green CI. Until that
gate is met, do not switch services or nginx. Build the new release completely
before entering maintenance; preserve queues and take database backups before
starting candidate services. The audit's no-paid-model restriction does not
prohibit the user's separately authorized post-deployment generation checks.
The content release script itself still makes zero model calls.
Initially only this document was written. The subsequently requested
`Studio-Picrete/ops/release_physical_chemistry.py` was also implemented locally.
Server inspection, Git remote queries, health
requests and SQL SELECTs were read-only; no builds, migrations, restarts, pushes,
content publication or paid LLM calls were executed. Commands below are future
operator instructions, not a record of actions already taken.

## Observed revisions and release gates

Local parent: `/Users/doroga/Documents/projects/rust-picrete`.
Server: SSH alias `picrete`, user `doroga`, host `picrete-r-and-d`.
Active checkout parent: `/srv/picrete/releases/20260912-physchem-git`.

| Repo | Local HEAD = server HEAD = GitHub main at audit | Git origin |
| --- | --- | --- |
| Picrete | `868c12a629c5f21a30152d664dc407f45bf21f84` | `https://github.com/dorogao5/Picrete.git` |
| Studio-Picrete | `01a8547ee21cbf02244d248e08281cd1389c7fb3` | `https://github.com/dorogao5/Studio-Picrete.git` |
| Front-Picrete | `f8cfce5333c1dd137185eafcf7f6ce2aacf05b77` | `https://github.com/dorogao5/Front-Picrete.git` |

All server checkouts were clean on `main`. The local checkouts were dirty:

- Picrete: `src/api/practice/catalog.rs`, `src/api/practice/tests.rs`,
  `src/api/trainer/handlers.rs`, `src/repositories/trainer_sets.rs`.
- Studio-Picrete: `backend/app/api/integration.py`,
  `backend/app/services/physical_chemistry.py`, `backend/app/services/taskgen.py`.
- Front-Picrete: `src/lib/practice.ts`, `src/pages/CourseTrainer.tsx`.

Local edits were changing during the audit; this is a snapshot, not an approved
release diff. None of these edits is deployed merely because HEAD matches.

| Live artifact | Identity / discrepancy |
| --- | --- |
| `app-api-1`, `app-worker-1` | `picrete-runtime:868c12a`; OCI revision `868c12a` |
| Runtime image ID | `sha256:ea85b22aa8d2e9d758ccd83a07b10cd65ce2dc611f54d1858ef3426f5b989ed5` |
| `studio-picrete-studio-api-1` | `picrete-studio:01a8547`; OCI revision `01a8547` |
| Studio image ID | `sha256:5ac6b3fb474863ffc2fbb6be6bf2c920fea109d308841d8c1009271952ddb995` |
| Student `dist/build-info.json` | `f8cfce5333c1dd137185eafcf7f6ce2aacf05b77`, matches checkout |
| Studio `frontend/dist/build-info.json` | **`083a8b5d9f40f529f3340acd384fdd0f0b1a8ea1`**, older than checkout/backend |
| API `/version` | **`development`**, despite labeled image |
| Studio `/version` | **`unknown`**, despite labeled image |

The API reads `option_env!("BUILD_REVISION")` at compile time in
`src/api/handlers.rs`. The deployed Dockerfile declares `ARG BUILD_REVISION`
only in the runtime stage, after compilation. The main agent has now fixed the
local Dockerfile with the builder-stage ARG and `cargo build --release --locked
--bins`; include this change in the approved Git revision. It is not deployed.
Retain runtime ARG/OCI labels. Runtime environment alone cannot repair the
compiled value. This audit did not edit the Dockerfile.

Studio's running environment contains `BUILD_REVISION=unknown`; its `.env`
overrides the image's baked-in revision. The preparation commands below remove
that override from the new release env. Both frontend build-info plugins run
`git rev-parse HEAD`; build both interfaces inside the exact Git checkouts.

## Build, Compose, environment and persistent data

Paths below are absolute server paths; `OLD` in commands abbreviates the active
checkout parent above.

| Component | Exact source / configuration |
| --- | --- |
| API + worker project | Compose project **`app`**; `$OLD/Picrete/docker-compose.prod.yml` |
| Runtime build | `$OLD/Picrete/Dockerfile`, context `$OLD/Picrete`; Rust `1.88.0-bookworm` builder, Ubuntu `24.04` runtime |
| API Compose interpolation env | **`/tmp/picrete-release.env`**, confirmed by container Compose label; no `.env` in current Picrete checkout |
| Studio project | Compose project **`studio-picrete`**; `$OLD/Studio-Picrete/docker-compose.yml` |
| Studio build at audit | `$OLD/Studio-Picrete/backend/Dockerfile`, context `backend`; uv `0.8.9`, Python `3.12-slim`, `uv sync --frozen --no-dev` |
| Studio env | `$OLD/Studio-Picrete/.env` → **`/tmp/studio-release.env`**; Compose service has `env_file: .env` |
| Student frontend | `$OLD/Front-Picrete/package-lock.json`, `vite.config.ts`; `npm ci && npm run build` → `dist` |
| Studio frontend | `$OLD/Studio-Picrete/frontend/package-lock.json`, `vite.config.ts`; `npm ci && npm run build` → `frontend/dist` |
| Runtime task bank | `$OLD/Picrete/tasks` → `/app/tasks:ro`; files are tracked in Git |
| Worker migrations | `$OLD/Picrete/migrations` → `/app/migrations:ro`; API uses migrations copied into image |
| Studio data | `$OLD/Studio-Picrete/data` → `/app/data`; preserve even though active DB is PostgreSQL |
| Databases | Host PostgreSQL: **`picrete_db`**, **`picrete_studio`**; Studio uses `postgresql+asyncpg` on `127.0.0.1` |
| Redis | Host service, loopback port `6379`; unrelated MolQuiz containers also exist and are out of scope |

The main agent has also changed local Studio Compose to repository-root build
context with `backend/Dockerfile`, and added the curated release script/content
to the image. Commit those changes before using the content-release command;
the old deployed image has no `ops` package. The release recipe below reads the
candidate Compose file and therefore uses its updated build context.

Root-owned env candidates exist at `/srv/picrete/shared/secrets/picrete.env` and
`/srv/picrete/shared/secrets/studio.env`. **Neither equals the corresponding live
temporary env file. Do not substitute them silently.** A text comparison of keys
and values reported these differing key names (values deliberately omitted):

- Picrete: `AI_MAX_TOKENS`, `ASSISTANT_AI_PROVIDER_ROUTES_JSON`,
  `ASSISTANT_AI_REQUEST_TIMEOUT`, `ASSISTANT_CHAT_MAX_CONCURRENT`,
  `FIRST_SUPERUSER_ISU`, `MAX_CONCURRENT_EXAMS`, `PATH`, `PICRETE_HOST`,
  `REDIS_DB`, `RELEASE_SHA`, `TASK_BANK_VOLUME`.
- Studio: `BUILD_REVISION`, `GPG_KEY`, `LANG`, `PATH`, `PYTHON_SHA256`,
  `PYTHON_VERSION`, `RELEASE_SHA`.

This comparison is textual, not a normalized dotenv comparison. Temporary env
permissions were `0600` for Picrete and **`0664` for Studio**. The future recipe
creates durable, private release env copies without printing values. Never use
`set -x`, dump `docker inspect`, print expanded `docker compose config`, `env`,
or copy secret files into Git. Use `docker compose config --quiet`.

The host currently has Node `20.19.5` / npm `10.8.2`; CI uses Node **24**.
For a release build use Node 24 explicitly, as below. Locks pin application
dependencies, but base image tags, apt packages and toolchain image tags are not
digest-pinned. The process reproduces source provenance, not guaranteed identical
image bytes; record final image IDs and build tool image IDs in the manifest.

## Live nginx and boot configuration

- Main config: `/etc/nginx/nginx.conf`, includes `/etc/nginx/sites-enabled/*`.
- `/etc/nginx/sites-enabled/picrete` → `/etc/nginx/sites-available/picrete`.
  `picrete.com` student HTML root: `/srv/picrete/Front-Picrete/dist`;
  `/assets/` root: `/srv/picrete/shared/frontend` (files under `assets/`).
  `/api/v1`, `/version`, `/healthz`, `/readyz` proxy to `127.0.0.1:8000`.
- `/etc/nginx/sites-enabled/dev.picrete.com` is a **regular file**, not the
  differently sized `/etc/nginx/sites-available/dev.picrete.com`.
  Studio HTML root: `/var/www/picrete-studio`; `/assets/` root:
  `/srv/picrete/shared/studio`. `/api/`, `/version`, `/healthz` proxy to
  `127.0.0.1:8100`.
- `/etc/nginx/sites-enabled/picrete.ru` is a regular file. Main page routes
  redirect to `.com`; API and assets still have explicit locations. The same
  file defines `dev.picrete.ru`, whose page routes redirect to `dev.picrete.com`.
- Shared headers: `/etc/nginx/snippets/picrete-security-headers.conf`;
  proxy settings: `/etc/nginx/proxy_params`; TLS config under `/etc/letsencrypt`.
- `/srv/picrete/Front-Picrete` points to `$OLD/Front-Picrete`.
  `/var/www/picrete-studio` points to `$OLD/Studio-Picrete/frontend/dist`.
- **Stale boot path:** `/srv/picrete/app` points to
  `/srv/picrete/releases/20260910-course-worker/Picrete`.
  `/etc/systemd/system/picrete-compose.service` is enabled and active, uses
  `WorkingDirectory=/srv/picrete/app`, and starts
  `docker compose -f docker-compose.prod.yml --env-file .env up -d`.
  That old working directory has **no `.env`**. Its stop command uses
  `docker compose -f docker-compose.prod.yml down`. Do not restart this unit
  during preparation; the future switch below repairs the symlink and gives
  the new `.env` an explicit Compose project name and revision.

Tracked templates are `Picrete/deploy/nginx-picrete.com.conf`,
`Picrete/deploy/nginx-picrete.ru.conf`,
`Picrete/deploy/nginx-security-headers.conf` and
`Studio-Picrete/deploy/nginx-dev.picrete.com.conf`. They are not proof of the
installed configuration and should not overwrite the live files in this release.
`nginx -t` passed, with protocol-option and proxy-header-hash warnings. No reload
is needed when only the static-root symlinks change.

For the upcoming generation-timeout change, use the Git-tracked
`deploy/apply-generation-timeouts.py` from the approved Picrete checkout.
It inserts only the generation locations from the tracked nginx templates into
the three active files (including the actual regular `dev.picrete.com` file).
Other configuration bytes and symlinks are preserved. It backs up originals,
runs `nginx -t`, reloads only after success, and restores originals on failure.
The second run is a no-op. Four location transforms were checked read-only
against the actual server configs; no live config was written.

```bash
# Run on picrete from the new Git checkout; default is read-only.
sudo python3 "$NEW/Picrete/deploy/apply-generation-timeouts.py"
# Only at the authorized switch, with the digest from that reviewed output:
sudo python3 "$NEW/Picrete/deploy/apply-generation-timeouts.py" \
  --apply --expect-digest "$NGINX_PLAN_DIGEST"
```

This narrow timeout update requires its own nginx reload, unlike static-root
symlink changes. Do not copy entire repository nginx templates over live files.

## CI and Git-only handoff

All three repos contain `.github/workflows/verify.yml`, triggered on push and
pull request. These are **verification workflows, not deployment workflows**;
no deploy job was found in the checked-in workflow inventory.

- Picrete: rustfmt, locked Cargo tests with PostgreSQL/Redis services, Python
  enrichment tests. Current commit: [passed](https://github.com/dorogao5/Picrete/actions/runs/34664309824).
- Studio: locked uv backend/ops pytest; Node 24 frontend tests/build. Current
  commit: [failed backend pytest](https://github.com/dorogao5/Studio-Picrete/actions/runs/34664314246);
  frontend job passed. The individual failing test cause was not audited.
- Front: Node 24 install, lint, math tests, build. Current commit:
  [passed](https://github.com/dorogao5/Front-Picrete/actions/runs/34665316913).

Future release gate: review and explicitly commit the intended fixes in each
repo, including the API builder revision fix above; wait for successful CI for
the **exact three candidate SHAs**. Do not include unrelated concurrent edits.
Do not force-push, ship local source archives, rsync source, or hot-edit server
code. Only Git push → server clone/pull transports application source.

After those commits exist, run locally (not executed during this audit):

```bash
set -euo pipefail
cd /Users/doroga/Documents/projects/rust-picrete
for repo in Picrete Studio-Picrete Front-Picrete; do
  test "$(git -C "$repo" branch --show-current)" = main
  test -z "$(git -C "$repo" status --porcelain)"
  git -C "$repo" push origin main
  git -C "$repo" rev-parse HEAD
  git -C "$repo" ls-remote origin refs/heads/main
  gh run list --repo "dorogao5/$repo" --commit "$(git -C "$repo" rev-parse HEAD)" \
    --workflow verify.yml --limit 5 --json headSha,status,conclusion,url
done
```

Record the three full SHAs after successful CI. Missing/pending/failed CI is a
stop condition. Tests must use isolated test services and mocked providers;
never point a test suite at production env or run live generation/grading probes.

## Future server preparation and build (writes, but no service switch)

The following is an operator Bash recipe. **Do not execute as part of this
audit.** Fill in a new release directory name and three approved full SHAs.
The placeholders deliberately fail validation. Run on the server in one Bash
session; retain the variables for the later switch. No secrets are shell args.

```bash
set -euo pipefail
umask 077
OLD=/srv/picrete/releases/20260912-physchem-git
RELEASE_ID=REPLACE_WITH_NEW_RELEASE_NAME
API_SHA=REPLACE_WITH_FULL_APPROVED_SHA
STUDIO_SHA=REPLACE_WITH_FULL_APPROVED_SHA
FRONT_SHA=REPLACE_WITH_FULL_APPROVED_SHA
[[ "$RELEASE_ID" =~ ^[0-9]{8}-[a-z0-9-]+$ ]]
for rev in "$API_SHA" "$STUDIO_SHA" "$FRONT_SHA"; do
  [[ "$rev" =~ ^[0-9a-f]{40}$ ]]
done
NEW="/srv/picrete/releases/$RELEASE_ID"
test ! -e "$NEW"
test -r /tmp/picrete-release.env
test -r /tmp/studio-release.env
sudo install -d -m 0755 -o doroga -g docker "$NEW"

checkout_release() {
  local repo="$1" expected="$2"
  (umask 022; git clone --branch main --single-branch "https://github.com/dorogao5/$repo.git" "$NEW/$repo")
  git -C "$NEW/$repo" pull --ff-only origin main
  test "$(git -C "$NEW/$repo" rev-parse HEAD)" = "$expected"
  test -z "$(git -C "$NEW/$repo" status --porcelain)"
}
checkout_release Picrete "$API_SHA"
checkout_release Studio-Picrete "$STUDIO_SHA"
checkout_release Front-Picrete "$FRONT_SHA"
# Abort if main advanced: approve the new SHA or use a reviewed release ref.
# Do not reset/reuse a live checkout to make it match.

# Preserve the observed effective env basis; remove only release-control keys.
# These files use ordinary KEY=value assignments; review formatting privately
# before running these filters if configuration management changes.
sed -E '/^(export )?(RELEASE_SHA|COMPOSE_PROJECT_NAME|TASK_BANK_VOLUME)=/d' \
  /tmp/picrete-release.env > "$NEW/Picrete/.env"
printf '\nRELEASE_SHA=%s\nCOMPOSE_PROJECT_NAME=app\nTASK_BANK_VOLUME=%s\n' \
  "$API_SHA" "$NEW/Picrete/tasks" >> "$NEW/Picrete/.env"
sed -E '/^(export )?(RELEASE_SHA|BUILD_REVISION|COMPOSE_PROJECT_NAME)=/d' \
  /tmp/studio-release.env > "$NEW/Studio-Picrete/.env"
printf '\nRELEASE_SHA=%s\nCOMPOSE_PROJECT_NAME=studio-picrete\n' \
  "$STUDIO_SHA" >> "$NEW/Studio-Picrete/.env"
chmod 0600 "$NEW/Picrete/.env" "$NEW/Studio-Picrete/.env"
test -d "$NEW/Picrete/tasks/Sviridov_tasks"
test ! -e "$NEW/Studio-Picrete/data"
ln -s "$OLD/Studio-Picrete/data" "$NEW/Studio-Picrete/data"
# Retain OLD while this data symlink exists; do not garbage-collect it.

api_compose() {
  RELEASE_SHA="$API_SHA" docker compose -p app \
    --project-directory "$NEW/Picrete" --env-file "$NEW/Picrete/.env" \
    -f "$NEW/Picrete/docker-compose.prod.yml" "$@"
}
studio_compose() {
  RELEASE_SHA="$STUDIO_SHA" docker compose -p studio-picrete \
    --project-directory "$NEW/Studio-Picrete" --env-file "$NEW/Studio-Picrete/.env" \
    -f "$NEW/Studio-Picrete/docker-compose.yml" "$@"
}
api_compose config --quiet
studio_compose config --quiet
api_compose build api
studio_compose build studio-api
test "$(docker image inspect "picrete-runtime:$API_SHA" --format \
  '{{index .Config.Labels "org.opencontainers.image.revision"}}')" = "$API_SHA"
test "$(docker image inspect "picrete-studio:$STUDIO_SHA" --format \
  '{{index .Config.Labels "org.opencontainers.image.revision"}}')" = "$STUDIO_SHA"

# Pin the resolved build-tool image locally for both frontend builds.
docker pull node:24-bookworm
NODE_IMAGE=$(docker image inspect node:24-bookworm --format '{{.Id}}')
for build_dir in "$NEW/Front-Picrete" "$NEW/Studio-Picrete/frontend"; do
  docker run --rm --user "$(id -u):$(id -g)" \
    -e npm_config_cache=/tmp/npm-cache -v "$build_dir:$build_dir" -w "$build_dir" \
    "$NODE_IMAGE" bash -euc 'node --version; npm --version; npm ci; npm run build'
done
export NEW API_SHA STUDIO_SHA FRONT_SHA NODE_IMAGE
python3 - <<'PY'
import json, os, pathlib, subprocess
root = pathlib.Path(os.environ['NEW'])
for rel, var in [('Front-Picrete/dist', 'FRONT_SHA'),
                 ('Studio-Picrete/frontend/dist', 'STUDIO_SHA')]:
    assert json.loads((root/rel/'build-info.json').read_text())['revision'] == os.environ[var]
images = {}
for name, var in [('picrete-runtime', 'API_SHA'), ('picrete-studio', 'STUDIO_SHA')]:
    tag = name + ':' + os.environ[var]
    images[tag] = subprocess.check_output(
        ['docker', 'image', 'inspect', tag, '--format', '{{.Id}}'], text=True).strip()
manifest = {'revisions': {k: os.environ[k] for k in ('API_SHA','STUDIO_SHA','FRONT_SHA')},
            'images': images, 'node_image': os.environ['NODE_IMAGE'],
            'previous_release': '/srv/picrete/releases/20260912-physchem-git',
            'status': 'built-not-deployed'}
(root/'release-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
PY
for repo in Picrete Studio-Picrete Front-Picrete; do
  test -z "$(git -C "$NEW/$repo" status --porcelain)"
done
```

This stage must finish before activation. Validate the API builder-stage fix by
reviewing the candidate Dockerfile; the full-SHA label alone does not prove the
compiled `/version` value. Ensure image tags are not overwritten after recording
their IDs. Run Compose with `--no-build --pull never` at activation.

## Schema migration and activation procedure

Picrete uses SQLx migrations from `migrations/*.sql`; API, worker and Telegram
binary all apply them on startup. **There is no separate migration-only binary
in this image.** Start the API before the worker. API startup also imports the
task bank and ensures the superuser, so startup itself is a data-writing step.

At audit, all **11** checked-in migrations succeeded in `picrete_db`, ending at
`20260912000000_physchem_trainer_difficulty.sql`. All 11 SHA-384 checksums matched
the server SQLx ledger. That last migration corrects physical chemistry item and
trainer JSON difficulty and increments trainer revision. **Do not rerun its SQL
manually or edit an applied migration.** Upcoming additional corrections require
new, versioned SQL migrations committed through Git. The observed local edits
contained no migration changes at audit time.

Studio uses SQLAlchemy `Base.metadata.create_all`, `ensure_postgres_columns` and
`ensure_fts` in `backend/app/main.py` lifespan, not Alembic. PostgreSQL already has
the `vector` extension. Studio startup also bootstraps/seeds config and reconciles
interrupted jobs. Do not run `scripts/migrate_sqlite_to_pg.py`: this installation
already uses PostgreSQL. Existing SQLite files under shared storage are not proof
that SQLite is the live database.

Before activation, inspect the Git diff of Picrete migrations and Studio
`app/main.py`, models and FTS code between old and candidate SHAs. Establish a
maintenance window and quiesce user writes/queues through the application's
normal operational process. At audit the checked practice queue and Studio
generation batches had zero queued/running and zero running rows respectively;
this is not a guarantee about future work or other queues. Do not exercise
generation, grading, OCR, tutor, publish or content-import endpoints as probes.

The following commands stop writers, back up both databases, then start the new
services. They cause a maintenance interruption. Keep the Bash variables and
functions from preparation; explicitly re-establish them if using a new shell.

```bash
# FUTURE ACTIVATION ONLY, after maintenance/quiescence and release review.
test "$(readlink -f /srv/picrete/Front-Picrete)" = "$OLD/Front-Picrete"
test "$(readlink -f /var/www/picrete-studio)" = "$OLD/Studio-Picrete/frontend/dist"
api_compose stop worker api
studio_compose stop studio-api

# Private, custom-format PostgreSQL backups; never stream dumps to the terminal.
BACKUP="/srv/picrete/backups/$RELEASE_ID"
sudo install -d -m 0700 -o postgres -g postgres "$BACKUP"
sudo -u postgres pg_dump -Fc -d picrete_db -f "$BACKUP/picrete_db.dump"
sudo -u postgres pg_dump -Fc -d picrete_studio -f "$BACKUP/picrete_studio.dump"
sudo -u postgres pg_restore --list "$BACKUP/picrete_db.dump" >/dev/null
sudo -u postgres pg_restore --list "$BACKUP/picrete_studio.dump" >/dev/null
# Listing verifies archive readability, not a full restore. Rehearse restoration
# into isolated databases for any release with material schema/data changes.

studio_compose up -d --no-build --pull never --wait --wait-timeout 180 studio-api
api_compose up -d --no-build --pull never --wait --wait-timeout 180 api
sudo -u postgres psql -X -v ON_ERROR_STOP=1 -d picrete_db -c \
  'SELECT version, description, success FROM _sqlx_migrations ORDER BY version'
# Confirm every candidate migration is present/successful and checksums match
# before proceeding; health alone is not a substitute for this comparison.
curl -fsS --max-time 10 http://127.0.0.1:8000/readyz >/dev/null
python3 - <<'PY'
import json, os, urllib.request
for port, key in [(8000,'API_SHA'), (8100,'STUDIO_SHA')]:
    with urllib.request.urlopen(f'http://127.0.0.1:{port}/version', timeout=10) as r:
        assert json.load(r)['revision'] == os.environ[key], f'{key}: version mismatch'
PY
api_compose up -d --no-build --pull never worker
```

If backup, startup, migration, checksum or version verification fails, stop here;
do not switch frontend roots or start the worker after a failed gate. A failed
startup may already have applied migrations; use the rollback rules below.
Once the worker starts it resumes real queued work, which may call providers;
this is normal deployment behavior, not an audit/test step.

Nginx serves hashed assets from **shared directories**, not the new `dist` roots.
Publish assets before switching HTML, keep old hashes for cached pages and
rollback, and abort on a same-name/different-content collision. The following
script copies only missing assets and atomically replaces the three symlinks;
the symlink replacements are individually atomic, not one global transaction.

```bash
sudo env NEW="$NEW" python3 - <<'PY'
import os, pathlib, shutil
root = pathlib.Path(os.environ['NEW'])
for relative, dest in [('Front-Picrete/dist/assets', '/srv/picrete/shared/frontend/assets'),
                       ('Studio-Picrete/frontend/dist/assets', '/srv/picrete/shared/studio/assets')]:
    src, dst = root/relative, pathlib.Path(dest)
    assert src.is_dir()
    for item in src.rglob('*'):
        if not item.is_file(): continue
        target = dst/item.relative_to(src)
        target.parent.mkdir(parents=True, exist_ok=True, mode=0o755)
        if target.exists():
            assert target.read_bytes() == item.read_bytes(), f'asset collision: {target.name}'
        else:
            shutil.copyfile(item, target)
            target.chmod(0o644)
for link, target in [('/srv/picrete/Front-Picrete', root/'Front-Picrete'),
                     ('/var/www/picrete-studio', root/'Studio-Picrete/frontend/dist'),
                     ('/srv/picrete/app', root/'Picrete')]:
    p = pathlib.Path(link)
    assert p.is_symlink(), f'expected symlink: {p}'
    tmp = p.with_name(p.name + '.next-' + root.name)
    assert not tmp.exists() and not tmp.is_symlink()
    tmp.symlink_to(target)
    os.replace(tmp, p)
PY
sudo nginx -t
# No nginx reload or systemctl restart is needed for this symlink switch.
```

The new Picrete `.env` also makes the existing boot unit resolve the correct
image/project via `/srv/picrete/app`. Its unit file remains unchanged. Inspect its
resolved working directory and env-file existence before ending maintenance.
Studio relies on Docker's `restart: always`; no Studio systemd unit was found.

## Verification, release record and rollback

The separately requested curated-content script is
`Studio-Picrete/ops/release_physical_chemistry.py`. Its default dry-run requires
`PICRETE_ACCESS_TOKEN` supplied privately to the container; it never logs in or
performs HTTP writes. `--apply` additionally requires `--expect-digest` from the
reviewed plan, and can authenticate with externally supplied
`PICRETE_USERNAME`/`PICRETE_PASSWORD` if no bearer token was supplied. Use the
actual Codex review timestamp with `--reviewed-at`; the default accountable
Studio user is doroga, with approval explicitly attributed to Codex on behalf
of doroga. Do not put token/password values into shell arguments or this document.

Once the Git-built image is available, with the bearer exported securely in the
operator shell, a future dry-run command is:

```bash
docker exec -e PICRETE_ACCESS_TOKEN studio-picrete-studio-api-1 \
  /app/.venv/bin/python /app/ops/release_physical_chemistry.py \
  --dry-run --reviewed-at "$ACTUAL_REVIEW_TIMESTAMP"
```

Append `--apply --expect-digest "$REVIEWED_CONTENT_DIGEST"` instead of `--dry-run`
only for the separately approved content activation. This command is not part
of read-only deployment verification. It writes Studio first, then imports via
the Picrete internal bridge and publishes the assistant/trainer using revision
checks; these are resumable stages, not a cross-database atomic transaction.
No uploads, model calls or fabricated Playground successes are involved.

Local checks validated 64 tasks (37 originals + 4 additions + 23 Eremin), 14
artifact hashes, and 15 topic/level templates. An in-memory test using read-only
server fixtures preserved all 37 Studio IDs and trainer section IDs, produced
six canonical sheets and four matching role families, and returned zero changes
on its second plan. The runtime-policy helper was mocked in that fixture test;
this is not a live content dry-run, a model test, or proof of deployment success.

Final r2 recheck: reviewed-bank has 41 tasks (14 easy, 26 medium, 1 hard);
Eremin has 23 (15 easy, 8 medium). Exact profile application, preservation of the
37 original Studio IDs, retirement of the two superseded large sheets, updating
PHYS-00 in place, and a second plan with zero changes were verified again against
fresh read-only server fixtures. Ruff, pure regression checks, all 14 artifact
validations and runbook Bash syntax checks passed. No live release-script run was
performed; that requires the new Git-built image, bearer and actual review time.

Repository handoff detail: Picrete `.gitignore` ignores `docs`. To include this
runbook in a future reviewed commit, explicitly stage only this file using
`git add -f docs/physchem-deployment.md`; it has not been staged by this audit.

After activation, check container **image IDs and full revision labels**, both
API versions, both frontend build-info files, actual HTML asset URLs and health.
These GETs do not invoke paid models:

```bash
docker inspect app-api-1 app-worker-1 studio-picrete-studio-api-1 \
  --format '{{.Name}} image={{.Image}} status={{.State.Status}}'
curl -fsS --max-time 10 https://picrete.com/healthz
curl -fsS --max-time 10 https://picrete.com/readyz
curl -fsS --max-time 10 https://picrete.com/version
curl -fsS --max-time 10 https://dev.picrete.com/healthz
curl -fsS --max-time 10 https://dev.picrete.com/version
curl -fsS --max-time 10 https://picrete.com/build-info.json
curl -fsS --max-time 10 https://dev.picrete.com/build-info.json
```

Require image IDs to equal the manifest, API + worker to use the same runtime
image, API revision = `API_SHA`, Studio API + UI revision = `STUDIO_SHA`, and
student UI revision = `FRONT_SHA`. Fetch both HTML pages and GET each referenced
`/assets/...` script/stylesheet; expect 200 and correct content type. Check `.ru`
redirects separately. A healthy worker process does not prove task execution;
avoid a live paid job as a smoke test. Record verification time, backup locations,
effective Compose paths and migration versions alongside the manifest; mark it
deployed only after the checks pass. Do not include env values in the manifest.

Keep old images, old checkout, old env files and shared assets. For an
application-only rollback **after confirming backward-compatible database state**,
stop the new worker and use the old explicit paths/tags:

```bash
api_compose stop worker
RELEASE_SHA=01a8547 docker compose -p studio-picrete \
  --project-directory "$OLD/Studio-Picrete" --env-file "$OLD/Studio-Picrete/.env" \
  -f "$OLD/Studio-Picrete/docker-compose.yml" \
  up -d --no-build --pull never --wait --wait-timeout 180 studio-api
RELEASE_SHA=868c12a docker compose -p app \
  --project-directory "$OLD/Picrete" --env-file /tmp/picrete-release.env \
  -f "$OLD/Picrete/docker-compose.prod.yml" \
  up -d --no-build --pull never --wait --wait-timeout 180 api
RELEASE_SHA=868c12a docker compose -p app \
  --project-directory "$OLD/Picrete" --env-file /tmp/picrete-release.env \
  -f "$OLD/Picrete/docker-compose.prod.yml" up -d --no-build --pull never worker
```

Verify the old tags still resolve to the recorded audit image IDs **before**
rollback. Restore the two frontend symlinks atomically to their recorded old
targets using the same guarded replacement pattern. Do not blindly restore the
known-broken old boot symlink: provision a private rollback `.env` with explicit
old `RELEASE_SHA` and `COMPOSE_PROJECT_NAME=app` in the old Picrete directory before
pointing `/srv/picrete/app` there. This is an operational rollback change, not
something done by this audit. Recheck health and image IDs; the old version
endpoints are known to be placeholders and old Studio UI has the recorded drift.

There are no reverse SQLx migrations in this release, and Studio startup DDL has
no versioned downgrade command. If schema/data changes are incompatible, keep
writers stopped and restore **both** approved backups through a separately
reviewed recovery procedure; that loses writes after the backup and must not be
automated as an unconditional rollback. Do not delete SQLx ledger rows, manually
reapply the difficulty migration, or restore into a running production database.

Scope limits: no live deployment/rebuild, CI test execution, backup/restore test,
paid-model test or content publication was performed in this audit. GitHub
branch-protection settings, external deployment integrations and other hosts
were not audited. The SSH alias currently disables host-key checking and uses
`UserKnownHostsFile=/dev/null`; no SSH configuration was changed.

## Essential-tools private bridge follow-up (prepared, not activated)

This section supersedes the earlier proposed loopback port 8180 configuration.
On the production Docker host an `internal: true` bridge retained the requested
port binding in HostConfig but installed no published port. A read-only host GET
to the existing bridge address `http://172.19.0.2:8080/health` succeeded.
Use the tracked Studio `docker-compose.essential-tools.yml` instead: new network
`tools-private-v2`, subnet `172.30.80.0/28`, gateway `172.30.80.1`, service static IP
`172.30.80.2`. No published ports, no host networking for the gateway, no manual
iptables changes. `internal: true` remains enabled. The local host can reach this
bridge, so bearer authentication remains mandatory; this is not an air gap.

The subnet did not overlap the audited VPC `10.130.0.0/24` or Docker networks
`172.17.0.0/16`, `172.18.0.0/16`, `172.19.0.0/16`. Before activation recheck
`ip -4 route` and every Docker network IPAM configuration. Abort on overlap;
do not resolve it by disabling isolation. The new network name avoids mutating
the existing bridge. Recreate only the gateway service on the new bridge after
the final Git commit/CI gate; do not run a broad Compose `down`.

Previous source root: `/srv/picrete/releases/20260912-physchem-r3`.
Final tools release target: `/srv/picrete/releases/20260912-tools-r5`.
Prepare Picrete and Studio-Picrete there via Git at their separately approved
full SHAs, preserving the private live env values and existing data/task mounts.
Build and activate using that same root; do not mix candidate source/Compose
paths with the old r3 checkout. Front remains on its existing UI release.
Private CSV: `/srv/picrete/shared/essential_skills/reference_db.csv`, UID65532,
mode0400, read-only bind `/data/reference_db.csv`. SHA256 must equal
`fb82fd2b7ff2228064883834828c824eadb9adf038c2182e774a1b96fc75b3c2`.
The CSV is authorized data-only transfer, never Git content, image content or
public CI artifact. Gateway env: `/srv/picrete/shared/essential_skills/gateway.env`
(0600), shared token copied privately to both application `.env` files (0600).
Preserve all other live values and back up env files before mechanical replacement.
Set `ESSENTIAL_TOOLS_GATEWAY_URL=http://172.30.80.2:8080` in all three private env
files at the approved deployment, replacing the obsolete loopback URL. Never
print their content or rendered Compose environment. Set exact final RELEASE_SHA
separately for each repository/image; do not infer the target from a newer HEAD.
Picrete Compose now explicitly passes URL/token/max rounds/max calls to API and
worker. Studio already uses `env_file: .env` and tested unprefixed aliases.

After Git pull/build, successful CI, current backups, and permission to activate:

```sh
sudo docker compose -p picrete-essential-tools \
  --env-file /srv/picrete/shared/essential_skills/gateway.env \
  -f /srv/picrete/releases/20260912-tools-r5/Studio-Picrete/docker-compose.essential-tools.yml \
  up -d --no-build --pull never --wait essential-tools
```

Gate application activation on host GET `/health`, unauthenticated POST401,
authenticated deterministic calculator/reference checks (suppress private record
values), actual no published ports, read-only root/data, UID65532, dropped caps,
no-new-privileges, pids32/memory2304MiB/CPU2, and blocked external egress. Verify
the new static IP and internal network by inspect, not just Compose text.
Then activate the final API/worker/Studio images using the normal runbook gates;
compare actual runtime URL/token nonempty/equality as booleans only. Leave Front
unchanged unless explicitly requested. All role tools flags remain false until
main authorizes the separate content helper apply; deploying code is not that
authorization. Do not run paid model probes as deployment health checks.
