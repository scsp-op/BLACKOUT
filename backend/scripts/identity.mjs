// Outbound identity for the one-off generator scripts, mirroring
// backend/src/util/http.rs so the Node side and the Rust side say the same
// thing about who this is.
//
// These scripts hit TeleGeography, PeeringDB and the ISO country list from a
// developer's machine rather than from a deployment, but they are still
// somebody else's free API, and PeeringDB in particular throttles
// unauthenticated callers hard (see gen_ixp_data.mjs). Identifying is the
// cheap half of being a good client; PEERINGDB_API_KEY is the other half.

import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))

const DEFAULT_DEPLOYMENT_ID = 'local'
const DEFAULT_CONTACT = 'https://github.com/moumenalaoui/BLACKOUT'
const MAX_ID_LEN = 48

// Same charset as util::http::sanitize — a stray newline in an env var must
// not be able to produce an invalid header.
const sanitize = (raw) => (raw ?? '').replace(/[^A-Za-z0-9._-]/g, '').slice(0, MAX_ID_LEN)

const version = (() => {
  try {
    const cargo = readFileSync(resolve(HERE, '../Cargo.toml'), 'utf8')
    return cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1] ?? '0'
  } catch {
    return '0'
  }
})()

export const deploymentId = sanitize(process.env.DEPLOYMENT_ID?.trim()) || DEFAULT_DEPLOYMENT_ID
export const contact = process.env.DEPLOYMENT_CONTACT?.trim() || DEFAULT_CONTACT

export const userAgent = (component) =>
  `blackout-${component}/${version} (+${contact}; id=${deploymentId})`

export const headers = (component, extra = {}) => ({
  'User-Agent': userAgent(component),
  ...extra,
})
