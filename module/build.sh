#!/bin/bash
# Builds the policy judge for wasm32-wasip2 and checks the artefact: the
# manifest is embedded (it is the outbound allowlist and declares the two
# keys bound to this build), the module imports both key interfaces, and it
# fits FastFS. Prints the sha256 a gate pins.
set -e

cd "$(dirname "$0")"

WASM_FILE="target/wasm32-wasip2/release/policy-judge.wasm"
MAX_SIZE=$((2 * 1024 * 1024))

echo "Building WASI module (wasm32-wasip2)..."
rustup target add wasm32-wasip2 2>/dev/null || true
# Source paths are rewritten to fixed prefixes, so the wasm — and the sha256
# a gate pins — does not depend on who built it or where: the same
# rust-toolchain.toml and Cargo.lock give the same bytes.
CARGO_HOME_DIR="${CARGO_HOME:-$HOME/.cargo}"
SDK_ROOT="$(cd ../../near-offshore && pwd)"
RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$CARGO_HOME_DIR=/cargo --remap-path-prefix=$SDK_ROOT=/outlayer --remap-path-prefix=$PWD=/build" \
    cargo build --target wasm32-wasip2 --release
echo ""

SIZE=$(stat -f%z "$WASM_FILE" 2>/dev/null || stat -c%s "$WASM_FILE" 2>/dev/null)
echo "WASM module: $WASM_FILE ($((SIZE / 1024)) KB)"

if ! grep -qa 'outlayer.manifest' "$WASM_FILE"; then
    echo "ERROR: the outlayer.manifest custom section is missing"
    exit 1
fi
if strings "$WASM_FILE" | grep -q -e "$HOME" -e "$PWD"; then
    echo "ERROR: $WASM_FILE still carries local paths"
    exit 1
fi
if ! command -v wasm-tools >/dev/null 2>&1; then
    echo "ERROR: wasm-tools is not installed; it checks the module's imports"
    exit 1
fi
WIT=$(wasm-tools component wit "$WASM_FILE" 2>/dev/null)
for interface in outlayer:encryption-keys outlayer:signing-keys; do
    if ! grep -q "$interface" <<<"$WIT"; then
        echo "ERROR: $WASM_FILE does not import $interface"
        exit 1
    fi
done
echo "OK: manifest embedded, no local paths, both key interfaces imported"

python3 - manifest.json src/main.rs <<'PY'
import json, re, sys
manifest = json.load(open(sys.argv[1]))
src = open(sys.argv[2]).read()
served = re.findall(r'"([a-z_]+)"', re.search(r'pub const OPERATIONS: &\[&str\] = &\[(.*?)\];', src, re.S).group(1))
if manifest["operations"] != served:
    sys.exit(f"ERROR: manifest operations {manifest['operations']} vs OPERATIONS {served}")
keys = manifest["signing_keys"] + manifest["encryption_keys"]
if any(k.get("bind") != "wasm" for k in keys):
    sys.exit("ERROR: every key must be bound to this build (bind: wasm)")
print(f"OK: {len(served)} operations agree with the code; every key is bound to this build")
PY

if [ "$SIZE" -gt "$MAX_SIZE" ]; then
    echo "ERROR: over the 2 MB FastFS limit"
    exit 1
fi
echo "SHA256: $(shasum -a 256 "$WASM_FILE" | cut -d' ' -f1)"
