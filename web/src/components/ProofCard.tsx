import { useState } from 'react';
import type { ProofGroup } from '../lib/verify/types';
import { Badge, VerdictIcon } from './Icons';

const VERDICT_WORD = { pass: 'Verified', warn: 'Verified with notes', fail: 'Failed', skip: 'Not checked' } as const;

export function ProofCard({ group }: { group: ProofGroup }) {
  const [raw, setRaw] = useState(false);
  return (
    <article className={`proof proof-${group.verdict}`}>
      <header className="proof-head">
        <Badge verdict={group.verdict} size={14} />
        <div className="proof-titles">
          <h3>{group.title}</h3>
          <p>{group.subtitle}</p>
        </div>
        <span className={`pill pill-${group.verdict}`}>{VERDICT_WORD[group.verdict]}</span>
      </header>

      <ol className="steps">
        {group.steps.map((step, i) => (
          <li key={i} className={`step step-${step.verdict}`}>
            <span className="step-icon">
              <VerdictIcon verdict={step.verdict} size={13} />
            </span>
            <div className="step-body">
              <div className="step-label">
                {step.label}
                {step.enclave && (
                  <span
                    className={`enclave-chip ${step.enclave.ok ? 'ok' : 'no'}`}
                    title={step.enclave.error ?? 'The enclave ran the same check before sealing the receipt'}
                  >
                    {step.enclave.ok ? 'also verified in enclave' : 'enclave: failed'}
                  </span>
                )}
              </div>
              <div className="step-detail">{step.detail}</div>
            </div>
          </li>
        ))}
      </ol>

      {group.facts.length > 0 && (
        <dl className="facts">
          {group.facts.map((fact) => (
            <div key={fact.label} className="fact">
              <dt>{fact.label}</dt>
              <dd className={fact.mono ? 'mono' : undefined} title={fact.value}>
                {fact.href ? (
                  <a href={fact.href} target="_blank" rel="noreferrer">
                    {fact.value}
                  </a>
                ) : (
                  fact.value
                )}
              </dd>
            </div>
          ))}
        </dl>
      )}

      {group.raw !== undefined && (
        <div className="raw">
          <button className="link" onClick={() => setRaw(!raw)}>
            {raw ? 'Hide' : 'Show'} the material this was checked from
          </button>
          {raw && <pre>{JSON.stringify(group.raw, null, 2)}</pre>}
        </div>
      )}
    </article>
  );
}
