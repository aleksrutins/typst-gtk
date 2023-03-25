// This file is selectively copy-pasted from Typst's CLI
use std::cell::{RefCell, RefMut};
use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::io::Read;
use std::path::{Path, PathBuf};

use comemo::Prehashed;
use elsa::FrozenVec;
use memmap2::Mmap;
use once_cell::unsync::OnceCell;
use same_file::Handle;
use siphasher::sip128::{Hasher128, SipHasher};
use typst::diag::{FileError, FileResult};
use typst::eval::Library;
use typst::font::{Font, FontBook, FontInfo};
use typst::syntax::{Source, SourceId};
use typst::util::{Buffer, PathExt};
use typst::World;
use walkdir::WalkDir;

/// A world that provides access to the operating system.
pub struct SystemWorld {
    pub root: PathBuf,
    pub library: Prehashed<Library>,
    pub book: Prehashed<FontBook>,
    pub fonts: Vec<FontSlot>,
    pub hashes: RefCell<HashMap<PathBuf, FileResult<PathHash>>>,
    pub paths: RefCell<HashMap<PathHash, PathSlot>>,
    pub sources: FrozenVec<Box<Source>>,
    pub main: SourceId,
}

/// Holds details about the location of a font and lazily the font itself.
pub struct FontSlot {
    path: PathBuf,
    index: u32,
    font: OnceCell<Option<Font>>,
}

/// Holds canonical data for all paths pointing to the same entity.
#[derive(Default)]
pub struct PathSlot {
    source: OnceCell<FileResult<SourceId>>,
    buffer: OnceCell<FileResult<Buffer>>,
}

impl SystemWorld {
    pub fn new(root: PathBuf) -> Self {
        let mut searcher = FontSearcher::new();
        searcher.search_system();

        #[cfg(feature = "embed-fonts")]
        searcher.add_embedded();

        Self {
            root,
            library: Prehashed::new(typst_library::build()),
            book: Prehashed::new(searcher.book),
            fonts: searcher.fonts,
            hashes: RefCell::default(),
            paths: RefCell::default(),
            sources: FrozenVec::new(),
            main: SourceId::detached(),
        }
    }
}

impl World for SystemWorld {
    fn root(&self) -> &Path {
        &self.root
    }

    fn library(&self) -> &Prehashed<Library> {
        &self.library
    }

    fn main(&self) -> &Source {
        self.source(self.main)
    }

    fn resolve(&self, path: &Path) -> FileResult<SourceId> {
        self.slot(path)?
            .source
            .get_or_init(|| {
                let buf = read(path)?;
                let text = String::from_utf8(buf)?;
                Ok(self.insert(path, text))
            })
            .clone()
    }

    fn source(&self, id: SourceId) -> &Source {
        &self.sources[id.into_u16() as usize]
    }

    fn book(&self) -> &Prehashed<FontBook> {
        &self.book
    }

    fn font(&self, id: usize) -> Option<Font> {
        let slot = &self.fonts[id];
        slot.font
            .get_or_init(|| {
                let data = self.file(&slot.path).ok()?;
                Font::new(data, slot.index)
            })
            .clone()
    }

    fn file(&self, path: &Path) -> FileResult<Buffer> {
        self.slot(path)?
            .buffer
            .get_or_init(|| read(path).map(Buffer::from))
            .clone()
    }
}

impl SystemWorld {
    pub fn slot(&self, path: &Path) -> FileResult<RefMut<PathSlot>> {
        let mut hashes = self.hashes.borrow_mut();
        let hash = match hashes.get(path).cloned() {
            Some(hash) => hash,
            None => {
                let hash = PathHash::new(path);
                if let Ok(canon) = path.canonicalize() {
                    hashes.insert(canon.normalize(), hash.clone());
                }
                hashes.insert(path.into(), hash.clone());
                hash
            }
        }?;

        Ok(std::cell::RefMut::map(self.paths.borrow_mut(), |paths| {
            paths.entry(hash).or_default()
        }))
    }

    pub fn insert(&self, path: &Path, text: String) -> SourceId {
        let id = SourceId::from_u16(self.sources.len() as u16);
        let source = Source::new(id, path, text);
        self.sources.push(Box::new(source));
        id
    }

    pub fn relevant(&mut self, event: &notify::Event) -> bool {
        match &event.kind {
            notify::EventKind::Any => {}
            notify::EventKind::Access(_) => return false,
            notify::EventKind::Create(_) => return true,
            notify::EventKind::Modify(kind) => match kind {
                notify::event::ModifyKind::Any => {}
                notify::event::ModifyKind::Data(_) => {}
                notify::event::ModifyKind::Metadata(_) => return false,
                notify::event::ModifyKind::Name(_) => return true,
                notify::event::ModifyKind::Other => return false,
            },
            notify::EventKind::Remove(_) => {}
            notify::EventKind::Other => return false,
        }

        event.paths.iter().any(|path| self.dependant(path))
    }

