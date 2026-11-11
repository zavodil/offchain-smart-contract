/**
 * One verdict, checked end to end from public data:
 *
 *   1. the judge build's key signed exactly these bytes (ed25519, here);
 *   2. the gate's own contract accepts it (`verify`, on chain);
 *   3. the rulebook it names is the one the registry holds, by hash;
 *   4. Intel signed a TDX quote for the run that produced it, the quote
 *      commits to that run, an approved OutLayer worker ran it, the code was
 *      the build the gate pins, and the run's attested output IS this signed
 *      verdict;
 *   5. when the text is known, it is the text the verdict names, and the run's
 *      attested input is the gate's request for it.
 */

import { ed25519 } from '@noble/curves/ed25519';
import { sha256Hex, utf8 } from './bytes';
import { view } from './chain';
import { VERDICT_PREFIX, type Network } from './network';
import { attestationByRequest } from './outlayer';
import { verifyOutlayerRun } from './verify/outlayer';
import { check, fail, message, overall, pass, short, warn, type ProofGroup, type Step } from './verify/types';

export type Decision = 'pass' | 'escalate' | 'block';

export interface Verdict {
  format: string;
  gate: string;
  for: string;
  request: number;
  pack: string;
  pack_sha256: string;
  registry: string;
  block_height: number;
  text_sha256: string;
  decision: Decision;
  fired: { rule: string; bp: number }[];
  outlayer_request: string;
  at: number;
}

export interface Signed {
  message: string;
  signature: string;
}

export interface GateBuild {
  outlayer: string;
  wasm_url: string;
  wasm_sha256: string;
  registry: string;
  secrets_owner: string;
  secrets_profile: string;
  verdict_key: string | null;
  enclave_key: string | null;
}

export interface PackMeta {
  id: string;
  version: number;
  sha256: string;
  publisher: string;
  published_at: number;
  deprecated: boolean;
  size: number;
}

const hex = (s: string) => Uint8Array.from(s.match(/../g) ?? [], (b) => parseInt(b, 16));

/** The judge's request as the gate wrote it: serde_json's map, so members in sorted order, compact. */
export function gateInput(verdict: Verdict, text: { plain: string } | { sealed: string }) {
  return { for: verdict.for, op: 'screen', pack: verdict.pack, request: verdict.request, text };
}

export async function checkVerdict(
  network: Network,
  build: GateBuild,
  signed: Signed,
  known: { text?: string; sent?: { plain: string } | { sealed: string } } = {},
): Promise<{ verdict: Verdict | null; proof: ProofGroup }> {
  const steps: Step[] = [];
  let verdict: Verdict | null = null;
  try {
    verdict = JSON.parse(signed.message) as Verdict;
  } catch (e) {
    steps.push(fail('The verdict reads', message(e)));
  }

  const key = build.verdict_key;
  const bytes = utf8(VERDICT_PREFIX + signed.message);
  steps.push(
    key && ed25519.verify(hex(signed.signature), bytes, hex(key))
      ? pass('Signed by the pinned judge build', `ed25519 over the verdict's exact bytes, key ${short(key, 8)}, which only build ${short(build.wasm_sha256, 8)} derives`)
      : fail('Signed by the pinned judge build', 'the signature does not hold under the gate\'s verdict key'),
  );

  try {
    const accepted = await view<Verdict | null>(network, verdict?.gate ?? '', 'verify', signed);
    steps.push(check(accepted !== null, 'The gate accepts it on chain', `${verdict?.gate}.verify(message, signature) returns the verdict`, 'the gate\'s verify refuses it'));
  } catch (e) {
    steps.push(fail('The gate accepts it on chain', message(e)));
  }

  if (verdict) {
    try {
      const [id, version] = verdict.pack.split('@');
      const meta = await view<PackMeta | null>(network, verdict.registry, 'get_meta', { id, version: Number(version) });
      steps.push(
        !meta
          ? fail('The rulebook is the one on chain', `${verdict.registry} holds no ${verdict.pack}`)
          : meta.sha256 !== verdict.pack_sha256
            ? fail('The rulebook is the one on chain', `the registry's hash is ${short(meta.sha256)}, the verdict names ${short(verdict.pack_sha256)}`)
            : meta.deprecated
              ? warn('The rulebook is the one on chain', `${verdict.pack} by ${meta.publisher}, sha256 matches; withdrawn by its publisher since`)
              : pass('The rulebook is the one on chain', `${verdict.pack}, published by ${meta.publisher}, sha256 ${short(meta.sha256, 8)}`),
      );
    } catch (e) {
      steps.push(fail('The rulebook is the one on chain', message(e)));
    }

    if (known.text !== undefined) {
      steps.push(
        check(sha256Hex(utf8(known.text)) === verdict.text_sha256, 'The text judged is the text sent', 'sha256(text) = the verdict\'s text_sha256', 'the verdict is about another text'),
      );
    }

    try {
      const att = await attestationByRequest(network, verdict.outlayer_request);
      const run = await verifyOutlayerRun(
        network,
        att as never,
        known.sent ? gateInput(verdict, known.sent) : null,
        { message: signed.message, signature: signed.signature },
        { id: 'run', title: 'run' },
        [
          check(
            att.wasm_hash === build.wasm_sha256 && att.executed_wasm_sha256 === build.wasm_sha256,
            'The run was the pinned build',
            `wasm sha256 ${short(build.wasm_sha256, 8)}, the one the gate is fixed to`,
            `the run executed ${short(att.executed_wasm_sha256 ?? '?')}`,
          ),
        ],
      );
      for (const step of run.steps) {
        steps.push(
          step.label === 'Output is what this page received'
            ? { ...step, label: 'The run\'s attested output is this verdict', detail: step.verdict === 'pass' ? 'sha256({message, signature}) = the attested output hash' : step.detail }
            : step.label === 'Input is what this page sent'
              ? { ...step, label: 'The run\'s attested input is the gate\'s request', detail: step.verdict === 'pass' ? 'sha256 of the gate\'s request for this text = the attested input hash' : step.verdict === 'skip' ? 'the text is not known to this check' : step.detail }
              : step,
        );
      }
    } catch (e) {
      steps.push(fail('Intel signed this quote', message(e)));
    }
  }

  return {
    verdict,
    proof: {
      id: verdict ? `${verdict.gate}#${verdict.request}` : 'verdict',
      title: verdict ? `${verdict.decision.toUpperCase()} · ${verdict.pack}` : 'verdict',
      subtitle: verdict ? `request ${verdict.request} for ${verdict.for} · OutLayer request ${verdict.outlayer_request}` : '',
      verdict: overall(steps),
      steps,
      facts: verdict
        ? [
            { label: 'Fired', value: verdict.fired.map((f) => `${f.rule} ${(f.bp / 100).toFixed(2)}%`).join(', ') || 'none' },
            { label: 'Text sha256', value: short(verdict.text_sha256, 8), mono: true },
            { label: 'Rulebook sha256', value: short(verdict.pack_sha256, 8), mono: true },
          ]
        : [],
      raw: { signed, verdict },
    },
  };
}
