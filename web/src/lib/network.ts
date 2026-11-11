/** Where the demo lives, and the services its proofs are checked against. */

export interface Network {
  id: 'testnet' | 'mainnet';
  /** OutLayer's HTTPS API: attestations and Intel collateral. */
  api: string;
  /** NEAR RPC. A keyed URL in RPC_URL takes precedence when it is set (Node only). */
  rpc: string;
  /** The contract holding approved OutLayer worker measurements. */
  registry: string;
  explorer: string;
}

const envRpc = typeof process !== 'undefined' ? process.env?.RPC_URL : undefined;

export const TESTNET: Network = {
  id: 'testnet',
  api: 'https://testnet-api.outlayer.ai',
  rpc: envRpc || 'https://rpc.testnet.fastnear.com',
  registry: 'worker.outlayer.testnet',
  explorer: 'https://testnet.nearblocks.io',
};

/** OutLayer attestations commit to the extended field set from this instant on. */
export const ATTESTATION_V1_FROM = 1769784939;

/** The deployment this repository runs and checks. */
export const DEMO = {
  outlayer: 'outlayer.testnet',
  gate: 'ai-policy-gate.testnet',
  policies: 'policies.zavodil.testnet',
  fund: 'digital-art-fund.testnet',
  council: 'council.digital-art-fund.testnet',
  token: 'usdc.fakes.testnet',
  markets: 'ai-prediction-markets.testnet',
  moderator: 'moderator.ai-prediction-markets.testnet',
};

/** What the judge signs before a verdict's message. */
export const VERDICT_PREFIX = 'policy-judge/verdict/1\n';
