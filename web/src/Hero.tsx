import { useEffect, useRef, useState, type RefObject } from 'react';
import { useRun } from './data';
import { TESTNET } from './lib/network';
import type { Verdict } from './lib/verdict';

const GRANT_RULES = ['off_theme', 'personal_expenses', 'prohibited_content', 'third_party_payment', 'instructs_the_reviewer'];
const MARKET_RULES = ['harm_to_a_person', 'private_person', 'creator_controls_outcome', 'instructs_the_reviewer', 'election'];

interface Example {
  id: string;
  title: string;
  contract: string;
  method: string;
  rules: string[];
  outcome: string;
  /** Why the contract ended where it did, in its own rules' terms. */
  why: string;
}

interface Group {
  contract: string;
  label: string;
  policy: string;
  examples: Example[];
}

/** Two contracts, two real transactions each, from the recorded run. */
const GROUPS: Group[] = [
  {
    contract: 'digital-art-fund',
    label: 'A digital-art grant fund',
    policy: 'Its rulebook on chain: pays for digital art, up to $250 on its own. Refuses off-theme events, personal costs and excluded content. Payments to third parties go to its council.',
    examples: [
      { id: 'G1', title: 'Art show, asks $100', contract: 'digital-art-fund', method: 'apply', rules: GRANT_RULES, outcome: '$100 USDC paid', why: 'No rule fired: it is digital art, and $100 is under the fund\'s $250 limit. The callback paid it.' },
      { id: 'G2', title: 'Food festival, asks $200', contract: 'digital-art-fund', method: 'apply', rules: GRANT_RULES, outcome: 'Refused, $0 moved', why: 'off_theme fired at 99.86%: this fund pays for digital art, not food festivals. Nothing moved.' },
    ],
  },
  {
    contract: 'ai-prediction-markets',
    label: 'A prediction-market factory',
    policy: 'Its rulebook on chain: never lists a market on a person\'s death or harm, a private person, or an outcome its creator controls. Political questions, such as elections and referendums, go through moderation.',
    examples: [
      { id: 'M2', title: 'Market: "Will a head of state be assassinated?"', contract: 'ai-prediction-markets', method: 'create_market', rules: MARKET_RULES, outcome: 'Never created, bond back', why: 'harm_to_a_person fired at 96.94%: the factory never lists bets on a person\'s death. No market exists; the bond went back.' },
      { id: 'M5', title: 'Market: "Who wins the 2028 US election?"', contract: 'ai-prediction-markets', method: 'create_market', rules: MARKET_RULES, outcome: 'Held for the moderator', why: 'election fired at 99.79%: political questions go through moderation, so a human moderator decided, and opened it later.' },
    ],
  },
];
const EXAMPLES = GROUPS.flatMap((g) => g.examples);

/** Why the decision can be trusted, in four facts anyone can check. */
const TRUST: [string, string][] = [
  ['Rules fixed on NEAR', 'Published once to a registry contract; every verdict names their hash.'],
  ['Code pinned by hash', 'The signing key exists for one build of the judge and no other.'],
  ['Attested by Intel', 'Every run has a public Intel TDX quote over its code, input and output.'],
  ['Checked before money moves', 'The gate verifies the verdict\'s signature on chain, inside the transaction.'],
];

const LEGO: [string, string][] = [
  ['DAO treasury', 'screens every spend against the charter'],
  ['Prediction markets', 'decides which markets may exist'],
  ['Grant and asset funds', 'pays only what the mandate allows'],
  ['Lending', 'checks the stated purpose of a loan'],
  ['Launchpads and marketplaces', 'lists only what the rules allow'],
  ['Agent wallets', 'lets an AI agent spend only on allowed purposes'],
];

const WORD = { pass: 'PASS', block: 'BLOCK', escalate: 'ESCALATE' } as const;
const CHIP = { pass: 'Paid', block: 'Refused', escalate: 'To people' } as const;
const CHIP_MARKET = { pass: 'Created', block: 'Never created', escalate: 'To the moderator' } as const;
const short = (s: string) => `${s.slice(0, 6)}…${s.slice(-4)}`;

