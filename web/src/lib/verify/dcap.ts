/**
 * Intel TDX quote verification in the browser: dcap-qvl compiled to
 * WebAssembly (vendored from the OutLayer dashboard's `wasm-verifier`), the
 * same crate the enclave and NEAR's contracts verify with.
 */

import init, { parse_quote, verify_quote } from '../../vendor/dcap-qvl/verifier.js';

export interface Measurements {
  mrtd: string;
  rtmr0: string;
  rtmr1: string;
  rtmr2: string;
  rtmr3: string;
}

export interface VerifiedQuote {
  ok: boolean;
  status?: string;
  advisory_ids?: string[];
  measurements?: Measurements;
  /** report_data[0..32], hex. */
  report_data_prefix?: string;
  /** report_data[32..64], hex — read from the same quote bytes once their signature has verified. */
  report_data_suffix?: string;
  error?: string;
}

let ready: Promise<unknown> | null = null;

function load() {
  ready ??= init({ module_or_path: `${import.meta.env.BASE_URL}attestation-verifier.wasm` });
  return ready;
}

/** The quote's platform id, unverified — only to pick the collateral. */
export async function quoteFmspc(quote: Uint8Array): Promise<string> {
  await load();
  const parsed = parse_quote(quote) as { ok: boolean; fmspc?: string; error?: string };
  if (!parsed.ok || !parsed.fmspc) throw new Error(parsed.error ?? 'the quote does not decode');
  return parsed.fmspc;
}

/** Verify against Intel-signed collateral as of `at` (unix seconds). */
export async function verifyQuote(quote: Uint8Array, collateralJson: string, at: number): Promise<VerifiedQuote> {
  await load();
  const verified = verify_quote(quote, collateralJson, at) as VerifiedQuote;
  if (!verified.ok) return verified;
  const parsed = parse_quote(quote) as { report_data_suffix?: string };
  return { ...verified, report_data_suffix: parsed.report_data_suffix };
}
