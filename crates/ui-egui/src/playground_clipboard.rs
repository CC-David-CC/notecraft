// SPDX-License-Identifier: MIT OR Apache-2.0
//! NoteCraft's browser build in Playground (patches/onenote.yaml in github.com/daijro/openplayground): the `arboard`
//! that crates/ui-egui/src/clipboard.rs reads (`mod arboard` there on wasm), answered from the browser's clipboard.
//! arboard itself has no browser clipboard, so the web build leaves it out.
//!
//! The browser hands its clipboard over asynchronously (navigator.clipboard.read, which may ask the person first), so
//! the ribbon's Paste asks for it ([`ask`], first thing in `paste_here`) and runs again once it has arrived
//! ([`arrived`], first thing in the window's frame): that run reads it here, picture or words, as the desktop reads
//! arboard. Copy, Cut and Ctrl+V never come here: egui and the browser carry those.

use std::borrow::Cow;
use std::cell::RefCell;
use wasm_bindgen::{JsCast as _, JsValue};

/// Nothing to hand over: no read has arrived for this Paste, or the clipboard has no such thing.
#[allow(dead_code)]
pub enum Error {
    ClipboardOccupied,
    Nothing,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the browser's clipboard has nothing to hand over")
    }
}

/// A picture, as arboard gives it: unmultiplied RGBA rows.
pub struct ImageData<'a> {
    pub width: usize,
    pub height: usize,
    pub bytes: Cow<'a, [u8]>,
}

/// What the browser handed over for one Paste; `None` when it would not.
type Read = Option<(Option<String>, Option<(usize, usize, Vec<u8>)>)>;

enum State {
    Idle,
    Reading,
    Arrived(Read),
    /// The Paste runs again with it, this frame.
    Serving(Read),
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State::Idle) };
    static CTX: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
}

pub struct Clipboard;

impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        Ok(Clipboard)
    }

    pub fn get_image(&mut self) -> Result<ImageData<'static>, Error> {
        STATE.with_borrow(|s| match s {
            State::Serving(Some((_, Some((width, height, rgba))))) => {
                Ok(ImageData { width: *width, height: *height, bytes: Cow::Owned(rgba.clone()) })
            }
            _ => Err(Error::Nothing),
        })
    }

    /// Copying goes through egui and the browser, not here.
    pub fn set_text(&mut self, _text: String) -> Result<(), Error> {
        Err(Error::Nothing)
    }

    pub fn get_text(&mut self) -> Result<String, Error> {
        STATE.with_borrow(|s| match s {
            State::Serving(Some((Some(text), _))) => Ok(text.clone()),
            _ => Err(Error::Nothing),
        })
    }

    /// The builder arboard's rich flavours come from.
    pub fn get(&mut self) -> Get {
        Get
    }

    /// The builder arboard's rich writes come from (copying goes through egui and the browser, not here).
    pub fn set(&mut self) -> Set {
        Set
    }
}

pub struct Set;

impl Set {
    pub fn html(self, _html: String, _alt_text: Option<String>) -> Result<(), Error> {
        Err(Error::Nothing)
    }
}

pub struct Get;

impl Get {
    /// The browser's rich flavour is not read: the Paste falls back to the words.
    pub fn html(self) -> Result<String, Error> {
        Err(Error::Nothing)
    }
}

/// The Paste:`None` when the browser's clipboard has arrived (read it now); otherwise what to say meanwhile, having
/// asked the browser for it.
pub fn ask() -> Option<&'static str> {
    STATE.with_borrow_mut(|s| match s {
        State::Serving(Some(_)) => None,
        State::Serving(None) => Some("The browser did not hand over its clipboard: press Ctrl+V to paste"),
        State::Idle => {
            *s = State::Reading;
            wasm_bindgen_futures::spawn_local(async {
                let read = read().await;
                STATE.set(State::Arrived(read));
                CTX.with_borrow(|c| c.as_ref().map(egui::Context::request_repaint));
            });
            Some("Reading the clipboard…")
        }
        State::Reading | State::Arrived(_) => Some("Reading the clipboard…"),
    })
}

/// Each frame: true when the browser's clipboard has just arrived, so the Paste that asked runs again.
pub fn arrived(ctx: &egui::Context) -> bool {
    CTX.with_borrow_mut(|c| {
        c.get_or_insert_with(|| ctx.clone());
    });
    STATE.with_borrow_mut(|s| {
        let (next, again) = match std::mem::replace(s, State::Idle) {
            State::Arrived(read) => (State::Serving(read), true),
            State::Serving(_) => (State::Idle, false),
            other => (other, false),
        };
        *s = next;
        again
    })
}

fn get(o: &JsValue, k: &str) -> JsValue {
    js_sys::Reflect::get(o, &k.into()).unwrap_or(JsValue::UNDEFINED)
}

/// `o[method](...args)`, awaited.
async fn call(o: &JsValue, method: &str, args: &[JsValue]) -> Option<JsValue> {
    let f = get(o, method).dyn_into::<js_sys::Function>().ok()?;
    let promise = f.apply(o, &args.iter().collect::<js_sys::Array>()).ok()?.dyn_into::<js_sys::Promise>().ok()?;
    wasm_bindgen_futures::JsFuture::from(promise).await.ok()
}

/// The clipboard's words and its first picture.
async fn read() -> Read {
    let clipboard = get(&get(&js_sys::global(), "navigator"), "clipboard");
    let items = call(&clipboard, "read", &[]).await?;
    let (mut text, mut picture) = (None, None);
    for item in js_sys::Array::from(&items).iter() {
        for kind in js_sys::Array::from(&get(&item, "types")).iter().filter_map(|t| t.as_string()) {
            if kind == "text/plain" && text.is_none() {
                if let Some(blob) = call(&item, "getType", &[kind.into()]).await {
                    text = call(&blob, "text", &[]).await.and_then(|t| t.as_string()).map(|t| t.replace("\r\n", "\n"));
                }
            } else if kind.starts_with("image/") && picture.is_none() {
                let Some(blob) = call(&item, "getType", &[kind.into()]).await else { continue };
                let Some(buffer) = call(&blob, "arrayBuffer", &[]).await else { continue };
                picture = image::load_from_memory(&js_sys::Uint8Array::new(&buffer).to_vec()).ok().map(|i| {
                    let i = i.to_rgba8();
                    (i.width() as usize, i.height() as usize, i.into_raw())
                });
            }
        }
    }
    Some((text, picture))
}
