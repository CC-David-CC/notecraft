import {zipSync, unzipSync} from './vendor/fflate.mjs';

const prefix = 'browser:/NoteCraft/';
const maximumEntries = 8192;
export const maximumBytes = 64 * 1024 * 1024;

function relative(path) {
  if (typeof path !== 'string' || path.length > 2048 || /[\\\x00-\x1f]/.test(path) ||
      path.split('/').some(part => !part || part === '.' || part === '..' || part.length > 255)) {
    throw new Error('The notebook contains an invalid relative path.');
  }
  return path;
}

export function storagePath(path) {
  if (typeof path !== 'string' || !path.startsWith(prefix)) throw new Error('File is outside the notebook.');
  return relative(path.slice(prefix.length));
}

function capacity(count, bytes) {
  if (count > maximumEntries || bytes > maximumBytes) throw new Error('The notebook exceeds the file or size limit.');
}

export function encodeNotebook(entries) {
  const tree = Object.create(null);
  let total = 0;
  for (const [path, value] of entries) {
    if (!(value instanceof Uint8Array)) throw new TypeError('Notebook file data must be bytes.');
    capacity(entries.size, total += value.length);
    tree['NoteCraft/' + relative(path)] = value;
  }
  const packed = zipSync(tree, {level: 1});
  capacity(entries.size, packed.length);
  return packed;
}

export function decodeNotebook(bytes) {
  if (!(bytes instanceof Uint8Array)) throw new TypeError('Expected a notebook file.');
  capacity(0, bytes.length);
  let count = 0, total = 0;
  const zipPaths = new Set();
  const files = unzipSync(bytes, {filter(entry) {
    if (!entry.name.startsWith('NoteCraft/')) throw new Error('This ZIP does not contain a NoteCraft notebook.');
    capacity(++count, total += entry.originalSize);
    if (zipPaths.has(entry.name)) throw new Error('A notebook entry occurs twice.');
    zipPaths.add(entry.name);
    const folder = entry.name.endsWith('/');
    const path = entry.name.slice('NoteCraft/'.length);
    if (path) relative(folder ? path.slice(0, -1) : path);
    return !folder;
  }});
  return new Map(Object.entries(files).map(([key, data]) => [key.slice('NoteCraft/'.length), data]));
}
