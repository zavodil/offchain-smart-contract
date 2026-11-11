import type { Calibration, Pack } from '../data';

const ACTION = { block: 'blocks', escalate: 'escalates' } as const;
const DECISION = { pass: 'pass', block: 'block', escalate: 'escalate' } as Record<string, string>;

/** A rulebook as its contract reads it: each rule, its threshold and what firing does. */
export function RuleTable({ pack }: { pack: Pack | null }) {
  if (!pack) return null;
  return (
    <div className="wrap">
      <table className="rules-table">
        <thead>
          <tr>
            <th>Rule</th>
            <th>The yes/no question Sage answers</th>
            <th>Fires at</th>
            <th>Then it</th>
          </tr>
        </thead>
        <tbody>
          {pack.rules.map((rule) => (
            <tr key={rule.id}>
              <td className="mono">{rule.id}</td>
              <td className="why">
                {rule.question}
                {rule.cite && <div className="cite">{rule.cite}</div>}
              </td>
              <td className="num">{rule.threshold}%</td>
              <td>
                <span className={`chip chip-${rule.action}`}>{ACTION[rule.action]}</span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** Every calibration case: the probability Sage gave each rule, the decision, and whether it was the expected one. */
export function CalibrationTable({ data }: { data: Calibration | null }) {
  if (!data) return null;
  return (
    <div className="wrap wide">
      <table className="calibration-table">
        <thead>
          <tr>
            <th>Case</th>
            {data.rules.map((rule) => (
              <th key={rule.id} className="num" title={`fires at ${rule.threshold}%, then ${rule.action}`}>
                {rule.id.replace(/_/g, ' ')}
                <span className="th-sub">
                  ≥ {rule.threshold}% · {rule.action}
                </span>
              </th>
            ))}
            <th>Decision</th>
            <th>Expected</th>
            <th className="num">Same ×{data.repeat}</th>
          </tr>
        </thead>
        <tbody>
          {data.cases.map((c) => (
            <tr key={c.name}>
              <td title={c.text}>{c.name}</td>
              {data.rules.map((rule) => {
                const bp = c.bp[rule.id];
                const fired = bp >= rule.threshold * 100;
                return (
                  <td key={rule.id} className={`num ${fired ? `fired fired-${rule.action}` : 'quiet'}`}>
                    {(bp / 100).toFixed(2)}%
                  </td>
                );
              })}
              <td>
                <span className={`chip chip-${c.decision}`}>{DECISION[c.decision]}</span>
              </td>
              <td>
                <span className={c.ok ? 'yes' : 'no'}>{c.ok ? '✓ ' : '✗ '}</span>
                {c.expect}
              </td>
              <td className="num">{c.identical ? '✓' : '✗'}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
