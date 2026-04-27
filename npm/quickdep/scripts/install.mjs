import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";
import https from "node:https";

import AdmZip from "adm-zip";
import tar from "tar";

const packageRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(
  fs.readFileSync(path.join(packageRoot, "package.json"), "utf8")
);

const ownerRepo = process.env.QUICKDEP_REPOSITORY || "embedclaw/QuickDep";
const version = packageJson.version;
const releaseBase =
  process.env.QUICKDEP_RELEASE_BASE_URL ||
  `https://github.com/${ownerRepo}/releases/download/v${version}`;

const vendorDir = path.join(packageRoot, "vendor");
fs.mkdirSync(vendorDir, { recursive: true });

const asset = resolveAsset();
const archivePath = path.join(os.tmpdir(), asset.name);

console.log(`Downloading ${asset.name} from ${releaseBase}`);
await downloadFile(`${releaseBase}/${asset.name}`, archivePath);
await extractArchive(archivePath, vendorDir, asset);
fs.rmSync(archivePath, { force: true });

const binaryPath = path.join(vendorDir, asset.binaryName);
if (process.platform !== "win32") {
  fs.chmodSync(binaryPath, 0o755);
}

function resolveAsset() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "darwin" && arch === "arm64") {
    return {
      name: "quickdep-darwin-aarch64.tar.gz",
      binaryName: "quickdep",
      archive: "tar.gz",
    };
  }

  if (platform === "darwin" && arch === "x64") {
    return {
      name: "quickdep-darwin-x86_64.tar.gz",
      binaryName: "quickdep",
      archive: "tar.gz",
    };
  }

  if (platform === "linux" && arch === "x64") {
    return {
      name: "quickdep-linux-x86_64.tar.gz",
      binaryName: "quickdep",
      archive: "tar.gz",
    };
  }

  if (platform === "linux" && arch === "arm64") {
    return {
      name: "quickdep-linux-aarch64.tar.gz",
      binaryName: "quickdep",
      archive: "tar.gz",
    };
  }

  if (platform === "win32" && arch === "x64") {
    return {
      name: "quickdep-windows-x86_64.zip",
      binaryName: "quickdep.exe",
      archive: "zip",
    };
  }

  throw new Error(`Unsupported platform: ${platform}/${arch}`);
}

async function downloadFile(url, destination) {
  await new Promise((resolve, reject) => {
    const request = https.get(url, (response) => {
      if (response.statusCode && response.statusCode >= 400) {
        reject(new Error(`Download failed with status ${response.statusCode}`));
        response.resume();
        return;
      }

      const file = fs.createWriteStream(destination);
      pipeline(response, file).then(resolve).catch(reject);
    });

    request.on("error", reject);
  });
}

async function extractArchive(archivePath, vendorDir, asset) {
  fs.rmSync(vendorDir, { recursive: true, force: true });
  fs.mkdirSync(vendorDir, { recursive: true });

  if (asset.archive === "tar.gz") {
    await tar.x({
      file: archivePath,
      cwd: vendorDir,
    });
    return;
  }

  if (asset.archive === "zip") {
    const zip = new AdmZip(archivePath);
    zip.extractAllTo(vendorDir, true);
    return;
  }

  throw new Error(`Unsupported archive type: ${asset.archive}`);
}
