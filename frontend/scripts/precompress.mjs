// Writes a gzipped sibling (`foo.js.gz`) next to every compressible file in the
// built bundle, so the server can send it as-is instead of gzipping on every
// request. Run as part of `npm run build`, after `vite build`.
//
// Without this, the backend's CompressionLayer gzips each response on the fly:
// Cesium.js alone is 5.9 MB and cost ~150 ms of CPU per first-time visitor, on
// a 2-vCPU VM that also serves the API. ServeDir (`precompressed_gzip`, see
// backend/src/main.rs) serves `foo.js.gz` when the client accepts gzip and
// falls back to `foo.js` otherwise, so a missing `.gz` only costs the old
// on-the-fly path, never a broken response.
//
// Images are skipped: PNG/JPEG/GIF are already compressed. Files under 1 KB,
// and any whose gzip is not meaningfully smaller, are left alone.

import { gzipSync, constants } from 'node:zlib'
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { extname, join } from 'node:path'

const ROOT = process.argv[2] ?? 'dist'
const EXTENSIONS = new Set(['.html', '.js', '.mjs', '.css', '.json', '.svg', '.xml', '.wasm', '.txt'])
const MIN_BYTES = 1024

function* files(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) yield* files(path)
    else if (entry.isFile()) yield path
  }
}

let count = 0
let before = 0
let after = 0
for (const path of files(ROOT)) {
  if (!EXTENSIONS.has(extname(path)) || statSync(path).size < MIN_BYTES) continue
  const raw = readFileSync(path)
  const gz = gzipSync(raw, { level: constants.Z_BEST_COMPRESSION })
  if (gz.length > raw.length * 0.9) continue
  writeFileSync(`${path}.gz`, gz)
  count += 1
  before += raw.length
  after += gz.length
}

const mb = (n) => (n / 1e6).toFixed(1)
console.log(`precompress: ${count} files, ${mb(before)} MB -> ${mb(after)} MB gzipped`)
