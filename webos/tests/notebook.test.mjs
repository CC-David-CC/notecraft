import test from 'node:test';
import assert from 'node:assert/strict';
import {encodeNotebook, decodeNotebook, storagePath} from '../notebook.mjs';

test('two native pages, page settings and a binary image survive a portable notebook round trip', () => {
  const entries = new Map([
    ['Field notes/Sketch/index.md', new TextEncoder().encode('# Sketch\nA real note')],
    ['Field notes/Notes/index.html', new TextEncoder().encode('<p>Second page</p>')],
    ['Field notes/Sketch/assets/ink.png', Uint8Array.of(0, 127, 255)],
    ['.notecraft/settings.toml', new TextEncoder().encode('last_page = "Sketch"')]
  ]);
  const bytes = encodeNotebook(entries);
  assert.deepEqual([...bytes.slice(0, 2)], [0x50, 0x4b]);
  assert.deepEqual(decodeNotebook(bytes), entries);
});

test('a notebook references only its own relative entries', () => {
  assert.equal(storagePath('browser:/NoteCraft/Field notes/Sketch/index.md'), 'Field notes/Sketch/index.md');
  for (const p of ['browser:/NoteCraft/../Other/a','browser:/Other/file','browser:/NoteCraft//file','browser:/NoteCraft/a\\b','browser:/NoteCraft/a\0b'])
    assert.throws(() => storagePath(p));
  assert.throws(() => encodeNotebook(new Map([['../other', Uint8Array.of(1)]])));
  assert.throws(() => decodeNotebook(Uint8Array.of(123,125)));
});
