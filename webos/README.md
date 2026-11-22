# NoteCraft for WebOS

NoteCraft uses the same Rust editor, notebook tree and page formats as its desktop application. Open NoteCraft from Office. **Save** puts the open notebook in Files. **Open** or a double-click on its `.notecraft` file reopens it. Anon uses the current browser workspace. Signed-in users can open a saved notebook from their account.

A `.notecraft` file is a ZIP containing the native notebook directories under `NoteCraft/`. HTML, Markdown, attachments and settings stay alongside their pages. Each save uses the previous Files revision. If another window saves first, save a separate copy to preserve your edits. Archives have a 64 MiB total uncompressed size limit and support up to 8,192 entries.

## Build and test

Use the Rust toolchain in `rust-toolchain.toml`, the wasm32-unknown-unknown target, and the wasm-bindgen CLI version in Cargo.lock. From the repository root, run the following commands.

```sh
webos/build-runtime.sh webos/native
node --test webos/tests/notebook.test.mjs
```

The browser entry point is `app.mjs`. The browser adaptation starts from OpenPlayground's NoteCraft web.diff and browser rules at commit 4317d6b7ec991926713fd1af1bc715c42c0e8a55. Notebook ZIP support uses fflate 0.8.2, distributed under the MIT license. [Browser port](https://github.com/daijro/openplayground/tree/4317d6b7ec991926713fd1af1bc715c42c0e8a55/patches/onenote) · [fflate license](vendor/fflate-LICENSE)
