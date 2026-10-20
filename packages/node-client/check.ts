// Fails where the committed types differ from what `bun run generate` makes of docs/openapi.json now, so the
// package never drifts from the document it is generated from.

import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const folder = mkdtempSync(join(tmpdir(), "node-client-"));
const fresh = join(folder, "schema.ts");

try {
  const made = Bun.spawnSync(["bun", "x", "openapi-typescript", "../../docs/openapi.json", "--output", fresh], {
    cwd: import.meta.dir,
    stdout: "ignore",
    stderr: "inherit",
  });

  if (made.exitCode !== 0) {
    console.error("openapi-typescript could not read docs/openapi.json");
    process.exit(1);
  }

  if (readFileSync(fresh, "utf8") !== readFileSync(join(import.meta.dir, "src", "schema.ts"), "utf8")) {
    console.error("src/schema.ts differs from docs/openapi.json; run bun run generate");
    process.exit(1);
  }
} finally {
  rmSync(folder, { recursive: true, force: true });
}
