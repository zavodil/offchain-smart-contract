#!/usr/bin/env bash
# Publishes every pack here to the registry: `publish.sh <registry-account> <publisher-account> [network]`.
# The publisher signs with its key in ~/.near-credentials/<network>/; the
# storage deposit is 0.05 NEAR a pack, the excess comes back.
set -euo pipefail
cd "$(dirname "$0")"
REGISTRY=${1:?registry account}; PUBLISHER=${2:?publisher account}; NETWORK=${3:-testnet}
for file in *.json; do
  id=$(jq -r .id "$file"); version=$(jq -r .version "$file")
  if near contract call-function as-read-only "$REGISTRY" get_meta json-args "$(jq -nc --arg i "$id" --argjson v "$version" '{id:$i, version:$v}')" network-config "$NETWORK" now 2>/dev/null | grep -q '"sha256"'; then
    echo "$id@$version: already published"; continue
  fi
  near contract call-function as-transaction "$REGISTRY" publish \
    json-args "$(jq -c --arg i "$id" --argjson v "$version" '{id:$i, version:$v, bytes: (. | tojson)}' "$file")" \
    prepaid-gas '30.0 Tgas' attached-deposit '0.05 NEAR' \
    sign-as "$PUBLISHER" network-config "$NETWORK" sign-with-legacy-keychain send
  echo "$id@$version: published, sha256 $(jq -j tojson "$file" | shasum -a 256 | cut -d' ' -f1)"
done
