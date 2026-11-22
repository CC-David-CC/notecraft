"""Build both official notebook editions, sharing identical assets across editions."""
import hashlib
import json
import pathlib
import sys
import zipfile

sources = pathlib.Path(__file__).resolve().parents[1] / 'docs/demo/DemoNotebook'
seen = set()
manifest = {}
with zipfile.ZipFile(sys.argv[1], 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as bundle:
    def add(name, data):
        entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
        entry.compress_type = zipfile.ZIP_DEFLATED
        bundle.writestr(entry, data)
    for edition in ('html', 'cmgfm'):
        root = sources / edition
        files = manifest[edition] = {}
        for path in sorted(root.rglob('*')):
            if path.is_file():
                data = path.read_bytes()
                digest = hashlib.sha256(data).hexdigest()
                files[path.relative_to(root).as_posix()] = digest
                if digest not in seen:
                    add('NoteCraft/'+digest, data)
                    seen.add(digest)
    add('NoteCraft/editions.json', json.dumps(manifest, separators=(',', ':')).encode())
