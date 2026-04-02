import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { describe, expect, it } from "vitest";

const TEST_DIR = path.dirname(fileURLToPath(import.meta.url));
const FRONTEND_ROOT = path.resolve(TEST_DIR, "..");
const TOML_BRIDGE_URL = pathToFileURL(path.resolve(FRONTEND_ROOT, "src/toml.js")).href;

describe("toml bridge", () => {
  it("loads when global is undefined", () => {
    const script = `
      delete globalThis.global;
      const mod = await import(${JSON.stringify(TOML_BRIDGE_URL)});
      process.stdout.write(mod.default.stringify({ ok: true }));
    `;

    const output = execFileSync(process.execPath, ["--input-type=module", "-e", script], {
      cwd: FRONTEND_ROOT,
      encoding: "utf8",
    });

    expect(output).toContain("ok = true");
  });
});
