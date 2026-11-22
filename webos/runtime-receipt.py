"""Record the native browser artifacts produced from the current NoteCraft commit."""
import hashlib
import json
import pathlib
import subprocess
import sys

runtime = pathlib.Path(sys.argv[1])
root = pathlib.Path(__file__).resolve().parents[1]
def git(*args):
    return subprocess.check_output(['git', '-C', str(root), *args], text=True).strip()
def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()
if git('status', '--porcelain', '--untracked-files=no'):
    raise SystemExit('Commit the selected source before recording a release build.')
inventory = []
for name in ('notecraft.js', 'notecraft_bg.wasm', 'NotoSansCJKsc-Regular.otf', 'OFL-NotoSansCJKsc.txt', 'welcome.zip'):
    path = runtime / name
    inventory.append({'path':name, 'bytes':path.stat().st_size, 'sha256':digest(path)})
result = {'schema':'webos-component-build/1', 'component':'notecraft','sourceCommit':git('rev-parse','HEAD'),
          'sourceTree':git('rev-parse','HEAD^{tree}'), 'cargoLockSHA256':digest(root/'Cargo.lock'),
          'wasmBindgen':subprocess.check_output(['wasm-bindgen','--version'],text=True).strip(), 'files':inventory}
(runtime / 'component-build.json').write_text(json.dumps(result,sort_keys=True,indent=2)+'\n')