// The transaction's path: along the chain, down into the enclave, back up.
const Y = 120;
const PATH = `M70,${Y} L392,${Y} C430,${Y} 432,236 486,236 L714,236 C768,236 770,${Y} 808,${Y} L1120,${Y}`;
const NODES = { signer: 70, contract: 232, gate: 392, enclaveIn: 486, enclaveOut: 714, verify: 808, callback: 968, outcome: 1120 };
const STEP_MS = 700;

/** The arc length at which the path reaches `x` (the path only moves right). */
function lengthAt(el: SVGPathElement, x: number): number {
  let lo = 0;
  let hi = el.getTotalLength();
  for (let i = 0; i < 30; i++) {
    const mid = (lo + hi) / 2;
    if (el.getPointAtLength(mid).x < x) lo = mid;
    else hi = mid;
  }
  return hi;
}

/**
 * The signal: it walks the path stop by stop, 0.7 s a stop and 0.7 s a rule
 * inside the enclave, lights what it has reached, stops at the end, and
 * starts again only when `restartKey` changes. It waits until `active`:
 * the diagram is on screen. The walk is the explanation,
 * so it runs under reduced motion too: one dot moving, nothing flashing.
 */
function useSignal(path: RefObject<SVGPathElement>, rules: number, restartKey: string, active: boolean) {
  const [state, setState] = useState({ x: NODES.signer, y: Y, done: false });
  useEffect(() => {
    const el = path.current;
    if (!el || !active) return;
    const total = el.getTotalLength();
    const stops = [NODES.signer, NODES.contract, NODES.gate, NODES.enclaveIn, NODES.enclaveOut, NODES.verify, NODES.callback, NODES.outcome].map((x) => lengthAt(el, x));
    stops[stops.length - 1] = total;
    const durations = stops.slice(1).map((_, i) => (i === 3 ? STEP_MS * (rules + 1) : STEP_MS));
    const end = durations.reduce((a, b) => a + b, 0);
    let frame = 0;
    const start = performance.now();
    const tick = (now: number) => {
      let t = now - start;
      if (t >= end) {
        const at = el.getPointAtLength(total);
        setState({ x: at.x, y: at.y, done: true });
        return;
      }
      let i = 0;
      while (t > durations[i]) t -= durations[i++];
      const at = el.getPointAtLength(stops[i] + ((stops[i + 1] - stops[i]) * t) / durations[i]);
      setState({ x: at.x, y: at.y, done: false });
      frame = requestAnimationFrame(tick);
    };
    setState({ x: NODES.signer, y: Y, done: false });
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [path, rules, restartKey, active]);
  return state;
}

export function Hero() {
  const run = useRun();
  const [pick, setPick] = useState(0);
  const [replays, setReplays] = useState(0);
  const example = EXAMPLES[pick];
  const step = run?.steps.find((s) => s.id === example.id);
  const verdict: Verdict | null = step?.signed ? JSON.parse(step.signed.message) : null;
  const decision = verdict?.decision ?? 'pass';
  const fired = new Map((verdict?.fired ?? []).map((f) => [f.rule, f.bp]));

  const pathRef = useRef<SVGPathElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const [seen, setSeen] = useState(false);
  useEffect(() => {
    const el = boxRef.current;
    if (!el || seen) return;
    const observer = new IntersectionObserver(([entry]) => entry.isIntersecting && setSeen(true), { threshold: 0.5 });
    observer.observe(el);
    return () => observer.disconnect();
  }, [seen]);
  const signal = useSignal(pathRef, example.rules.length, `${example.id}:${verdict ? 1 : 0}:${replays}`, seen);
  const reached = (x: number) => signal.done || signal.x >= x - 1;
  const inEnclave = !signal.done && signal.x > NODES.enclaveIn && signal.x < NODES.enclaveOut;
  const span = NODES.enclaveOut - NODES.enclaveIn;
  const answered = (i: number) => reached(NODES.enclaveIn + ((i + 1) * span) / (example.rules.length + 1));
  const judged = reached(NODES.enclaveOut);
  const settled = signal.done;

  return (
    <section className="hero2">
      <p className="hero2-eyebrow">NEAR · OutLayer · NEAR AI · live on NEAR</p>
      <h1 className="hero2-title">
        Off-chain smart contracts.
        <br />
        <span>The guarantees of a smart contract, with any off-chain computation in the same transaction, AI included.</span>
      </h1>
      <p className="hero2-lede">
        Pinned code, rules fixed on chain, every run attested, identities and outcomes verified on chain before money moves. Add reproducible AI judgment to your smart contract.
      </p>

      <div className="trust">
        {TRUST.map(([title, text]) => (
          <div key={title}>
            <b>{title}</b>
            <span>{text}</span>
          </div>
        ))}
      </div>

      <div className="lego">
        <div className="lego-head">
          <h3>A smart contract like any other. It just makes the judgment call.</h3>
          <code>policy_gate.check(rulebook, text) → pass · block · escalate</code>
        </div>
        <div className="lego-tiles">
          {LEGO.map(([title, text]) => (
            <div className="lego-tile" key={title}>
              <b>{title}</b>
              <span>{text}</span>
            </div>
          ))}
        </div>
        <p className="lego-foot">One cross-contract call from any contract. Each contract publishes its own rulebook and decides in its own code what pass, block and escalate do with its money.</p>
      </div>

      <p className="hero2-pick">Two contracts from the demo, two real transactions each. Same contract, same rules, different text: pick one and watch it run.</p>
      <div className="groups" role="tablist">
        {GROUPS.map((g) => (
          <div className="group" key={g.contract}>
            <div className="group-head">
              <b>{g.label}</b>
              <span>{g.policy}</span>
            </div>
            <div className="group-examples">
              {g.examples.map((e) => {
                const i = EXAMPLES.indexOf(e);
                const s = run?.steps.find((x) => x.id === e.id);
                const d: Verdict['decision'] = s?.signed ? JSON.parse(s.signed.message).decision : 'pass';
                return (
                  <button key={e.id} role="tab" aria-selected={i === pick} className={`example ${i === pick ? 'on' : ''}`} onClick={() => setPick(i)}>
                    <span className="example-title">{e.title}</span>
                    <span className={`chip chip-${d}`}>{(e.method === 'create_market' ? CHIP_MARKET : CHIP)[d]}</span>
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>

      <div className={`tx tx-${decision}`} ref={boxRef}>
        <div className="tx-head">
          <span className="tx-label">One NEAR transaction</span>
          <span className="tx-head-right">
            {settled && (
              <button className="link replay" onClick={() => setReplays((n) => n + 1)}>
                ↻ Replay
              </button>
            )}
            {step?.tx && (
              <a className="tx-hash" href={`${TESTNET.explorer}/txns/${step.tx}/tree`} target="_blank" rel="noreferrer">
                {short(step.tx)} on NearBlocks ↗
              </a>
            )}
          </span>
        </div>

        <svg viewBox="0 0 1200 470" className="tx-svg" role="img" aria-label="The transaction leaves the chain into the enclave and comes back">
          <defs>
            <linearGradient id="chain" x1="0" x2="1">
              <stop offset="0" stopColor="var(--accent)" stopOpacity="0.15" />
              <stop offset="0.5" stopColor="var(--accent)" stopOpacity="0.7" />
              <stop offset="1" stopColor="var(--accent)" stopOpacity="0.15" />
            </linearGradient>
          </defs>

          <text x="34" y="64" className="tx-zone">ON CHAIN · NEAR</text>
          <line x1="30" y1={Y} x2="1170" y2={Y} stroke="url(#chain)" strokeWidth="2" />
          <text x="34" y="204" className="tx-zone tx-zone-off">OFF CHAIN · INTEL TDX</text>
          <line x1="30" y1="182" x2="1170" y2="182" className="tx-border" />

          <text x="600" y={Y - 8} textAnchor="middle" className="tx-pause">the transaction waits here (yield)</text>
          <text x="600" y={Y + 14} textAnchor="middle" className="tx-pause">and resumes with the verdict</text>
          <path d={PATH} className="tx-path" ref={pathRef} />
          {!signal.done && <circle r="7" cx={signal.x} cy={signal.y} className="tx-dot" />}

          {(
            [
              [NODES.signer, step?.signer.replace('.testnet', '') ?? '…', 'signs'],
              [NODES.contract, example.contract, example.method],
              [NODES.gate, 'policy-gate', 'check'],
              [NODES.verify, 'policy-gate', 'signature ✓ on chain'],
              [NODES.callback, example.contract, 'on_judged'],
            ] as [number, string, string][]
          ).map(([x, who, what]) => (
            <g key={x} className={reached(x) ? 'lit' : 'dim'}>
              <circle cx={x} cy={Y} r="9" className="tx-node" />
              <text x={x} y={Y - 22} textAnchor="middle" className="tx-who">
                {who}
              </text>
              <text x={x} y={Y + 32} textAnchor="middle" className="tx-what">
                {what}
              </text>
            </g>
          ))}

          <g className={`tx-out ${settled ? `tx-out-${decision} lit` : 'tx-out-wait'}`}>
            <circle cx={NODES.outcome} cy={Y} r="22" />
            {settled && (
              <>
                <text x={NODES.outcome} y={Y + 6} textAnchor="middle" className="tx-out-mark">
                  {decision === 'pass' ? '✓' : decision === 'block' ? '✕' : '!'}
                </text>
                <text x="1166" y={Y + 52} textAnchor="end" className="tx-out-text">
                  {example.outcome}
                </text>
              </>
            )}
          </g>

          <foreignObject x="486" y="196" width="228" height="262">
            <div className={`enclave ${inEnclave ? 'busy' : ''} ${judged ? 'judged' : ''}`}>
              <div className="enclave-head">Intel TDX enclave</div>
              <div className="enclave-sub">Sage answers each rule, yes or no</div>
              <ul className="enclave-rules">
                {example.rules.map((rule, i) => (
                  <li key={rule} className={answered(i) ? (fired.has(rule) ? 'fired' : 'answered') : 'pending'}>
                    <span>{rule}</span>
                    <b>{!answered(i) ? '·' : fired.has(rule) ? `yes ${(fired.get(rule)! / 100).toFixed(2)}%` : 'no'}</b>
                  </li>
                ))}
              </ul>
              {judged ? (
                <>
                  <div className={`enclave-verdict v-${decision}`}>{WORD[decision]}</div>
                  <div className="enclave-sign">signed by build e2b5fb1d</div>
                </>
              ) : (
                <div className="enclave-wait">{inEnclave ? 'reading…' : 'waiting for the text'}</div>
              )}
            </div>
          </foreignObject>

          <foreignObject x="40" y="214" width="420" height="200">
            <div className="tx-text">
              <div className="tx-text-label">The text it read</div>
              <p>
                “{step?.text?.replace(/\n/g, ' · ').slice(0, 170) ?? '…'}
                {(step?.text?.length ?? 0) > 170 ? '…' : ''}”
              </p>
            </div>
          </foreignObject>

          <foreignObject x="760" y="214" width="410" height="220">
            <div className="tx-facts">
              <div>
                <span>Rulebook</span> {verdict?.pack ?? '…'}, the {example.contract}'s own, fixed on chain
              </div>
              <div>
                <span>Code</span> one pinned build, nobody can swap it
              </div>
              <div>
                <span>Proof</span> Intel attestation of the run{verdict ? ` (request ${verdict.outlayer_request})` : ''}
              </div>
              <div>
                <span>Settled</span> by the contract, in the callback
              </div>
            </div>
          </foreignObject>
        </svg>
        <div className={`tx-why ${settled ? 'shown' : ''}`}>
          <b>Why:</b> {settled ? example.why : 'the verdict is on its way…'}
        </div>
      </div>
      <div className="hero2-cta">
        <a href="#/grants" className="primary-link">All grant-fund transactions →</a>
        <a href="#/markets">All market-factory transactions →</a>
        <a href="#/attacks">Every attempt to cheat it →</a>
        <a href="#/build">Build one →</a>
      </div>
    </section>
  );
}
