// SPDX-License-Identifier: MIT OR Apache-2.0
//! NoteCraft's browser build in Playground (patches/onenote.yaml in github.com/daijro/openplayground).
//! wasm32-unknown-unknown has no files and no clock, so the modules that use them take `std` from here instead (one
//! `#[cfg(target_arch = "wasm32")] use notecraft_model::playground::std;` line each): the same std, except
//!   fs    an in-memory file system holding the notebooks folder; every change is noted (see [`std::fs::changes`]) so
//!         the page can keep browser storage in step with it;
//!   time  web-time's Instant and SystemTime, which work in the browser.

pub mod std {
    pub use ::std::*;

    pub mod time {
        pub use web_time::*;
    }

    pub mod fs {
        pub use ::std::fs::*;
        use ::std::{
            cell::RefCell,
            collections::{BTreeMap, BTreeSet},
            io,
            path::{Component, Path, PathBuf},
        };

        enum Entry {
            Dir,
            /// The bytes and their [`content_id`].
            File(Vec<u8>, String),
        }

        #[derive(Default)]
        struct Tree {
            entries: BTreeMap<PathBuf, Entry>,
            /// Paths written, created or removed since the last [`changes`].
            changed: BTreeSet<PathBuf>,
            /// Goes up with every change.
            generation: u64,
        }

        thread_local! {
            static TREE: RefCell<Tree> = RefCell::default();
        }

        fn with<T>(f: impl FnOnce(&mut Tree) -> T) -> T {
            TREE.with(|t| f(&mut t.borrow_mut()))
        }

        /// An id that names the content: FNV-1a over the bytes, and their length.
        // ponytail: not a cryptographic hash; collisions need intent, and the people who could craft one can edit the notebook anyway.
        pub fn content_id(bytes: &[u8]) -> String {
            let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
            format!("{hash:016x}-{}", bytes.len())
        }

        /// `/a/./b/../c` -> `/a/c`: one spelling per file.
        pub fn key(p: &Path) -> PathBuf {
            let mut out = PathBuf::from("/");
            for c in p.components() {
                match c {
                    Component::ParentDir => {
                        out.pop();
                    }
                    Component::Normal(n) => out.push(n),
                    _ => {}
                }
            }
            out
        }

        fn missing(p: &Path) -> io::Error {
            io::Error::new(io::ErrorKind::NotFound, format!("{} does not exist", p.display()))
        }

        impl Tree {
            fn mark(&mut self, p: PathBuf) {
                self.generation += 1;
                self.changed.insert(p);
            }
            fn parents(&mut self, p: &Path) {
                for dir in p.ancestors().skip(1) {
                    self.entries.entry(dir.to_path_buf()).or_insert(Entry::Dir);
                }
            }
            fn put(&mut self, p: PathBuf, bytes: Vec<u8>) {
                self.parents(&p);
                let id = content_id(&bytes);
                self.entries.insert(p, Entry::File(bytes, id));
            }
            /// `p` and everything below it.
            fn below(&self, p: &Path) -> Vec<PathBuf> {
                self.entries.range(p.to_path_buf()..).map(|(k, _)| k).take_while(|k| k.starts_with(p)).cloned().collect()
            }
        }

        /// Stores a file without noting a change (files the page loads from browser storage).
        pub fn put(p: &Path, bytes: Vec<u8>) {
            with(|t| t.put(key(p), bytes));
        }

        /// The paths changed since the last call: each now holds what browser storage should hold, or is gone.
        pub fn changes() -> Vec<PathBuf> {
            with(|t| ::std::mem::take(&mut t.changed).into_iter().collect())
        }

        /// Goes up with every change, so a caller can tell nothing changed.
        pub fn generation() -> u64 {
            with(|t| t.generation)
        }

        /// Every file at or below `root` with its bytes and content id, in path order.
        pub fn walk(root: &Path, mut f: impl FnMut(&Path, &[u8], &str)) {
            let root = key(root);
            TREE.with(|t| {
                for (p, e) in t.borrow().entries.range(root.clone()..).take_while(|(k, _)| k.starts_with(&root)) {
                    if let Entry::File(bytes, id) = e {
                        f(p, bytes, id);
                    }
                }
            });
        }

        /// The content id of the file at `p`.
        pub fn id_of(p: &Path) -> Option<String> {
            TREE.with(|t| match t.borrow().entries.get(&key(p)) {
                Some(Entry::File(_, id)) => Some(id.clone()),
                _ => None,
            })
        }

        pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(p: P, bytes: C) -> io::Result<()> {
            let p = key(p.as_ref());
            with(|t| {
                t.put(p.clone(), bytes.as_ref().to_vec());
                t.mark(p);
            });
            Ok(())
        }

