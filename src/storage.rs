//! Files on disk: data directories, the song / design library, the media store
//! and safe (non-destructive) writing.

use crate::model::{Design, Song};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

pub struct Paths {
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
}

impl Paths {
    pub fn songs(&self) -> PathBuf {
        self.data.join("songs")
    }
    pub fn designs(&self) -> PathBuf {
        self.data.join("designs")
    }
    pub fn media(&self) -> PathBuf {
        self.data.join("media")
    }
    pub fn bibles(&self) -> PathBuf {
        self.data.join("bibles")
    }
    pub fn backups(&self) -> PathBuf {
        self.data.join("backups")
    }
    pub fn bookmarks_file(&self) -> PathBuf {
        self.data.join("bible_bookmarks.json")
    }
    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }
    pub fn lang(&self) -> PathBuf {
        self.config.join("lang")
    }
    pub fn client_media(&self) -> PathBuf {
        self.cache.join("client-media")
    }
}

pub fn paths() -> &'static Paths {
    static PATHS: OnceLock<Paths> = OnceLock::new();
    PATHS.get_or_init(|| {
        // OH_PRESENTER_HOME keeps everything in one folder (portable installs, tests).
        let p = if let Ok(home) = std::env::var("OH_PRESENTER_HOME") {
            let home = PathBuf::from(home);
            Paths { data: home.join("data"), config: home.join("config"), cache: home.join("cache") }
        } else if let Some(d) = directories::ProjectDirs::from("cz", "oh", "oh-presenter-rs") {
            Paths { data: d.data_dir().to_path_buf(), config: d.config_dir().to_path_buf(), cache: d.cache_dir().to_path_buf() }
        } else {
            let home = PathBuf::from(".oh-presenter-rs");
            Paths { data: home.join("data"), config: home.join("config"), cache: home.join("cache") }
        };
        for dir in [p.songs(), p.designs(), p.media(), p.bibles(), p.backups(), p.lang(), p.client_media()] {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::create_dir_all(&p.config);
        p
    })
}

pub fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Writes through a temporary file and rename, so a crash never leaves a half
/// written file. With `backup`, an existing file is first copied into the
/// backups directory (timestamped), so nothing is ever lost by saving.
pub fn write_atomic(path: &Path, bytes: &[u8], backup: bool) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if backup && path.exists() {
        backup_file(path)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn backup_file(path: &Path) -> io::Result<()> {
    let dir = paths().backups();
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    std::fs::copy(path, dir.join(format!("{stamp}-{name}")))?;
    // keep the 20 newest backups of this file name
    let mut old: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().map(|n| n.to_string_lossy().ends_with(&format!("-{name}"))).unwrap_or(false))
        .collect();
    old.sort();
    while old.len() > 20 {
        let _ = std::fs::remove_file(old.remove(0));
    }
    Ok(())
}

/// `dir/name (2).ext`, `dir/name (3).ext`... — the first path that does not exist yet.
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..).map(|n| parent.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap()
}

/// File-name friendly version of a title.
pub fn safe_file_name(title: &str) -> String {
    let s: String = title.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    let s = s.trim().to_string();
    if s.is_empty() { "untitled".into() } else { s }
}

/// Makes sure `path` ends with `.ext`.
pub fn with_extension(path: PathBuf, ext: &str) -> PathBuf {
    if path.extension().map(|e| e.eq_ignore_ascii_case(ext)).unwrap_or(false) { path } else { path.with_extension(ext) }
}

// ---------------------------------------------------------------------------
// Media store: content addressed, so two different images never overwrite each other.

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Stores bytes in the media store and returns the media name.
pub fn store_media_bytes(bytes: &[u8], ext: &str) -> io::Result<String> {
    let ext = ext.to_ascii_lowercase();
    let name = format!("{:016x}.{}", fnv1a(bytes), if ext.is_empty() { "bin" } else { &ext });
    let path = paths().media().join(&name);
    if !path.exists() {
        write_atomic(&path, bytes, false)?;
    }
    Ok(name)
}

/// Stores a (possibly large) file in the media store without reading it into memory
/// at once (videos). Same naming as [`store_media_bytes`].
pub fn store_media_file_streamed(path: &Path) -> io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h: u64 = 0xcbf29ce484222325;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for b in &buf[..n] {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let name = format!("{h:016x}.{}", if ext.is_empty() { "bin" } else { &ext });
    let target = paths().media().join(&name);
    if !target.exists() {
        // copy next to it, then rename: a half-copied file never looks complete
        let tmp = target.with_extension(format!("{ext}.part"));
        std::fs::copy(path, &tmp)?;
        std::fs::rename(&tmp, &target)?;
    }
    Ok(name)
}

pub fn media_path(name: &str) -> PathBuf {
    paths().media().join(name)
}

// ---------------------------------------------------------------------------
// Library: every song and design is its own file named by id, so saving one
// can never replace another, even when they share a title.

pub struct Library {
    pub songs: Vec<Song>,
    pub designs: Vec<Design>,
}

