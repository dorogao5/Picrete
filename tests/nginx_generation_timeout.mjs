import { readFileSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import assert from "node:assert/strict";
import test from "node:test";

// Validate the real location hierarchy with nginx, without production TLS files,
// host ports or upstream requests. Requires the local nginx:alpine Docker image.
for (const domain of ["com", "ru"]) {
  test(`nginx picrete.${domain}: generation timeout config parses`, () => {
    const source = readFileSync(new URL(`../deploy/nginx-picrete.${domain}.conf`, import.meta.url), "utf8");
    const config = source
      .replace(/^\s*(?:include|ssl_\w+)\s+[^;]+;\s*$/gm, "")
      .replace(/listen ([^;]+?) ssl(?: http2)?;/g, "listen $1;");
    const directory = mkdtempSync(join(tmpdir(), "picrete-nginx-test-"));
    try {
      writeFileSync(join(directory, "nginx.conf"), `events {}\nhttp {\n${config}\n}\n`);
      const result = spawnSync("docker", ["run", "--rm", "--network", "none", "--entrypoint", "nginx",
        "-v", `${directory}:/test:ro`, "nginx:alpine", "-t", "-c", "/test/nginx.conf"], { encoding: "utf8" });
      assert.equal(result.status, 0, result.stderr || result.error?.message);
      assert.match(result.stderr, /test is successful/);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
}
