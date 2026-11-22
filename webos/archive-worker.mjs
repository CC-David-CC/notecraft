import {encodeNotebook, decodeNotebook} from './notebook.mjs';
self.addEventListener('message', ({data}) => {
  try {
    if (data.action === 'open') {
      const entries = decodeNotebook(data.bytes);
      self.postMessage({entries});
    } else if (data.action === 'save') {
      const bytes = encodeNotebook(data.entries);
      self.postMessage({bytes}, [bytes.buffer]);
    } else throw new Error('Unknown notebook operation.');
  } catch (error) {
    self.postMessage({error: error.message});
  }
});
