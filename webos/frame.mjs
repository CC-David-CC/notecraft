// Load the native application only after its own WebOS view supplies its Files adapter.
if (parent === window) throw new Error('Open NoteCraft from WebOS.');
const attachment = new Event('webos-notecraft-attach');
window.frameElement.dispatchEvent(attachment);
const host = globalThis.webosNotebook;
if (!host) throw new Error('The NoteCraft file adapter is unavailable.');
// wasm-bindgen checks Promise and typed-array types in the editor's own JavaScript realm.
globalThis.playgroundFiles = {
  files: async path => Array.from(await host.files(path)),
  read: async path => new Uint8Array(await host.read(path)),
  write: (path,data) => host.write(path,data),
  remove: path => host.remove(path),
  setUnsaved: value => host.setUnsaved(value),
  welcome: format => {
    const files = host.welcome(format);
    return files && Array.from(files, ([name,bytes]) => [name,new Uint8Array(bytes)]);
  },
  onFlush: handler => host.onFlush(handler), flushed: value => host.flushed(value),
  ready: () => host.ready(), failed: message => host.failed(message)
};
const failed = reason => globalThis.playgroundFiles.failed(String(reason?.message ?? reason));
window.addEventListener('error', event => {
  // The browser can defer resize notifications to a later frame.
  if (event.error == null && event.message === 'ResizeObserver loop completed with undelivered notifications.') return;
  failed(event.error ?? event.message);
});
window.addEventListener('unhandledrejection', event => failed(event.reason));
try {
  const {default: initialize} = await import('./native/notecraft.js');
  await initialize();
} catch (error) { failed(error); }
