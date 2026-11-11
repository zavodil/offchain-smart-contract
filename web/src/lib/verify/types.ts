/** What a proof card shows: checks the browser ran, and what they rest on. */

export type Verdict = 'pass' | 'warn' | 'fail' | 'skip';

export interface Step {
  label: string;
  verdict: Verdict;
  detail: string;
  /** The enclave's own verdict on the same check, when it ran one. */
  enclave?: { ok: boolean; error?: string | null };
}

export interface Fact {
  label: string;
  value: string;
  mono?: boolean;
  href?: string;
}

export interface ProofGroup {
  id: string;
  title: string;
  subtitle: string;
  verdict: Verdict;
  steps: Step[];
  facts: Fact[];
  /** The material itself, for the "raw" drawer. */
  raw?: unknown;
}

/** A group is as good as its worst step; skipped steps do not count. */
export function overall(steps: Step[]): Verdict {
  const counted = steps.filter((s) => s.verdict !== 'skip');
  if (counted.length === 0) return 'skip';
  if (counted.some((s) => s.verdict === 'fail')) return 'fail';
  if (counted.some((s) => s.verdict === 'warn')) return 'warn';
  return 'pass';
}

export const pass = (label: string, detail: string): Step => ({ label, verdict: 'pass', detail });
export const fail = (label: string, detail: string): Step => ({ label, verdict: 'fail', detail });
export const warn = (label: string, detail: string): Step => ({ label, verdict: 'warn', detail });
export const skip = (label: string, detail: string): Step => ({ label, verdict: 'skip', detail });
export const check = (ok: boolean, label: string, yes: string, no: string): Step => (ok ? pass(label, yes) : fail(label, no));

export const short = (value: string, keep = 10): string =>
  value.length > keep * 2 + 1 ? `${value.slice(0, keep)}…${value.slice(-keep)}` : value;

export const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));
