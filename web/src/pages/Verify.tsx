import { Fragment, useEffect, useState } from 'react';
import { sha256 } from '@noble/hashes/sha256';
import { bytesToHex } from '@noble/hashes/utils';
import { PageNav, SeeNext } from '../components/PageNav';
import { ProofCard } from '../components/ProofCard';
import { accountLink, REPO, useBuild, useRun, type RunStep } from '../data';
import { accessKeys, view } from '../lib/chain';
import { DEMO, TESTNET } from '../lib/network';
import { checkVerdict, type GateBuild, type Signed, type Verdict } from '../lib/verdict';
import type { ProofGroup, Step as Check } from '../lib/verify/types';

interface Stored {
  id: number;
  verdict_message: string | null;
  verdict_signature: string | null;
  status: { status: string };
}

interface Row {
  key: string;
  where: string;
  signed: Signed;
  verdict: Verdict;
  state: 'waiting' | 'checking' | 'done';
  proof?: ProofGroup;
}

const MARK = { pass: '✓', warn: '!', fail: '✗', skip: '–' } as const;

/** What the scenario's record knows of a verdict's text: the exact request when it holds this verdict, the text otherwise. */
function known(steps: RunStep[], signed: Signed): { text?: string; sent?: { plain: string } | { sealed: string } } {
  const exact = steps.find((s) => s.signed?.message === signed.message);
  if (exact?.text) return { text: exact.text, sent: exact.sent };
  const textSha = (JSON.parse(signed.message) as Verdict).text_sha256;
  const same = steps.find((s) => s.text && bytesToHex(sha256(new TextEncoder().encode(s.text))) === textSha);
  if (!same?.text) return {};
  return same.sent && 'plain' in same.sent ? { text: same.text, sent: same.sent } : { text: same.text };
}

