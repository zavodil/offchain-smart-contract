/** The recorded run, the live gate, and what the pages show of them. */

import { useEffect, useState } from 'react';
import { view } from './lib/chain';
import { DEMO, TESTNET } from './lib/network';
import type { GateBuild, Signed } from './lib/verdict';

export const REPO = 'https://github.com/zavodil/offchain-smart-contract';
export const EVIDENCE = 'evidence';

export interface RunStep {
  id: string;
  title: string;
  story: string;
  expect: string;
  signer: string;
  contract: string;
  method: string;
  args: any;
  deposit: string;
  tx: string;
  result: any;
  ok: boolean;
  notes: string[];
  text?: string;
  sent?: { plain: string } | { sealed: string };
  signed?: Signed;
}

export interface Run {
  when: string;
  deployment: typeof DEMO & { build: GateBuild };
  steps: RunStep[];
}

export interface Rule {
  id: string;
  cite?: string;
  question: string;
  threshold: number;
  action: 'block' | 'escalate';
}

export interface Pack {
  id: string;
  version: number;
  title?: string;
  model: string;
  rules: Rule[];
}

export interface Calibration {
  pack: string;
  model: string;
  repeat: number;
  rules: { id: string; threshold: number; action: 'block' | 'escalate' }[];
  cases: { name: string; text: string; expect: string; decision: string; ok: boolean; bp: Record<string, number>; identical: boolean }[];
}

function useJson<T>(path: string): T | null {
  const [value, setValue] = useState<T | null>(null);
  useEffect(() => {
    fetch(path)
      .then((r) => r.json())
      .then(setValue)
      .catch(() => setValue(null));
  }, [path]);
  return value;
}

export const useRun = () => useJson<Run>(`${EVIDENCE}/run.json`);
export const usePack = (file: string) => useJson<Pack>(`${EVIDENCE}/packs/${file}.json`);
export const useCalibration = (file: string) => useJson<Calibration>(`${EVIDENCE}/calibration/${file}.json`);

export function useBuild(): GateBuild | null {
  const [build, setBuild] = useState<GateBuild | null>(null);
  useEffect(() => {
    view<GateBuild>(TESTNET, DEMO.gate, 'build').then(setBuild).catch(() => setBuild(null));
  }, []);
  return build;
}

export const txLink = (tx: string) => (tx ? `${TESTNET.explorer}/txns/${tx}` : '');
export const accountLink = (account: string) => `${TESTNET.explorer}/address/${account}`;
export const pct = (bp: number) => `${(bp / 100).toFixed(2)}%`;

/** Screens per step: [file in shots/, caption]. */
export const SHOTS: Record<string, [string, string][]> = {
  G1: [
    ['G1-paid.jpg', 'The whole transaction on NearBlocks: apply → check → request_execution → on_execution_response → on_verdict, then on_judged → ft_transfer → on_paid. The selected receipt: "Transfer 100000000 from digital-art-fund.testnet to lisbon-artist.testnet" — $100 USDC, moved by the callback that read the verdict.'],
    ['G1-recheck.jpg', 'Re-check in this report: every proof of this verdict checked again in the browser.'],
  ],
  G2: [['G2-refused.jpg', 'The same chain for the food festival: on_judged returns "refused" with the rule off_theme, and no ft_transfer follows.']],
  G8: [['G8-sealed.jpg', 'What the transaction carries for a private application: "sealed", ciphertext only. The judge read the plaintext inside the enclave.']],
  M2: [['M2-refused.jpg', 'create_market for "Will a sitting head of state be assassinated before the end of 2027?": on_judged returns "refused", rule harm_to_a_person, and the 1 NEAR bond goes back in the transfer below it.']],
  E3: [['E3-denied.jpg', 'The judge run directly, naming the gate\'s NEAR AI row: OutLayer refuses the row ("Access denied by access condition"), the run never starts, and the result is null.']],
};
