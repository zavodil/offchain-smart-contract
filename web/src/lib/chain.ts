/** Reading NEAR: view calls, access keys and accounts, over JSON-RPC. */

import type { Network } from './network';

async function rpc(network: Network, method: string, params: object): Promise<any> {
  let last: unknown;
  for (let attempt = 0; attempt < 4; attempt++) {
    try {
      const response = await fetch(network.rpc, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: 'offchain-smart-contract', method, params }),
      });
      const json = await response.json();
      if (json.error) throw new Error(`RPC ${method}: ${JSON.stringify(json.error).slice(0, 300)}`);
      return json.result;
    } catch (e) {
      last = e;
      await new Promise((r) => setTimeout(r, 1500 * (attempt + 1)));
    }
  }
  throw last;
}

const base64 = (text: string) => (typeof btoa === 'function' ? btoa(unescape(encodeURIComponent(text))) : Buffer.from(text).toString('base64'));

/** A view call, its JSON answer parsed. */
export async function view<T>(network: Network, account: string, method: string, args: object = {}): Promise<T> {
  const result = await rpc(network, 'query', {
    request_type: 'call_function',
    finality: 'final',
    account_id: account,
    method_name: method,
    args_base64: base64(JSON.stringify(args)),
  });
  if (result.error) throw new Error(`${account}.${method}: ${result.error}`);
  return JSON.parse(new TextDecoder().decode(new Uint8Array(result.result)));
}

export interface AccessKey {
  public_key: string;
  access_key: { permission: 'FullAccess' | { FunctionCall: { receiver_id: string; method_names: string[] } } };
}

export async function accessKeys(network: Network, account: string): Promise<AccessKey[]> {
  return (await rpc(network, 'query', { request_type: 'view_access_key_list', finality: 'final', account_id: account })).keys;
}

/** The sha256 of the code deployed on `account`, base58 as NEAR reports it. */
export async function codeHash(network: Network, account: string): Promise<string> {
  return (await rpc(network, 'query', { request_type: 'view_account', finality: 'final', account_id: account })).code_hash;
}