export function VerifyPage() {
  const run = useRun();
  const liveBuild = useBuild();
  const build = liveBuild ?? run?.deployment.build ?? null;
  const [rows, setRows] = useState<Row[]>([]);
  const [gate, setGate] = useState<Check[] | null>(null);
  const [running, setRunning] = useState(false);
  const [open, setOpen] = useState<string | null>(null);

  useEffect(() => {
    window.scrollTo(0, 0);
  }, []);

  // Every verdict the two contracts hold, read live from NEAR.
  useEffect(() => {
    (async () => {
      const [applications, markets] = await Promise.all([
        view<Stored[]>(TESTNET, DEMO.fund, 'applications', { from: 0, limit: 50 }),
        view<Stored[]>(TESTNET, DEMO.markets, 'markets', { from: 0, limit: 50 }),
      ]);
      const toRow = (where: string, x: Stored): Row | null =>
        x.verdict_message && x.verdict_signature
          ? { key: `${where}#${x.id}`, where: `${where} #${x.id}`, signed: { message: x.verdict_message, signature: x.verdict_signature }, verdict: JSON.parse(x.verdict_message), state: 'waiting' }
          : null;
      setRows([...applications.map((a) => toRow('digital-art-fund application', a)), ...markets.map((m) => toRow('ai-prediction-markets market', m))].filter((r): r is Row => r !== null));
    })().catch(() => setRows([]));
  }, []);

  async function verifyAll() {
    if (!build) return;
    setRunning(true);
    const checks: Check[] = [];
    try {
      const wasm = new Uint8Array(await (await fetch(build.wasm_url)).arrayBuffer());
      const served = bytesToHex(sha256(wasm));
      checks.push(
        served === build.wasm_sha256
          ? { label: 'The judge build is the wasm FastFS serves', verdict: 'pass', detail: `sha256 ${served}, ${wasm.length} bytes, hashed in your browser` }
          : { label: 'The judge build is the wasm FastFS serves', verdict: 'fail', detail: `FastFS serves ${served}` },
      );
    } catch (e) {
      checks.push({ label: 'The judge build is the wasm FastFS serves', verdict: 'warn', detail: `could not fetch it: ${e instanceof Error ? e.message : String(e)}` });
    }
    checks.push(
      build.verdict_key
        ? { label: 'The gate holds the build\'s verdict key', verdict: 'pass', detail: `${build.verdict_key}, taken from the build itself by load_keys` }
        : { label: 'The gate holds the build\'s verdict key', verdict: 'fail', detail: 'load_keys has not run' },
    );
    try {
      const full = (await accessKeys(TESTNET, DEMO.gate)).filter((k) => k.access_key.permission === 'FullAccess');
      checks.push(
        full.length === 0
          ? { label: 'Nobody can change the gate', verdict: 'pass', detail: 'the gate account has no full-access key: its code and its pins are final' }
          : { label: 'Nobody can change the gate', verdict: 'warn', detail: `${full.length} full-access key still on ${DEMO.gate}` },
      );
    } catch (e) {
      checks.push({ label: 'Nobody can change the gate', verdict: 'warn', detail: e instanceof Error ? e.message : String(e) });
    }
    setGate(checks);

    for (const row of rows) {
      setRows((rs) => rs.map((r) => (r.key === row.key ? { ...r, state: 'checking' } : r)));
      const { proof } = await checkVerdict(TESTNET, build as GateBuild, row.signed, known(steps, row.signed));
      setRows((rs) => rs.map((r) => (r.key === row.key ? { ...r, state: 'done', proof } : r)));
    }
    setRunning(false);
  }

  const steps = run?.steps ?? [];
  const sha = (t: string) => bytesToHex(sha256(new TextEncoder().encode(t)));
  const stepOf = (r: Row) =>
    steps.find((s) => s.signed?.message === r.signed.message) ?? steps.find((s) => s.text && sha(s.text) === r.verdict.text_sha256);
  const earlier = (r: Row) => !steps.some((s) => s.signed?.message === r.signed.message);
  const pageOf = (id: string) => (id.startsWith('G') ? 'grants' : id.startsWith('M') ? 'markets' : 'attacks');

  const done = rows.filter((r) => r.state === 'done');
  const counts = done.reduce((c, r) => ({ ...c, [r.proof!.verdict]: (c[r.proof!.verdict] ?? 0) + 1 }), {} as Record<string, number>);

  return (
    <div className="page report">
      <PageNav current="check" />

      <header className="showcase-head">
        <span className="showcase-tag">Don't trust this page</span>
        <h1>Verify every verdict, in your browser</h1>
        <p className="lede">
          This page reads every verdict the two contracts hold straight from NEAR, and checks each one from public data: the signature against the build's key, the gate's own check on chain, the rulebook's hash in the registry, and Intel's attestation of the run that produced it. Nothing comes from us but this page's code, and that is <a href={REPO}>open</a>.
        </p>
      </header>

      <div className="verify-run">
        <button className="primary big" onClick={verifyAll} disabled={running || !build || rows.length === 0}>
          {running ? `Checking… ${done.length} of ${rows.length}` : done.length ? 'Verify again' : `Verify all ${rows.length || ''} verdicts`}
        </button>
        {done.length > 0 && !running && (
          <span className="verify-summary">
            {done.length} checked: {Object.entries(counts).map(([k, v]) => `${v} ${k === 'pass' ? 'verified' : k === 'warn' ? 'verified with notes' : 'failed'}`).join(', ')}
          </span>
        )}
      </div>

      {gate && (
        <section className="report-section">
          <h2>The gate</h2>
          <ul className="verify-checks">
            {gate.map((c) => (
              <li key={c.label} className={`ok-${c.verdict}`}>
                <b>{MARK[c.verdict]}</b> {c.label} <span>{c.detail}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="report-section">
        <h2>The verdicts</h2>
        <div className="wrap wide">
          <table className="verify-table">
            <thead>
              <tr>
                <th>Transaction</th>
                <th>Stored at</th>
                <th>Rulebook</th>
                <th>Decision</th>
                <th>Rules that fired</th>
                <th>Checks</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <Fragment key={r.key}>
                  <tr className={`verify-row ${r.state}`}>
                    <td>
                      {(() => {
                        const step = stepOf(r);
                        return step ? (
                          <a href={`#/${pageOf(step.id)}/${step.id}`}>
                            <b>{step.id}</b> {step.title}
                            {earlier(r) && <span className="note"> · an earlier run of the same text</span>}
                          </a>
                        ) : (
                          <span className="note">an earlier test run, before the scenario’s final texts</span>
                        );
                      })()}
                    </td>
                    <td className="note">{r.where}</td>
                    <td className="mono">{r.verdict.pack}</td>
                    <td>
                      <span className={`chip chip-${r.verdict.decision}`}>{r.verdict.decision}</span>
                    </td>
                    <td className="mono">{r.verdict.fired.map((f) => `${f.rule} ${(f.bp / 100).toFixed(2)}%`).join(', ') || '—'}</td>
                    <td>
                      {r.state === 'waiting' && <span className="note">not checked yet</span>}
                      {r.state === 'checking' && <span className="note">checking…</span>}
                      {r.proof && (
                        <span className={"verify-steps"}>
                          {r.proof.steps.map((s, i) => (
                            <i key={i} className={`ok-${s.verdict}`} title={`${s.label}: ${s.detail}`}>
                              {MARK[s.verdict]}
                            </i>
                          ))}
                        </span>
                      )}
                    </td>
                    <td>
                      {r.proof && (
                        <button className="link" onClick={() => setOpen(open === r.key ? null : r.key)}>
                          {open === r.key ? 'hide' : 'details'}
                        </button>
                      )}
                    </td>
                  </tr>
                  {open === r.key && r.proof && (
                    <tr>
                      <td colSpan={7}>
                        <ProofCard group={r.proof} />
                      </td>
                    </tr>
                  )}
                </Fragment>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section className="report-section">
        <h2>What each check means</h2>
        <ol className="steps-list">
          <li><b>Signed by the pinned judge build.</b> ed25519 over the verdict's exact bytes, under the key that only that build of the judge derives (from NEAR MPC confidential key derivation).</li>
          <li><b>The gate accepts it on chain.</b> The gate contract's own <code>verify</code> answers for these bytes.</li>
          <li><b>The rulebook is the one on chain.</b> The registry holds the same sha256 for the rulebook the verdict names.</li>
          <li><b>The text judged is the text sent.</b> When this page knows the text, its hash matches the verdict's.</li>
          <li><b>Intel signed this quote.</b> The TDX quote of the run, checked with Intel's signed collateral, in this browser (Intel DCAP compiled to WebAssembly).</li>
          <li><b>The quote commits to this run.</b> Its report data is the hash of the run's code, input, output, caller and time.</li>
          <li><b>An approved OutLayer worker ran it.</b> The quote's measurements are listed on <code>{TESTNET.registry}</code>.</li>
          <li><b>The attested input and output.</b> The run's input is the gate's request; its output is exactly this signed verdict.</li>
          <li><b>The run was the pinned build.</b> The code that ran has the sha256 the gate is fixed to.</li>
        </ol>
      </section>

      <section className="report-section">
        <h2>Or from a terminal</h2>
        <pre className="command">{`git clone ${REPO}
cd offchain-smart-contract/web && npm ci
npm run verify`}</pre>
        <p>The same checks, with no account and no key, ending with <code>N stored verdicts: N pass</code>.</p>
        {build && (
          <div className="evidence">
            <dl>
              <dt>Policy gate</dt>
              <dd>
                <a href={accountLink(DEMO.gate)} target="_blank" rel="noreferrer">
                  {DEMO.gate}
                </a>
              </dd>
              <dt>Judge build</dt>
              <dd className="mono">
                <a href={build.wasm_url}>{build.wasm_sha256}</a>
              </dd>
              <dt>Verdict key</dt>
              <dd className="mono">{build.verdict_key}</dd>
              <dt>Rulebooks</dt>
              <dd>
                <code>{build.registry}</code>
              </dd>
            </dl>
          </div>
        )}
      </section>

      <SeeNext current="check" />
      <footer className="footer">
        Built on <a href="https://outlayer.ai">OutLayer</a>, <a href="https://cloud.near.ai">NEAR AI Cloud</a> and NEAR.
      </footer>
    </div>
  );
}
