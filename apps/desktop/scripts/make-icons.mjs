#!/usr/bin/env node
/**
 * Regenerate src-tauri/icons from the two logo files in public/.
 *
 *   public/logo.svg        the drawing edge to edge  -> every icon except icon.icns
 *   public/logo-macos.svg  the same drawing, padded  -> icon.icns only
 *
 * Every platform but macOS draws an app icon edge to edge: the Windows taskbar, Linux launchers
 * and the browser favicon all give the file the whole cell. macOS instead lays its icons on a
 * 1024pt grid where the rounded square fills 824 of it, and it does not mask third-party icons
 * to match, so an edge-to-edge tile sits visibly larger than every neighbour in the Dock. One
 * file cannot satisfy both, and padding the only file shrinks the icon everywhere else.
 *
 * So `tauri icon` runs twice. The first pass renders logo.svg into src-tauri/icons and is the
 * icon set of record. The second renders logo-macos.svg into a temporary directory, and only its
 * icon.icns is copied over the first pass.
 *
 * The two files are meant to differ in their viewBox and comments alone. The script checks that
 * before rendering anything and stops if the drawings have drifted apart, so an edit made to one
 * file cannot quietly ship a different icon on macOS.
 *
 * A third file, public/tray-template.svg, is the macOS menu-bar icon: the same platters as one black
 * silhouette with no tile, rendered to src-tauri/icons/tray/44x44.png. It is a different drawing
 * rather than a copy, so the drift check above does not cover it (T168).
 *
 * Usage:
 *   npm run icons
 *   node scripts/make-icons.mjs
 *
 * The how and why for people, with pictures of what goes wrong: docs/standards/desktop/app-icon.md.
 */
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const edgeToEdge = join(root, "public", "logo.svg");
const padded = join(root, "public", "logo-macos.svg");
const trayTemplate = join(root, "public", "tray-template.svg");
const iconsDir = join(root, "src-tauri", "icons");
const tauriCli = join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");

/** The canvas each file must declare. The macOS one is a 64-unit tile on 79.5, i.e. 824/1024. */
const EDGE_TO_EDGE_VIEWBOX = 'viewBox="0 0 64 64"';
const PADDED_VIEWBOX = 'viewBox="-7.75 -7.75 79.5 79.5"';

/** The drawing alone: no comments, no viewBox, whitespace collapsed. Two files whose drawings
 *  match give the same string here, whatever their comments say. */
function drawingOf(svg) {
  return svg
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(/viewBox="[^"]*"/, "")
    .replace(/\s+/g, " ")
    .trim();
}

function fail(message) {
  console.error(`make-icons: ${message}`);
  process.exit(1);
}

function tauriIcon(svgPath, outDir, pngSizes = []) {
  const args = [tauriCli, "icon", svgPath];
  if (outDir) args.push("--output", outDir);
  for (const size of pngSizes) args.push("--png", String(size));
  execFileSync(process.execPath, args, { cwd: root, stdio: "inherit" });
}

const edgeSvg = readFileSync(edgeToEdge, "utf8");
const paddedSvg = readFileSync(padded, "utf8");
const edgeName = relative(root, edgeToEdge);
const paddedName = relative(root, padded);

if (!edgeSvg.includes(EDGE_TO_EDGE_VIEWBOX)) {
  fail(`${edgeName} must declare ${EDGE_TO_EDGE_VIEWBOX}.`);
}
if (!paddedSvg.includes(PADDED_VIEWBOX)) {
  fail(`${paddedName} must declare ${PADDED_VIEWBOX}.`);
}
if (drawingOf(edgeSvg) !== drawingOf(paddedSvg)) {
  fail(
    `${edgeName} and ${paddedName} draw different things. They may differ only in viewBox and ` +
      `comments; copy the drawing from the one you edited into the other, then run again.`,
  );
}

console.log(`Rendering ${edgeName} edge to edge into ${relative(root, iconsDir)} …`);
tauriIcon(edgeToEdge);

const scratch = mkdtempSync(join(tmpdir(), "mixlab-icons-"));
try {
  const paddedOut = join(scratch, "icons");
  console.log(`Rendering ${paddedName} with the Dock margin, keeping only icon.icns …`);
  tauriIcon(padded, paddedOut);
  copyFileSync(join(paddedOut, "icon.icns"), join(iconsDir, "icon.icns"));
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

/* 44 pixels: macOS draws a status item 18pt tall and scales the image to it, so one image twice the
   1x height stays sharp on a Retina menu bar. */
console.log(`Rendering ${relative(root, trayTemplate)} into ${relative(root, join(iconsDir, "tray"))} …`);
tauriIcon(trayTemplate, join(iconsDir, "tray"), [44]);

console.log("Done: every icon is edge to edge, icon.icns carries the macOS margin.");
