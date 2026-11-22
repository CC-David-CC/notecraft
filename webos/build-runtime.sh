#!/usr/bin/env bash
# Generate the browser runtime in a separate output directory. Target artifacts remain untracked.
set -euo pipefail
if [[ $# != 1 ]]; then echo 'Usage: webos/build-runtime.sh OUTPUT_DIRECTORY' >&2; exit 2; fi
root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$1"
out=$(cd "$1" && pwd)
cd "$root"
version=$(awk '/^name = "wasm-bindgen"$/{getline;gsub(/"/,"",$3);print $3;exit}' Cargo.lock)
if [[ $(wasm-bindgen --version) != "wasm-bindgen $version" ]]; then echo "Install wasm-bindgen-cli $version." >&2; exit 1; fi
cargo build --locked --target wasm32-unknown-unknown --profile playground-web -p notecraft-web
target_dir=${CARGO_TARGET_DIR:-$root/target}
wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name notecraft "$target_dir/wasm32-unknown-unknown/playground-web/notecraft-web.wasm"
install -m 0644 assets/fonts/NotoSansCJKsc-Regular.otf assets/fonts/OFL-NotoSansCJKsc.txt "$out/"
python3 webos/build-welcome.py "$out/welcome.zip"
python3 webos/runtime-receipt.py "$out"