    pub fn dependant(&self, path: &Path) -> bool {
        self.hashes.borrow().contains_key(&path.normalize())
            || PathHash::new(path).map_or(false, |hash| self.paths.borrow().contains_key(&hash))
    }

    pub fn reset(&mut self) {
        self.sources.as_mut().clear();
        self.hashes.borrow_mut().clear();
        self.paths.borrow_mut().clear();
    }
}

/// A hash that is the same for all paths pointing to the same entity.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct PathHash(u128);

impl PathHash {
    fn new(path: &Path) -> FileResult<Self> {
        let f = |e| FileError::from_io(e, path);
        let handle = Handle::from_path(path).map_err(f)?;
        let mut state = SipHasher::new();
        handle.hash(&mut state);
        Ok(Self(state.finish128().as_u128()))
    }
}

fn read(path: &Path) -> FileResult<Vec<u8>> {
    let f = |e| FileError::from_io(e, path);
    let mut file = File::open(path).map_err(f)?;
    if file.metadata().map_err(f)?.is_file() {
        let mut data = vec![];
        file.read_to_end(&mut data).map_err(f)?;
        Ok(data)
    } else {
        Err(FileError::IsDirectory)
    }
}

/// Searches for fonts.
pub struct FontSearcher {
    book: FontBook,
    fonts: Vec<FontSlot>,
}

impl FontSearcher {
    /// Create a new, empty system searcher.
    fn new() -> Self {
        Self { book: FontBook::new(), fonts: vec![] }
    }

    /// Add fonts that are embedded in the binary.
    #[cfg(feature = "embed-fonts")]
    fn add_embedded(&mut self) {
        let mut add = |bytes: &'static [u8]| {
            let buffer = Buffer::from_static(bytes);
            for (i, font) in Font::iter(buffer).enumerate() {
                self.book.push(font.info().clone());
                self.fonts.push(FontSlot {
                    path: PathBuf::new(),
                    index: i as u32,
                    font: OnceCell::from(Some(font)),
                });
            }
        };

        // Embed default fonts.
        add(include_bytes!("../../assets/fonts/LinLibertine_R.ttf"));
        add(include_bytes!("../../assets/fonts/LinLibertine_RB.ttf"));
        add(include_bytes!("../../assets/fonts/LinLibertine_RBI.ttf"));
        add(include_bytes!("../../assets/fonts/LinLibertine_RI.ttf"));
        add(include_bytes!("../../assets/fonts/NewCMMath-Book.otf"));
        add(include_bytes!("../../assets/fonts/NewCMMath-Regular.otf"));
        add(include_bytes!("../../assets/fonts/DejaVuSansMono.ttf"));
        add(include_bytes!("../../assets/fonts/DejaVuSansMono-Bold.ttf"));
    }

    /// Search for fonts in the linux system font directories.
    #[cfg(all(unix, not(target_os = "macos")))]
    fn search_system(&mut self) {
        self.search_dir("/usr/share/fonts");
        self.search_dir("/usr/local/share/fonts");

        if let Some(dir) = dirs::font_dir() {
            self.search_dir(dir);
        }
    }

    /// Search for fonts in the macOS system font directories.
    #[cfg(target_os = "macos")]
    fn search_system(&mut self) {
        self.search_dir("/Library/Fonts");
        self.search_dir("/Network/Library/Fonts");
        self.search_dir("/System/Library/Fonts");

        if let Some(dir) = dirs::font_dir() {
            self.search_dir(dir);
        }
    }

    /// Search for fonts in the Windows system font directories.
    #[cfg(windows)]
    fn search_system(&mut self) {
        let windir =
            std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());

        self.search_dir(Path::new(&windir).join("Fonts"));

        if let Some(roaming) = dirs::config_dir() {
            self.search_dir(roaming.join("Microsoft\\Windows\\Fonts"));
        }

        if let Some(local) = dirs::cache_dir() {
            self.search_dir(local.join("Microsoft\\Windows\\Fonts"));
        }
    }

    /// Search for all fonts in a directory recursively.
    fn search_dir(&mut self, path: impl AsRef<Path>) {
        for entry in WalkDir::new(path)
            .follow_links(true)
            .sort_by(|a, b| a.file_name().cmp(b.file_name()))
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("ttf" | "otf" | "TTF" | "OTF" | "ttc" | "otc" | "TTC" | "OTC"),
            ) {
                self.search_file(path);
            }
        }
    }

    /// Index the fonts in the file at the given path.
    fn search_file(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        if let Ok(file) = File::open(path) {
            if let Ok(mmap) = unsafe { Mmap::map(&file) } {
                for (i, info) in FontInfo::iter(&mmap).enumerate() {
                    self.book.push(info);
                    self.fonts.push(FontSlot {
                        path: path.into(),
                        index: i as u32,
                        font: OnceCell::new(),
                    });
                }
            }
        }
    }
}