fn load_dir<T: serde::de::DeserializeOwned>(dir: &Path) -> Vec<T> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                match std::fs::read(&p).map(|b| serde_json::from_slice::<T>(&b)) {
                    Ok(Ok(v)) => out.push(v),
                    Ok(Err(err)) => eprintln!("skipping {}: {err}", p.display()),
                    Err(err) => eprintln!("skipping {}: {err}", p.display()),
                }
            }
        }
    }
    out
}

fn id_file(dir: PathBuf, id: &str) -> PathBuf {
    dir.join(format!("{}.json", safe_file_name(id)))
}

impl Library {
    pub fn load() -> Self {
        let mut songs: Vec<Song> = load_dir(&paths().songs());
        songs.sort_by_key(|s| s.title.to_lowercase());
        let mut designs: Vec<Design> = load_dir(&paths().designs());
        designs.sort_by_key(|d| d.name.to_lowercase());
        Self { songs, designs }
    }

    pub fn song(&self, id: &str) -> Option<&Song> {
        self.songs.iter().find(|s| s.id == id)
    }

    pub fn design(&self, id: &str) -> Option<&Design> {
        self.designs.iter().find(|d| d.id == id)
    }

    /// Other songs with the same title (case-insensitive).
    pub fn title_clash(&self, song: &Song) -> bool {
        let t = song.title.trim().to_lowercase();
        self.songs.iter().any(|s| s.id != song.id && s.title.trim().to_lowercase() == t)
    }

    pub fn design_name_clash(&self, design: &Design) -> bool {
        let t = design.name.trim().to_lowercase();
        self.designs.iter().any(|d| d.id != design.id && d.name.trim().to_lowercase() == t)
    }

    pub fn save_song(&mut self, song: &Song) -> io::Result<()> {
        write_atomic(&id_file(paths().songs(), &song.id), &serde_json::to_vec_pretty(song).unwrap(), true)?;
        match self.songs.iter_mut().find(|s| s.id == song.id) {
            Some(s) => *s = song.clone(),
            None => self.songs.push(song.clone()),
        }
        self.songs.sort_by_key(|s| s.title.to_lowercase());
        Ok(())
    }

    pub fn delete_song(&mut self, id: &str) -> io::Result<()> {
        let path = id_file(paths().songs(), id);
        if path.exists() {
            backup_file(&path)?;
            std::fs::remove_file(path)?;
        }
        self.songs.retain(|s| s.id != id);
        Ok(())
    }

    pub fn save_design(&mut self, design: &Design) -> io::Result<()> {
        write_atomic(&id_file(paths().designs(), &design.id), &serde_json::to_vec_pretty(design).unwrap(), true)?;
        match self.designs.iter_mut().find(|d| d.id == design.id) {
            Some(d) => *d = design.clone(),
            None => self.designs.push(design.clone()),
        }
        self.designs.sort_by_key(|d| d.name.to_lowercase());
        Ok(())
    }

    pub fn delete_design(&mut self, id: &str) -> io::Result<()> {
        let path = id_file(paths().designs(), id);
        if path.exists() {
            backup_file(&path)?;
            std::fs::remove_file(path)?;
        }
        self.designs.retain(|d| d.id != id);
        Ok(())
    }

    /// Adds an imported song. A song with the same id but different content gets a
    /// new id instead of replacing the existing one. Returns false if an identical
    /// song already exists.
    pub fn import_song(&mut self, mut song: Song) -> io::Result<bool> {
        if let Some(existing) = self.song(&song.id) {
            if *existing == song {
                return Ok(false);
            }
            song.id = crate::model::make_id();
        }
        if song.id.is_empty() {
            song.id = crate::model::make_id();
        }
        self.save_song(&song)?;
        Ok(true)
    }

    pub fn import_design(&mut self, mut design: Design) -> io::Result<bool> {
        if let Some(existing) = self.design(&design.id) {
            if *existing == design {
                return Ok(false);
            }
            design.id = crate::model::make_id();
        }
        if design.id.is_empty() {
            design.id = crate::model::make_id();
        }
        self.save_design(&design)?;
        Ok(true)
    }
}

/// Lower-case, without diacritics and with collapsed whitespace — for searching.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        for l in c.to_lowercase() {
            out.push(strip_diacritic(l));
        }
    }
    out
}

fn strip_diacritic(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ą' => 'a',
        'č' | 'ç' | 'ć' => 'c',
        'ď' => 'd',
        'é' | 'è' | 'ê' | 'ë' | 'ě' | 'ē' | 'ę' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ľ' | 'ĺ' | 'ł' => 'l',
        'ň' | 'ñ' | 'ń' => 'n',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ő' => 'o',
        'ř' | 'ŕ' => 'r',
        'š' | 'ś' => 's',
        'ť' => 't',
        'ú' | 'ù' | 'û' | 'ü' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' => 'y',
        'ž' | 'ź' | 'ż' => 'z',
        'æ' => 'a',
        _ => c,
    }
}
