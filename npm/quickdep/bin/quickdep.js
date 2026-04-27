#!/usr/bin/env node

const { spawnSync } = require("node:child_process");
const { existsSync } = require("node:fs");
const path = require("node:path");

const executableName = process.platform === "win32" ? "quickdep.exe" : "quickdep";
const binaryPath = path.join(__dirname, "..", "vendor", executableName);

if (!existsSync(binaryPath)) {
  console.error(
    "QuickDep binary is missing. Reinstall @embedclaw/quickdep or run `npm rebuild @embedclaw/quickdep`."
  );
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
