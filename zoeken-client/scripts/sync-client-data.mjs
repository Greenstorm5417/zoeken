#!/usr/bin/env bun
/** Copy ClearURLs tracker rules from zoeken-data into the SPA (never ahmia). */
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const src = join(root, "zoeken/zoeken-data/data/tracker_patterns.json");
const destDir = join(root, "zoeken-client/src/lib/generated");
const dest = join(destDir, "tracker_patterns.json");

mkdirSync(destDir, { recursive: true });

if (existsSync(src)) {
	copyFileSync(src, dest);
	console.log("synced tracker_patterns.json → src/lib/generated/");
} else if (existsSync(dest)) {
	console.warn(
		"tracker_patterns.json source missing; keeping existing SPA copy (Docker/client-only builds)",
	);
} else {
	console.error(
		"missing tracker_patterns.json (expected zoeken/zoeken-data/data/ or src/lib/generated/)",
	);
	process.exit(1);
}
