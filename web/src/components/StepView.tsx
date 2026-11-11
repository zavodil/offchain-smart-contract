import { useEffect, useRef, useState } from 'react';
import { ProofCard } from './ProofCard';
import { pct, SHOTS, txLink, type RunStep } from '../data';
import { TESTNET } from '../lib/network';
import { checkVerdict, type GateBuild, type Verdict } from '../lib/verdict';
import type { ProofGroup } from '../lib/verify/types';

export function Shots({ shots, tx }: { shots: [string, string][]; tx?: string }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [open, setOpen] = useState<[string, string] | null>(null);
  useEffect(() => {
    if (open) dialog.current?.showModal();
  }, [open]);
  if (!shots.length) return null;
  return (
    <div className="shots">
      {shots.map(([file, caption]) => (
        <figure key={file}>
          <a
            href={`shots/${file}`}
            onClick={(e) => {
              if (e.metaKey || e.ctrlKey || e.shiftKey) return;
              e.preventDefault();
              setOpen([file, caption]);
            }}
          >
            <img src={`shots/${file}`} alt={caption} loading="lazy" />
          </a>
          <figcaption>
            {caption}
            {tx && !file.includes('recheck') && (
              <a href={`${txLink(tx)}${file.includes('sealed') || file.includes('denied') ? '/execution' : '/tree'}`} target="_blank" rel="noreferrer">
                Open on NearBlocks ↗
              </a>
            )}
          </figcaption>
        </figure>
      ))}
      <dialog ref={dialog} className="shot-view" onClick={() => dialog.current?.close()} onClose={() => setOpen(null)}>
        {open && (
          <>
            <img src={`shots/${open[0]}`} alt={open[1]} />
            <p>{open[1]}</p>
          </>
        )}
      </dialog>
    </div>
  );
}

function Outcome({ step }: { step: RunStep }) {
  const r = step.result ?? {};
  const status: string | undefined = r.status?.status;
  const verdict: Verdict | null = step.signed ? JSON.parse(step.signed.message) : null;
  return (
    <div className="evidence">
      <dl>
        <dt>Call</dt>
        <dd>
          <code>{step.signer}</code> → <code>{step.contract}.{step.method}</code>
          {step.deposit && step.deposit !== '0 NEAR' && step.deposit !== '0' ? ` · attached ${step.deposit}` : ''}
        </dd>
        {step.tx && (
          <>
            <dt>Transaction</dt>
            <dd>
              <a href={`${txLink(step.tx)}/tree`} target="_blank" rel="noreferrer" className="mono">
                {step.tx} ↗
              </a>
            </dd>
          </>
        )}
        <dt>Expected</dt>
        <dd>{step.expect}</dd>
        <dt>Result</dt>
        <dd>
          <span className={step.ok ? 'yes' : 'no'}>{step.ok ? 'as expected' : 'NOT as expected'}</span>
          {status ? ` · status ${status}` : ''}
          {r.status?.reason ? ` · ${r.status.reason}` : ''}
          {r.failed ? ` · no verdict: ${r.failed}` : ''}
          {r.failure ? ` · ${r.failure}` : ''}
        </dd>
        {verdict && (
          <>
            <dt>Verdict</dt>
            <dd>
              <strong>{verdict.decision}</strong> under <code>{verdict.pack}</code>
              {verdict.fired.length ? ` · fired: ${verdict.fired.map((f) => `${f.rule} ${pct(f.bp)}`).join(', ')}` : ' · no rule fired'}
            </dd>
            <dt>Run</dt>
            <dd>
              OutLayer request{' '}
              <a href={`${TESTNET.api}/attestations/by-request/${verdict.outlayer_request}`} target="_blank" rel="noreferrer">
                {verdict.outlayer_request}
              </a>{' '}
              (its Intel TDX attestation)
            </dd>
          </>
        )}
        {step.notes.length > 0 && (
          <>
            <dt>Checked</dt>
            <dd>{step.notes.join(' · ')}</dd>
          </>
        )}
      </dl>
      {step.text && (
        <details className="evidence-file">
          <summary>
            The text judged <span>{step.sent && 'sealed' in step.sent ? '(sent sealed: this is the plaintext only the enclave read)' : ''}</span>
          </summary>
          <textarea readOnly rows={4} value={step.text} />
        </details>
      )}
      {step.sent && 'sealed' in step.sent && (
        <details className="evidence-file">
          <summary>
            What the transaction carried <span>(ciphertext)</span>
          </summary>
          <textarea readOnly rows={3} value={step.sent.sealed} />
        </details>
      )}
      {step.signed && (
        <details className="evidence-file">
          <summary>
            The signed verdict <span>(message + ed25519 signature, as the gate checked it)</span>
          </summary>
          <textarea readOnly rows={8} value={JSON.stringify({ message: JSON.parse(step.signed.message), signature: step.signed.signature }, null, 2)} />
        </details>
      )}
    </div>
  );
}

function Recheck({ step, build }: { step: RunStep; build: GateBuild | null }) {
  const [state, setState] = useState<'idle' | 'running' | 'done' | 'error'>('idle');
  const [proof, setProof] = useState<ProofGroup | null>(null);
  const [error, setError] = useState('');
  if (!step.signed || !build) return null;
  const run = async () => {
    setState('running');
    try {
      const { proof } = await checkVerdict(TESTNET, build, step.signed!, { text: step.text, sent: step.sent });
      setProof(proof);
      setState('done');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setState('error');
    }
  };
  return (
    <div className="recheck">
      <button className="secondary" onClick={run} disabled={state === 'running'}>
        {state === 'running' ? 'Checking…' : state === 'done' ? 'Re-check again' : 'Re-check this verdict in your browser'}
      </button>
      {state === 'idle' && <span className="note">Signature, the gate's verify on chain, the rulebook's hash, Intel's quote, the attested input and output.</span>}
      {state === 'error' && <p className="recheck-error">{error}</p>}
      {proof && (
        <div className="recheck-detail">
          <ProofCard group={proof} />
        </div>
      )}
    </div>
  );
}

export function Step({ step, build }: { step: RunStep; build: GateBuild | null }) {
  return (
    <section className="report-step" id={`step-${step.id}`}>
      <h3>
        <span className="step-n">{step.id}</span>
        {step.title}
      </h3>
      <p>{step.story}</p>
      <Shots shots={SHOTS[step.id] ?? []} tx={step.tx} />
      <Outcome step={step} />
      <Recheck step={step} build={build} />
    </section>
  );
}
