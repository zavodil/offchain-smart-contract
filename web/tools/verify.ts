/**
 * Check the deployed demo from public data alone: no account, no key.
 *
 *   npm run verify
 *
 * It reads what the contracts on NEAR testnet hold and what OutLayer
 * publishes, and checks:
 *
 *   the gate    — the judge build it is fixed to is the wasm FastFS serves,
 *                 byte for byte; its keys came from that build; whether any
 *                 key can still change the gate's code;
 *   every verdict the grant fund and the launchpad stored —
 *                 ed25519 by the pinned build over its exact bytes; accepted by
 *                 the gate's own `verify`; the rulebook is the registry's, by
 *                 hash; Intel signed the TDX quote of the run that produced it,
 *                 the quote commits to that run, an approved OutLayer worker
 *                 ran it, the code was the pinned build, and the run's attested
 *                 output is this signed verdict. With `public/evidence/run.json`
 *                 (written by `npm run scenario`) it also checks the text and
 *                 the run's attested input.
 *
 * Exit status 0 when nothing fails. A rulebook withdrawn after a verdict was
 * given shows as a warning, not a failure.
 */

import './node';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { sha256 } from '@noble/hashes/sha256';
import { bytesToHex } from '@noble/hashes/utils';
import { HERE } from './node';
import { accessKeys, view } from '../src/lib/chain';
import { DEMO, TESTNET } from '../src/lib/network';
import { checkVerdict, type GateBuild, type Signed } from '../src/lib/verdict';
import type { ProofGroup, Step } from '../src/lib/verify/types';

const EVIDENCE = resolve(HERE, '../public/evidence/run.json');
const OUT = resolve(HERE, '../public/evidence/verify.json');

interface Stored {
  id: number;
  status: { status: string };
  verdict_message: string | null;
  verdict_signature: string | null;
}

const mark = { pass: 'ok  ', warn: 'warn', fail: 'FAIL', skip: 'skip' } as const;

async function main() {
  const build = await view<GateBuild>(TESTNET, DEMO.gate, 'build');
  const gate: Step[] = [];

  const wasm = new Uint8Array(await (await fetch(build.wasm_url)).arrayBuffer());
  const served = bytesToHex(sha256(wasm));
  gate.push(
    served === build.wasm_sha256
      ? { label: 'The judge build is the wasm FastFS serves', verdict: 'pass', detail: `sha256 ${served}, ${wasm.length} bytes` }
      : { label: 'The judge build is the wasm FastFS serves', verdict: 'fail', detail: `FastFS serves ${served}` },
  );
  gate.push(
    build.verdict_key && build.enclave_key
      ? { label: 'The gate holds the build\'s keys', verdict: 'pass', detail: `verdict key ${build.verdict_key}, taken from the build by load_keys` }
      : { label: 'The gate holds the build\'s keys', verdict: 'fail', detail: 'load_keys has not run' },
  );
  const keys = await accessKeys(TESTNET, DEMO.gate);
  const full = keys.filter((k) => k.access_key.permission === 'FullAccess');
  gate.push(
    full.length === 0
      ? { label: 'Nobody can change the gate', verdict: 'pass', detail: 'the account has no full-access key: its code and its pins are final' }
      : { label: 'Nobody can change the gate', verdict: 'warn', detail: `${full.length} full-access key(s) still on ${DEMO.gate}` },
  );
  console.log(`gate ${DEMO.gate}`);
  for (const s of gate) console.log(`  ${mark[s.verdict]} ${s.label} — ${s.detail}`);

  const evidence = existsSync(EVIDENCE) ? JSON.parse(readFileSync(EVIDENCE, 'utf8')) : { steps: [] };
  // What this check knows about a verdict's text, from the scenario's record:
  // the exact request for a verdict the record holds, and for any other
  // verdict about the same text, the text — and the request too when it went
  // in the clear. A sealed text is sealed anew each time, so another run's
  // ciphertext is not this verdict's.
  type Known = { text: string; sent?: { plain: string } | { sealed: string } };
  const byMessage = new Map<string, Known>();
  const byText = new Map<string, Known>();
  for (const step of evidence.steps) {
    if (!step.text || !step.sent) continue;
    if (step.signed) byMessage.set(step.signed.message, { text: step.text, sent: step.sent });
    const sha = bytesToHex(sha256(new TextEncoder().encode(step.text)));
    byText.set(sha, 'plain' in step.sent ? { text: step.text, sent: step.sent } : { text: step.text });
  }

  const stored: { where: string; id: number; signed: Signed }[] = [];
  const applications = await view<Stored[]>(TESTNET, DEMO.fund, 'applications', { from: 0, limit: 50 });
  const launches = await view<Stored[]>(TESTNET, DEMO.launchpad, 'launches', { from: 0, limit: 50 });
  for (const a of applications) if (a.verdict_message && a.verdict_signature) stored.push({ where: `${DEMO.fund} application ${a.id}`, id: a.id, signed: { message: a.verdict_message, signature: a.verdict_signature } });
  for (const l of launches) if (l.verdict_message && l.verdict_signature) stored.push({ where: `${DEMO.launchpad} launch ${l.id}`, id: l.id, signed: { message: l.verdict_message, signature: l.verdict_signature } });

  const proofs: (ProofGroup & { where: string })[] = [];
  let failed = gate.some((s) => s.verdict === 'fail');
  for (const item of stored) {
    const textSha = JSON.parse(item.signed.message).text_sha256 as string;
    const known = byMessage.get(item.signed.message) ?? byText.get(textSha) ?? {};
    const { proof } = await checkVerdict(TESTNET, build, item.signed, known);
    proofs.push({ ...proof, where: item.where });
    failed ||= proof.verdict === 'fail';
    console.log(`\n${mark[proof.verdict]} ${item.where}: ${proof.title} — ${proof.facts[0]?.value}`);
    for (const s of proof.steps) console.log(`  ${mark[s.verdict]} ${s.label} — ${s.detail}`);
  }

  writeFileSync(OUT, JSON.stringify({ when: new Date().toISOString(), build, gate, proofs }, null, 2));
  const counts = proofs.reduce((c, p) => ({ ...c, [p.verdict]: (c[p.verdict] ?? 0) + 1 }), {} as Record<string, number>);
  console.log(`\n${stored.length} stored verdicts: ${Object.entries(counts).map(([k, v]) => `${v} ${k}`).join(', ')} · ${OUT}`);
  process.exit(failed ? 1 : 0);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
