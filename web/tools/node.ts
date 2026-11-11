/** The library, run in Node: the Intel verifier's wasm from disk, and a dropped connection tried again. */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

export const HERE = new URL('.', import.meta.url).pathname;
const VERIFIER = resolve(HERE, '../public/attestation-verifier.wasm');

const real = globalThis.fetch;
globalThis.fetch = (async (url: RequestInfo | URL, init?: RequestInit) => {
  const target = String(url);
  if (target.endsWith('attestation-verifier.wasm') && !target.startsWith('http')) {
    return new Response(readFileSync(VERIFIER), { headers: { 'Content-Type': 'application/wasm' } });
  }
  let last: unknown;
  for (let i = 0; i < 4; i++) {
    try {
      return await real(url, init);
    } catch (e) {
      last = e;
      await new Promise((r) => setTimeout(r, 2000));
    }
  }
  throw last;
}) as typeof fetch;
