// SPDX-License-Identifier: MIT OR Apache-2.0
//! NoteCraft's native egui editor running on an HTML canvas. The browser port comes from
//! daijro/openplayground (patches/onenote). WebOS gives each editor window a Files adapter.
//! The window loads the native notebook directory tree into memory, reports page changes
//! after each frame, and flushes the current page before the host saves the complete notebook.
//! The CJK font loads on demand and pen input carries pressure through to the ink editor.

#[cfg(target_arch = "wasm32")]
mod web {
    use eframe::egui;
    use notecraft_engine::{Output, Params, Session};
    use notecraft_model::playground::std::fs;
    use notecraft_store::Settings;
    use notecraft_ui_egui::NoteCraftApp;
    use std::cell::RefCell;
    use std::path::Path;
    use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};

    /// The notebooks folder, in memory and in Files (`browser:/NoteCraft`).
    pub const ROOT: &str = "/NoteCraft";
    /// The settings (theme, the page open last), personal: a dot folder is not a notebook.
    const SETTINGS: &str = "/NoteCraft/.notecraft/settings.toml";

    thread_local! {
        /// Files Playground handed in ("Open in NoteCraft"), for the next frame.
        static OPENED: RefCell<Vec<JsValue>> = const { RefCell::new(Vec::new()) };
        static FLUSH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static CTX: std::cell::OnceCell<egui::Context> = const { std::cell::OnceCell::new() };
        static CJK: RefCell<Face> = const { RefCell::new(Face::Wanted) };
        static PEN: RefCell<Vec<PenReading>> = const { RefCell::new(Vec::new()) };
    }

    /// A pen's reading from the canvas's pointer events: phase, pointer, where (CSS pixels), pressure.
    type PenReading = (egui::TouchPhase, f64, f64, f64, f32);

    /// The CJK face, which crates/text leaves out of the wasm (patches/onenote.yaml): its file beside the page.
    const CJK_FILE: &str = "native/NotoSansCJKsc-Regular.otf";
    /// The name crates/text gives it, so the window has one face of that name either way.
    const CJK_NAME: &str = "noto-sans-cjk-sc";

    enum Face {
        Wanted,
        Loading,
        Arrived(Vec<u8>),
        Added,
    }

    /// Chinese, Japanese or Korean letters (and their punctuation and full-width forms).
    fn cjk(text: &str) -> bool {
        text.chars().any(|c| matches!(c as u32, 0x2E80..=0x9FFF | 0xA960..=0xA97F | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF))
    }

    /// `fetch(url)`'s bytes.
    async fn fetch(url: &str) -> Option<Vec<u8>> {
        let global = js_sys::global();
        let response = wait(get(&global, "fetch").dyn_into::<js_sys::Function>().ok()?.call1(&global, &url.into()).ok()).await?;
        if !get(&response, "ok").is_truthy() {
            return None;
        }
        let read = get(&response, "arrayBuffer").dyn_into::<js_sys::Function>().ok()?;
        Some(js_sys::Uint8Array::new(&wait(read.call0(&response).ok()).await?).to_vec())
    }

    /// Follows the canvas's pen pointers: eframe turns them into mouse events and drops their pressure.
    fn listen_to_the_pen(canvas: &web_sys::HtmlCanvasElement) {
        use egui::TouchPhase::{Cancel, End, Move, Start};
        for (name, phase) in [("pointerdown", Start), ("pointermove", Move), ("pointerup", End), ("pointercancel", Cancel)] {
            let read = Closure::<dyn Fn(JsValue)>::new(move |e: JsValue| {
                if get(&e, "pointerType").as_string().as_deref() == Some("pen") {
                    let n = |k: &str| get(&e, k).as_f64().unwrap_or(0.0);
                    PEN.with_borrow_mut(|p| p.push((phase, n("pointerId"), n("offsetX"), n("offsetY"), n("pressure") as f32)));
                }
            });
            let _ = canvas.add_event_listener_with_callback(name, read.as_ref().unchecked_ref());
            read.forget();
        }
    }

    fn api() -> Option<JsValue> {
        js_sys::Reflect::get(&js_sys::global(), &"playgroundFiles".into()).ok().filter(JsValue::is_object)
    }

    fn get(o: &JsValue, k: &str) -> JsValue {
        js_sys::Reflect::get(o, &k.into()).unwrap_or(JsValue::UNDEFINED)
    }

    /// `playgroundFiles[method](...args)`; None when the page has none or it threw.
    fn call(method: &str, args: &[JsValue]) -> Option<JsValue> {
        let api = api()?;
        let f = get(&api, method).dyn_into::<js_sys::Function>().ok()?;
        f.apply(&api, &args.iter().collect::<js_sys::Array>()).ok()
    }

    async fn wait(v: Option<JsValue>) -> Option<JsValue> {
        let promise = v?.dyn_into::<js_sys::Promise>().ok()?;
        wasm_bindgen_futures::JsFuture::from(promise).await.ok()
    }

    pub fn repaint() {
        CTX.with(|c| c.get().map(egui::Context::request_repaint));
    }

    /// Opened from a Playground invitation: the session's notebooks stay in this tab.
    fn invited() -> bool {
        web_sys::window().and_then(|w| w.location().search().ok()).is_some_and(|q| q.split(['?', '&']).any(|p| p.starts_with("pg-session=")))
    }

    /// The notebooks this browser keeps in Files, into the in-memory folder. False when there are none (or no Files).
    async fn load() -> bool {
        let Some(list) = wait(call("files", &[format!("browser:{ROOT}").into()])).await else { return false };
        let paths: Vec<String> = js_sys::Array::from(&list).iter().filter_map(|p| p.as_string()).collect();
        for p in &paths {
            if let (Some(local), Some(bytes)) = (p.strip_prefix("browser:"), wait(call("read", &[p.into()])).await) {
                fs::put(Path::new(local), js_sys::Uint8Array::new(&bytes).to_vec());
            }
        }
        !paths.is_empty()
    }

    /// Picture links in pages (`file:///NoteCraft/…/assets/x.png`), read from the in-memory folder.
    struct Files;

    impl egui::load::BytesLoader for Files {
        fn id(&self) -> &str {
            "notecraft::web::Files"
        }
        fn load(&self, _: &egui::Context, uri: &str) -> egui::load::BytesLoadResult {
            let path = uri.strip_prefix("file://").ok_or(egui::load::LoadError::NotSupported)?.replace("%20", " ");
            match fs::read(&path) {
                Ok(bytes) => Ok(egui::load::BytesPoll::Ready { size: None, bytes: egui::load::Bytes::Shared(bytes.into()), mime: None }),
                Err(e) => Err(egui::load::LoadError::Loading(e.to_string())),
            }
        }
        fn forget(&self, _: &str) {}
        fn forget_all(&self) {}
        fn byte_size(&self) -> usize {
            0
        }
    }

    fn params(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    pub struct Web {
        pub app: NoteCraftApp,
        /// The in-memory folder is kept in Files.
        persist: bool,
        unsaved: Option<bool>,
    }

    impl Web {
        /// A page file Playground handed in: one of these notebooks' pages opens; any other becomes a new page in the
        /// Quick Notes notebook.
        fn open(&mut self, file: &JsValue) {
            let name = get(file, "name").as_string().unwrap_or_default();
            let path = get(file, "path").as_string().unwrap_or_default();
            if let Some(dir) = path.strip_prefix("browser:").map(Path::new).filter(|p| p.starts_with(ROOT)).and_then(Path::parent)
                && self.app.session().resolve(&dir.to_string_lossy()).is_ok()
            {
                return self.app.open_path(&dir.to_string_lossy());
            }
            let text = String::from_utf8_lossy(&js_sys::Uint8Array::new(&get(file, "bytes")).to_vec()).into_owned();
            let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, ""));
            let (format, body) = if ext.eq_ignore_ascii_case("md") {
                ("cmgfm", notecraft_formats::frontmatter::split_front(&text).1)
            } else {
                ("html", notecraft_formats::html::inner(&text))
            };
            let s = self.app.session_mut();
            let path = |o: Output| if let Output::Path(p) = o { Some(p.to_string_lossy().into_owned()) } else { None };
            let notebook = s.tree().iter().find(|n| n.info.title == "Quick Notes").map(|n| n.path.to_string_lossy().into_owned());
            let notebook = notebook.or_else(|| s.run("notebook.create", &params(&[("name", "Quick Notes"), ("empty", "true")])).ok().and_then(path));
            let Some(notebook) = notebook else { return };
            let section = s
                .tree()
                .iter()
                .find(|n| n.path.to_string_lossy() == notebook)
                .and_then(|n| n.children.iter().find(|c| c.info.kind == notecraft_model::NodeKind::Section));
            let section = section.map(|c| c.path.to_string_lossy().into_owned());
            let section = section.or_else(|| {
                s.run("node.create", &params(&[("parent", &notebook), ("kind", "section"), ("title", "Opened files")])).ok().and_then(path)
            });
            let Some(section) = section else { return };
            let Some(page) =
                s.run("node.create", &params(&[("parent", &section), ("kind", "page"), ("title", stem), ("format", format)])).ok().and_then(path)
            else {
                return;
            };
            let _ = s.run("page.save", &params(&[("path", &page), ("body", &body)]));
            self.app.open_path(&page);
        }

        /// The CJK face: fetched once the open page or a title in the tree has those letters, then added behind the
        /// window's faces (after the window has put its own in, which would replace it).
        fn faces(&mut self) {
            let ready = CJK.with_borrow_mut(|face| match face {
                Face::Wanted => {
                    fn titles(n: &notecraft_store::tree::Node) -> bool {
                        cjk(&n.info.title) || n.children.iter().any(titles)
                    }
                    if self.app.body().is_some_and(cjk) || self.app.session().tree().iter().any(titles) {
                        *face = Face::Loading;
                        wasm_bindgen_futures::spawn_local(async {
                            let bytes = fetch(CJK_FILE).await;
                            CJK.set(bytes.map_or(Face::Added, Face::Arrived));
                            repaint();
                        });
                    }
                    None
                }
                Face::Arrived(_) => match std::mem::replace(face, Face::Added) {
                    Face::Arrived(bytes) => Some(bytes),
                    _ => None,
                },
                _ => None,
            });
            if let (Some(bytes), Some(ctx)) = (ready, CTX.with(|c| c.get().cloned())) {
                use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
                notecraft_ui_egui::fonts::install_once(&ctx);
                let families = [egui::FontFamily::Proportional, egui::FontFamily::Monospace];
                let families = families.map(|family| InsertFontFamily { family, priority: FontPriority::Lowest }).to_vec();
                ctx.add_font(FontInsert::new(CJK_NAME, egui::FontData::from_owned(bytes), families));
            }
        }

        /// After the window's frame: files handed in, the in-memory folder's changes to Files, the unsaved state.
        fn after(&mut self) {
            for file in OPENED.take() {
                self.open(&file);
            }
            let flush = FLUSH.with(|pending| pending.replace(false));
            if flush {
                self.app.save_page_now();
            }
            for p in fs::changes() {
                if !self.persist || !p.starts_with(ROOT) {
                    continue;
                }
                let target = JsValue::from(format!("browser:{}", p.to_string_lossy()));
                match fs::metadata(&p) {
                    Ok(m) if m.is_file() => {
                        let bytes = fs::read(&p).unwrap_or_default();
                        call("write", &[target, js_sys::Uint8Array::from(bytes.as_slice()).into()]);
                    }
                    Ok(_) => {}
                    Err(_) => {
                        call("remove", &[target]);
                    }
                }
            }
            self.faces();
            let unsaved = self.app.is_dirty();
            if flush {
                call("flushed", &[unsaved.into()]);
            }
            let first_frame = self.unsaved.is_none();
            if self.unsaved.replace(unsaved) != Some(unsaved) {
                call("setUnsaved", &[unsaved.into()]);
            }
            if first_frame {
                call("ready", &[]);
            }
        }
    }

    impl eframe::App for Web {
        fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
            raw_input.events.extend(self.app.take_events());
            // The pen's pressure, as the touch force the window's stylus reader takes (as it gets it on Windows).
            let zoom = f64::from(ctx.zoom_factor());
            for (phase, id, x, y, pressure) in PEN.take() {
                let pos = egui::pos2((x / zoom) as f32, (y / zoom) as f32);
                let (device_id, id) = (egui::TouchDeviceId(1), egui::TouchId(id as u64));
                raw_input.events.push(egui::Event::Touch { device_id, id, phase, pos, force: Some(pressure) });
            }
            // A dropped file is only a name on the web: its bytes go where that name points in the in-memory
            // folder, so the window's "Insert the picture" / "Attach the file" copy it into the page.
            for f in raw_input.dropped_files.clone() {
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(bytes) = f.bytes_async().await {
                        fs::put(f.path(), bytes);
                    }
                });
            }
        }

        fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
            self.app.show(ui);
            self.after();
        }

        fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
            Some(self)
        }
    }

    pub fn start() {
        let open = Closure::<dyn Fn(JsValue)>::new(|f: JsValue| {
            OPENED.with_borrow_mut(|o| o.push(f));
            repaint();
        });
        call("onOpen", &[open.into_js_value()]);
        let flush = Closure::<dyn Fn()>::new(|| {
            FLUSH.with(|pending| pending.set(true));
            repaint();
        });
        call("onFlush", &[flush.into_js_value()]);
        wasm_bindgen_futures::spawn_local(async {
            let persist = !invited() && ["files", "write", "remove"].iter().all(|m| api().is_some_and(|a| get(&a, m).is_function()));
            if persist {
                load().await;
            }
            let document = web_sys::window().and_then(|w| w.document()).expect("no document");
            let canvas = document
                .get_element_by_id("notecraft_canvas")
                .and_then(|c| c.dyn_into::<web_sys::HtmlCanvasElement>().ok())
                .expect("no #notecraft_canvas");
            listen_to_the_pen(&canvas);
            let started = eframe::WebRunner::new()
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(move |cc| {
                        CTX.with(|c| c.set(cc.egui_ctx.clone()).ok());
                        cc.egui_ctx.add_bytes_loader(std::sync::Arc::new(Files));
                        let session = Session::new(Path::new(ROOT), Settings::at(SETTINGS)).map_err(|e| e.to_string())?;
                        let app = NoteCraftApp::new(session).with_welcome_notebook_setup(|root, format| {
                            let files = call("welcome", &[format.into()])
                                .and_then(|value| value.dyn_into::<js_sys::Array>().ok())
                                .ok_or_else(|| "The Welcome notebook could not be loaded.".to_owned())?;
                            let destination = root.join("welcome-to-notecraft");
                            let mut edition = Vec::new();
                            for row in files {
                                let row = js_sys::Array::from(&row);
                                let key = row.get(0).as_string().ok_or("Invalid Welcome notebook path.")?;
                                let parts = Path::new(&key);
                                if !parts.components().all(|part| matches!(part, std::path::Component::Normal(_))) {
                                    return Err("Invalid Welcome notebook path.".to_owned());
                                }
                                edition.push((destination.join(parts), js_sys::Uint8Array::new(&row.get(1)).to_vec()));
                            }
                            if !edition.iter().any(|(p, _)| p == &destination.join("_node.toml")) {
                                return Err("Incomplete Welcome notebook.".to_owned());
                            }
                            if fs::metadata(&destination).is_ok() {
                                fs::remove_dir_all(&destination).map_err(|e| e.to_string())?;
                            }
                            for (file, data) in edition {
                                fs::write(file, data).map_err(|e| e.to_string())?;
                            }
                            Ok(destination)
                        });
                        Ok(Box::new(Web { app, persist, unsaved: None }))
                    }),
                )
                .await;
            if let Some(loading) = document.get_element_by_id("notecraft_loading") {
                match started {
                    Ok(()) => loading.remove(),
                    Err(e) => loading.set_text_content(Some(&format!("NoteCraft could not start: {e:?}"))),
                }
            }
        });
    }
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    web::start();
}
