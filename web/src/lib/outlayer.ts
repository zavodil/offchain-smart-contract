/** OutLayer's public record of a run: its attestation, and the Intel collateral to check it with. */

import type { Network } from './network';

export interface AttestationRecord {
  task_id: number;
  task_type: string;
  tdx_quote: string;
  worker_measurement: string;
  request_id?: number;
  call_id?: string;
  caller_account_id?: string;
  payment_key_owner?: string;
  transaction_hash?: string;
  repo_url?: string;
  commit_hash?: string;
  build_target?: string;
  wasm_hash?: string;
  executed_wasm_sha256?: string;
  input_hash?: string;
  output_hash: string;
  block_height?: number;
  project_id?: string;
  secrets_ref?: string;
  attached_usd?: string;
  timestamp: number;
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** The attestation of an on-chain request, as OutLayer publishes it. */
export async function attestationByRequest(network: Network, requestId: string | number): Promise<AttestationRecord> {
  const deadline = Date.now() + 30_000;
  for (;;) {
    const response = await fetch(`${network.api}/attestations/by-request/${requestId}`);
    if (response.ok) return response.json();
    if (response.status !== 404 || Date.now() > deadline) throw new Error(`no attestation for request ${requestId} (HTTP ${response.status})`);
    await wait(1500);
  }
}

/** Intel collateral OutLayer archives for its own worker platforms, as of `at`. */
export async function fetchCollateral(network: Network, fmspc: string, at: number): Promise<string> {
  const response = await fetch(`${network.api}/public/collateral?fmspc=${encodeURIComponent(fmspc)}&at=${at}`);
  if (!response.ok) throw new Error(`no collateral for platform ${fmspc} (HTTP ${response.status})`);
  return (await response.json()).collateral;
}

/** Whether the register contract lists these worker measurements as an approved build. */
export async function measurementsApproved(network: Network, measurements: object): Promise<boolean> {
  const { view } = await import('./chain');
  return (await view<boolean>(network, network.registry, 'is_measurements_approved', { measurements })) === true;
}