        pub fn read<P: AsRef<Path>>(p: P) -> io::Result<Vec<u8>> {
            TREE.with(|t| match t.borrow().entries.get(&key(p.as_ref())) {
                Some(Entry::File(bytes, _)) => Ok(bytes.clone()),
                _ => Err(missing(p.as_ref())),
            })
        }

        pub fn read_to_string<P: AsRef<Path>>(p: P) -> io::Result<String> {
            String::from_utf8(read(p)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }

        pub fn create_dir_all<P: AsRef<Path>>(p: P) -> io::Result<()> {
            let p = key(p.as_ref());
            with(|t| {
                if !t.entries.contains_key(&p) {
                    t.parents(&p);
                    t.entries.insert(p.clone(), Entry::Dir);
                    t.mark(p);
                }
            });
            Ok(())
        }

        pub fn create_dir<P: AsRef<Path>>(p: P) -> io::Result<()> {
            if metadata(p.as_ref()).is_ok() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", p.as_ref().display())));
            }
            create_dir_all(p)
        }

        pub fn remove_file<P: AsRef<Path>>(p: P) -> io::Result<()> {
            let p = key(p.as_ref());
            with(|t| match t.entries.get(&p) {
                Some(Entry::File(..)) => {
                    t.entries.remove(&p);
                    t.mark(p);
                    Ok(())
                }
                _ => Err(missing(&p)),
            })
        }

        /// Removes an empty folder.
        pub fn remove_dir<P: AsRef<Path>>(p: P) -> io::Result<()> {
            let p = key(p.as_ref());
            with(|t| {
                if t.below(&p).len() != 1 || !matches!(t.entries.get(&p), Some(Entry::Dir)) {
                    return Err(io::Error::other(format!("{} is not an empty folder", p.display())));
                }
                t.entries.remove(&p);
                t.mark(p);
                Ok(())
            })
        }

        pub fn remove_dir_all<P: AsRef<Path>>(p: P) -> io::Result<()> {
            let p = key(p.as_ref());
            with(|t| {
                let gone = t.below(&p);
                if gone.is_empty() {
                    return Err(missing(&p));
                }
                for g in gone {
                    t.entries.remove(&g);
                }
                t.mark(p);
                Ok(())
            })
        }

        pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<()> {
            let (from, to) = (key(from.as_ref()), key(to.as_ref()));
            let bytes = read(&from)?;
            with(|t| {
                t.entries.remove(&from);
                t.mark(from);
                t.put(to.clone(), bytes);
                t.mark(to);
            });
            Ok(())
        }

        pub fn copy<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<u64> {
            let bytes = read(from)?;
            let n = bytes.len() as u64;
            write(to, bytes)?;
            Ok(n)
        }

        pub fn canonicalize<P: AsRef<Path>>(p: P) -> io::Result<PathBuf> {
            let p = key(p.as_ref());
            metadata(&p).map(|_| p)
        }

        pub struct Metadata(Option<u64>);
        impl Metadata {
            pub fn len(&self) -> u64 {
                self.0.unwrap_or(0)
            }
            pub fn is_file(&self) -> bool {
                self.0.is_some()
            }
            pub fn is_dir(&self) -> bool {
                self.0.is_none()
            }
        }

        pub fn metadata<P: AsRef<Path>>(p: P) -> io::Result<Metadata> {
            TREE.with(|t| match t.borrow().entries.get(&key(p.as_ref())) {
                Some(Entry::File(bytes, _)) => Ok(Metadata(Some(bytes.len() as u64))),
                Some(Entry::Dir) => Ok(Metadata(None)),
                None => Err(missing(p.as_ref())),
            })
        }

        pub struct DirEntry(PathBuf);
        impl DirEntry {
            pub fn path(&self) -> PathBuf {
                self.0.clone()
            }
            pub fn file_name(&self) -> ::std::ffi::OsString {
                self.0.file_name().unwrap_or_default().to_os_string()
            }
            pub fn metadata(&self) -> io::Result<Metadata> {
                metadata(&self.0)
            }
            /// What it is (is_dir, is_file), as its metadata says.
            pub fn file_type(&self) -> io::Result<Metadata> {
                self.metadata()
            }
        }

        pub fn read_dir<P: AsRef<Path>>(p: P) -> io::Result<::std::vec::IntoIter<io::Result<DirEntry>>> {
            let dir = key(p.as_ref());
            if !metadata(&dir)?.is_dir() {
                return Err(io::Error::other(format!("{} is not a folder", dir.display())));
            }
            let entries =
                TREE.with(|t| t.borrow().below(&dir).into_iter().filter(|k| k.parent() == Some(&dir)).map(|k| Ok(DirEntry(k))).collect::<Vec<_>>());
            Ok(entries.into_iter())
        }
    }
}
