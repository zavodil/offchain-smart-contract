import { readFileSync } from 'node:fs';
import { ed25519 } from '@noble/curves/ed25519';
import { describe, expect, it } from 'vitest';
import { utf8 } from '../src/lib/bytes';
import { VERDICT_PREFIX } from '../src/lib/network';
import { gateInput, type Verdict } from '../src/lib/verdict';

const run = JSON.parse(readFileSync(new URL('../public/evidence/run.json', import.meta.url), 'utf8'));
const hex = (s: string) => Uint8Array.from(s.match(/../g) ?? [], (b) => parseInt(b, 16));
const key = hex(run.deployment.build.verdict_key);
const signedSteps = run.steps.filter((s: any) => s.signed);

describe('the verdicts in the evidence', () => {
  it('are signed by the gate\'s pinned verdict key over the prefix and the exact message', () => {
    expect(signedSteps.length).toBeGreaterThan(5);
    for (const step of signedSteps) {
      const ok = ed25519.verify(hex(step.signed.signature), utf8(VERDICT_PREFIX + step.signed.message), key);
      expect(ok, step.id).toBe(true);
    }
  });

  it('stop verifying when one character of the decision changes', () => {
    const blocked = signedSteps.find((s: any) => JSON.parse(s.signed.message).decision === 'block');
    const forged = blocked.signed.message.replace('"decision":"block"', '"decision":"pass"');
    expect(ed25519.verify(hex(blocked.signed.signature), utf8(VERDICT_PREFIX + forged), key)).toBe(false);
  });

  it('name the gate, the caller and a rulebook version, and only fired rules at or over threshold', () => {
    for (const step of signedSteps) {
      const v: Verdict = JSON.parse(step.signed.message);
      expect(v.gate).toBe(run.deployment.gate);
      expect([run.deployment.fund, run.deployment.launchpad, step.signer]).toContain(v.for);
      expect(v.pack).toMatch(/^[a-z0-9-]+\/[a-z0-9-]+@\d+$/);
      expect(v.decision === 'pass').toBe(v.fired.length === 0);
    }
  });
});

describe('the gate\'s request to the judge', () => {
  it('is serde_json\'s map: members sorted, compact', () => {
    const v = { for: 'fund.testnet', pack: 'grants/digital-art@2', request: 3 } as Verdict;
    expect(JSON.stringify(gateInput(v, { plain: 'x' }))).toBe('{"for":"fund.testnet","op":"screen","pack":"grants/digital-art@2","request":3,"text":{"plain":"x"}}');
  });
});
