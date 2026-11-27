//! What the shared modules (`model`, `bible`, `song_formats`) need of the program's
//! `storage`: text folding for searches. The web edition has no files: bibles are kept in
//! IndexedDB by the app, so the bible module's own file functions (`save`, `load`, …) fail
//! here and are not used.

use std::path::PathBuf;

/// Lowercase without diacritics ("Žalm" → "zalm"), for searching. The same as the
/// program's `storage::fold`.
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

/// No files in the browser (see the module documentation).
pub struct Paths;

impl Paths {
    pub fn bibles(&self) -> PathBuf {
        PathBuf::from("/nonexistent/bibles")
    }
    pub fn bookmarks_file(&self) -> PathBuf {
        PathBuf::from("/nonexistent/bookmarks.json")
    }
}

pub fn paths() -> Paths {
    Paths
}

pub fn write_atomic(_path: &std::path::Path, _bytes: &[u8], _sync: bool) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no files in the browser"))
}
