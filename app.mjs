import {storagePath, maximumBytes} from './webos/notebook.mjs';

const validSha = value => typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
function workspacePath(value) {
  if (typeof value !== 'string' || value.length > 2048 ||
      value.split('/').some(part => !part || part === '.' || part === '..' || /[\\\x00-\x1f]/.test(part))) {
    throw new Error('Choose a notebook in Files.');
  }
  return value;
}

function archive(operation, signal) {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./webos/archive-worker.mjs', import.meta.url), {type:'module'});
    const end = (fn, result) => { worker.terminate(); signal.removeEventListener('abort', abort); fn(result); };
    const abort = () => end(reject, new DOMException('Window closed.', 'AbortError'));
    if (signal.aborted) return abort();
    signal.addEventListener('abort', abort, {once:true});
    worker.onmessage = ({data}) => data.error ? end(reject, Error(data.error)) : end(resolve, data);
    worker.onerror = event => end(reject, Error(event.message || 'Notebook conversion failed.'));
    worker.postMessage(operation);
  });
}

export function mount(host) {
  const session = host.session();
  const owner = host.storage.workspaceOwner();
  const stop = new AbortController();
  const {signal} = stop;
  const element = document.createElement('section');
  element.className = 'window-content notecraft-webos';
  const style = document.createElement('link');
  style.rel = 'stylesheet';
  style.href = new URL('./webos/app.css', import.meta.url);
  const toolbar = document.createElement('div');
  toolbar.className = 'notecraft-filebar';
  toolbar.setAttribute('role','toolbar');
  toolbar.setAttribute('aria-label','Notebook files');
  const status = document.createElement('span');
  status.setAttribute('role','status');
  const frame = document.createElement('iframe');
  frame.title = 'NoteCraft notebook';
  frame.allow = 'clipboard-read; clipboard-write';
  const buttons = [];
  let loaded = false, paused = false, editing = false, revision = 0, savedRevision = 0;
  let entries = new Map(), binding = null, ctx, unregister;
  let welcomeBundle;
  let nativeFlush, flushWait, pending, timer, firstDirtyAt = 0, lastSave = 0;
  const changed = () => editing || revision !== savedRevision;
  function check() {
    if (signal.aborted || session !== host.session() || owner !== host.storage.workspaceOwner() || (ctx && !ctx.valid())) {
      throw new DOMException('The notebook session ended.', 'AbortError');
    }
  }
  function message(text) { if (!signal.aborted) status.textContent = text; }
  function controls() { for (const button of buttons) button.disabled = !loaded || paused || !!pending; }
  function button(label, action) {
    const b = document.createElement('button');
    b.type='button'; b.textContent=label; b.disabled=true;
    b.addEventListener('click', () => void action().catch(error => message(error.message)), {signal});
    toolbar.append(b); buttons.push(b);
  }
  function schedule() {
    if (!binding || !loaded || paused || !changed() || signal.aborted || pending) return;
    if (!firstDirtyAt) firstDirtyAt = Date.now();
    clearTimeout(timer);
    timer = setTimeout(() => void save(false).catch(error => message(error.message)),
      Math.max(0, 60000 - (Date.now() - lastSave), Math.min(10000, 30000 - (Date.now() - firstDirtyAt))));
  }
  function change() { revision++; if (loaded) { message('Notebook has unsaved changes.'); schedule(); } }
  async function pick(options) {
    const result = await host.storage.pickWorkspaceFile({...options, signal});
    check();
    if (!result) return null;
    if (result === '/' && options.foldersOnly) return '';
    return workspacePath(result.replace(/^\//,''));
  }
  button('Open…', async () => {
    const path = await pick({extensions:['.notecraft']});
    if (!path) return;
    const file = await ctx.files.version(path);
    check();
    if (!validSha(file.sha256)) throw new Error('Could not verify the selected notebook.');
    host.desktop.openAccountFile('notecraft', {owner, path, sha256:file.sha256});
  });
  button('Save', () => save(false));
  button('Save as…', () => save(true));
  toolbar.append(status);
  element.append(style,toolbar,frame);
  message('Opening NoteCraft…');

  function flush() {
    if (!nativeFlush) throw new Error('The notebook editor is still starting.');
    return new Promise((resolve,reject) => {
      const timeout = setTimeout(() => settle(Error('The notebook editor did not finish saving.')),15000);
      const settle = error => {
        if (!flushWait) return;
        clearTimeout(timeout); flushWait=null;
        error ? reject(error) : resolve();
      };
      flushWait = settle;
      try { nativeFlush(); } catch(error) { settle(error); }
    });
  }
  async function persist(asCopy) {
    check();
    let dest = binding;
    if (asCopy || !dest) {
      const dir = await pick({foldersOnly:true});
      if (dir == null) return;
      const name = await host.ui.prompt('Notebook name', {title:'Save notebook to Files',
        value:binding?.path.split('/').pop() || 'My notebook.notecraft', confirmLabel:'Save', signal});
      check();
      if (name == null) return;
      const leaf = name.trim();
      if (!leaf || /[/\\]/.test(leaf) || leaf.length > 245) throw new Error('Enter one notebook name.');
      const path = workspacePath((dir ? dir+'/' : '')+leaf+(/\.notecraft$/i.test(leaf)?'':'.notecraft'));
      dest = binding?.path === path ? binding : {path,sha256:null};
    }
    await flush(); check();
    const captured = revision;
    if (!asCopy && dest === binding && captured === savedRevision) { message('Saved to Files.'); return; }
    message('Saving notebook to Files…');
    const {bytes} = await archive({action:'save',entries}, signal);
    check();
    const result = await ctx.files.write(dest.path, new Blob([bytes], {type:'application/zip'}), dest.sha256, {signal});
    check();
    if (!validSha(result.sha256)) throw new Error('Files could not confirm the saved revision.');
    binding = {path:dest.path, sha256:result.sha256};
    savedRevision = captured; lastSave = Date.now();
    host.desktop.updateAccountFileBinding(host.w,{owner,...binding});
    message(changed() ? 'Saving recent changes…' : 'Saved to Files.');
  }
  async function save(asCopy) {
    check();
    if (!loaded || paused) return;
    if (pending) return pending;
    clearTimeout(timer); firstDirtyAt=0;
    let success = false;
    pending = persist(asCopy).then(() => {success = true;}).catch(error => {
      if (error?.status === 409) throw Error('This notebook changed in Files. Save as a new notebook to keep your edits.');
      throw error;
    }).finally(() => {pending=null; controls(); if(success) schedule();});
    controls();
    return pending;
  }

  let markReady, failReady;
  const firstFrame = new Promise((resolve,reject) => {markReady=resolve; failReady=reject;});
  firstFrame.catch(() => {});
  const api = {
    files: async root => { check(); if (root !== 'browser:/NoteCraft') throw Error('Unknown notebook root.');
      return [...entries.keys()].map(key=>'browser:/NoteCraft/'+key); },
    read: async path => {check(); return entries.get(storagePath(path));},
    write(path, data) {
      check();
      const key = storagePath(path);
      // Uint8Array originates in the notebook iframe.
      if (!ArrayBuffer.isView(data) || data.BYTES_PER_ELEMENT !== 1) throw Error('Invalid native notebook bytes.');
      entries.set(key,new Uint8Array(data)); change();
    },
    remove(path) {
      check();
      const key = storagePath(path);
      let removed = entries.delete(key);
      for (const p of entries.keys()) if (p.startsWith(key+'/')) { entries.delete(p); removed=true; }
      if (removed) change();
    },
    setUnsaved(value) { editing = !!value; if (editing && loaded) { message('Notebook has unsaved changes.'); schedule(); } },
    welcome(format) {
      const edition = welcomeBundle?.manifest[format];
      if (!edition) return null;
      return Object.entries(edition).map(([path,hash]) => [path,welcomeBundle.entries.get(hash)]);
    },
    onFlush(fn) { nativeFlush = fn; },
    failed(reason) {
      const error = Error('NoteCraft could not continue. '+reason);
      flushWait?.(error); message(error.message); failReady(error);
    },
    flushed(dirty) { flushWait?.(dirty ? Error('NoteCraft could not save the current page. Your edit is still open.') : null); },
    ready() { if (!loaded) {loaded=true; savedRevision=revision; controls(); message(binding ? 'Opened from Files.' : 'New notebook'); markReady();} }
  };
  async function start() {
    ctx = await host.storage.workspaceFileContext(); check();
    if (ctx.owner !== owner) throw Error('The selected notebook belongs to a different workspace.');
    const selected = host.w.launchContext;
    if (selected) {
      if (selected.storage !== 'account' || selected.owner !== owner) throw Error('The selected notebook belongs to a different workspace.');
      const path = workspacePath(selected.path);
      const file = await ctx.files.read(path,{signal}); check();
      if (!validSha(file.sha256) || (!selected.restore && selected.sha256 !== file.sha256)) throw Error('The notebook changed in Files. Open its current revision.');
      if ((file.data.size ?? file.data.byteLength) > maximumBytes) throw Error('The notebook exceeds the file size limit.');
      const bytes = file.data instanceof Blob ? new Uint8Array(await file.data.arrayBuffer()) : new Uint8Array(file.data);
      ({entries} = await archive({action:'open', bytes},signal)); check();
      binding = {path,sha256:file.sha256};
    }
    unregister = ctx.registerFileWriter?.(async () => {
      if (pending) await pending;
      if (changed()) {
        if (!binding) throw Error('Save the new NoteCraft notebook to Files before transferring this workspace.');
        await save(false);
      }
      paused=true; controls(); frame.inert=true;
      return () => {paused=false; controls(); frame.inert=false; schedule();};
    });
    const loadWelcome = async () => {
      const response = await fetch(new URL('./webos/native/welcome.zip',import.meta.url),{signal});
      if (!response.ok) throw Error('Could not load the Welcome notebook.');
      const bytes = new Uint8Array(await response.arrayBuffer());
      const bundle = (await archive({action:'open',bytes},signal)).entries;
      const manifest = JSON.parse(new TextDecoder().decode(bundle.get('editions.json')));
      check(); welcomeBundle={manifest,entries:bundle};
    };
    // First-time setup offers either official notebook edition. Existing projects can open immediately.
    if (!entries.has('.notecraft/settings.toml')) await loadWelcome();
    else void loadWelcome().catch(error => message(error.message));
    frame.addEventListener('webos-notecraft-attach', () => {
      check();
      Object.defineProperty(frame.contentWindow,'webosNotebook',{value:api});
    },{signal});
    frame.src = new URL('./webos/index.html',import.meta.url);
    return firstFrame;
  }
  const ready = start().catch(error => {message(error.message); throw error;});
  ready.catch(()=>{});
  return {
    element, ready, focus:() => frame.focus(),
    inspect:() => ({ready:loaded, dirty:changed(), pending:!!pending, path:binding?.path ?? null}),
    async requestClose() {
      if (pending) await pending;
      if (!changed()) return true;
      if (binding) { await save(false); return !changed(); }
      const previous = revision;
      return await host.ui.confirm('Discard the unsaved notebook?', {confirmLabel:'Discard and close',signal}) && previous===revision;
    },
    dispose() {
      clearTimeout(timer); unregister?.(); stop.abort();
      const error = new DOMException('Notebook closed.', 'AbortError');
      flushWait?.(error); failReady(error);
      frame.remove(); entries.clear();
    }
  };
}
