/**
 * The OutLayer side: was this run executed by an approved OutLayer worker in
 * Intel TDX, on exactly the input this page sent, producing exactly the output
 * this page received?
 *
 * The worker hashes the run's parameters into the quote's report_data before
 * Intel's hardware signs it; recomputing that hash from the published record
 * ties the quote to this call. The field order and encoding are the
 * attestation format (`worker/src/tdx_attestation.rs`, V1).
 */

import { utf8, fromBase64, sha256Hex } from '../bytes';
import { ATTESTATION_V1_FROM, type Network } from '../network';
import { fetchCollateral, measurementsApproved, type AttestationRecord } from '../outlayer';
import { quoteFmspc, verifyQuote } from './dcap';
import { check, fail, message, overall, pass, short, skip, warn, type ProofGroup, type Step } from './types';

export function taskHash(att: AttestationRecord): string {
  const parts: Uint8Array[] = [];
  const int64 = (value: number, signed: boolean) => {
    const view = new DataView(new ArrayBuffer(8));
    if (signed) view.setBigInt64(0, BigInt(value), true);
    else view.setBigUint64(0, BigInt(value), true);
    return new Uint8Array(view.buffer);
  };
  const text = (value?: string | null) => value && parts.push(utf8(value));

  text(att.task_type);
  parts.push(int64(att.task_id, true));
  text(att.repo_url);
  text(att.commit_hash);
  text(att.build_target);
  text(att.wasm_hash);
  text(att.input_hash);
  parts.push(utf8(att.output_hash));
  if (att.block_height) parts.push(int64(att.block_height, false));
  if (att.timestamp >= ATTESTATION_V1_FROM) {
    text(att.caller_account_id);
    text(att.project_id);
    text(att.secrets_ref);
    parts.push(int64(att.timestamp, true));
    text(att.attached_usd);
  }
  const joined = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let offset = 0;
  for (const p of parts) {
    joined.set(p, offset);
    offset += p.length;
  }
  return sha256Hex(joined);
}

/**
 * Verify one OutLayer run. `input` and `output` are what this page sent and
 * received; the worker hashed `input` as the API relayed it and `output` as
 * compact JSON, both in the member order the module and this page use. A
 * run the platform started on an officer's approval has an input this page
 * never saw (`null`): the attestation's input hash is then taken as it is.
 */
export async function verifyOutlayerRun(
  network: Network,
  att: AttestationRecord,
  input: unknown | null,
  output: unknown,
  card: { id: string; title: string },
  extra: Step[] = [],
): Promise<ProofGroup> {
  const steps: Step[] = [];
  let measurements: object | undefined;
  let rtmr3 = '';

  try {
    const quote = fromBase64(att.tdx_quote);
    const collateral = await fetchCollateral(network, await quoteFmspc(quote), att.timestamp);
    const verified = await verifyQuote(quote, collateral, att.timestamp);
    if (!verified.ok) {
      steps.push(fail('Intel signed this quote', verified.error ?? 'the quote did not verify'));
    } else {
      measurements = verified.measurements;
      rtmr3 = verified.measurements?.rtmr3 ?? '';
      steps.push(
        verified.status === 'UpToDate'
          ? pass('Intel signed this quote', 'Genuine Intel TDX hardware, TCB up to date')
          : warn('Intel signed this quote', `Genuine Intel TDX, TCB status ${verified.status}`),
      );
      steps.push(
        check(
          verified.report_data_prefix === taskHash(att),
          'The quote commits to this run',
          'report_data = hash(code, input, output, caller, project, time)',
          'report_data does not match this run’s record',
        ),
      );
    }
  } catch (e) {
    steps.push(fail('Intel signed this quote', message(e)));
  }

  if (measurements) {
    try {
      const approved = await measurementsApproved(network, measurements);
      steps.push(
        check(
          approved,
          'An approved OutLayer worker ran it',
          `Measurements listed on ${network.registry}`,
          `Measurements are not on ${network.registry}`,
        ),
      );
    } catch (e) {
      steps.push(warn('An approved OutLayer worker ran it', `Could not ask ${network.registry}: ${message(e)}`));
    }
  }

  steps.push(
    input === null
      ? skip('Input is what this page sent', 'the platform started this run on the officer’s approval; its input is the task’s approval, not this page’s')
      : check(
          sha256Hex(JSON.stringify(input)) === att.input_hash,
          'Input is what this page sent',
          'sha256(input) matches the attested input hash',
          'the attested input hash is of other input',
        ),
    check(
      sha256Hex(JSON.stringify(output)) === att.output_hash,
      'Output is what this page received',
      'sha256(output) matches the attested output hash',
      'the attested output hash is of other output',
    ),
    ...extra,
  );

  return {
    ...card,
    subtitle: 'OutLayer worker · Intel TDX',
    verdict: overall(steps),
    steps,
    facts: [
      { label: 'Project', value: att.project_id ?? '—' },
      { label: 'Code (wasm sha256)', value: short(att.wasm_hash ?? '—'), mono: true },
      { label: 'Run for', value: att.payment_key_owner ?? att.caller_account_id ?? '—' },
      { label: 'Call', value: att.call_id ?? '—', mono: true, href: `${network.api}/attestations/by-call/${att.call_id}` },
      { label: 'Worker RTMR3', value: short(rtmr3 || att.worker_measurement), mono: true },
      { label: 'Attested at', value: new Date(att.timestamp * 1000).toLocaleString() },
    ],
    raw: att,
  };
}
