//! Bibles: import (Zefania, OSIS, OpenSong XML, OpenLP database), reference
//! parsing in Czech and English, full-text search, visit history and pins.

use crate::storage::{fold, paths, write_atomic};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Verse {
    pub number: u32,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chapter {
    pub number: u32,
    pub verses: Vec<Verse>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Book {
    /// Canonical book number (1 = Genesis ... 66 = Revelation).
    pub number: u32,
    pub name: String,
    #[serde(default)]
    pub abbrev: String,
    pub chapters: Vec<Chapter>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BibleMeta {
    pub id: String,
    pub name: String,
    pub abbrev: String,
    #[serde(default)]
    pub copyright: String,
    /// ISO 639-1 language code (`cs`, `en`, …) from the file or guessed from the text;
    /// empty = unknown. The word search index is made per language.
    #[serde(default)]
    pub language: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bible {
    pub meta: BibleMeta,
    pub books: Vec<Book>,
}

/// A verse found by a search.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub book: usize,
    pub chapter: usize,
    pub verse: usize,
}

// ---------------------------------------------------------------------------
// Book names

struct BookNames {
    osis: &'static str,
    en: &'static [&'static str],
    cs: &'static [&'static str],
}

macro_rules! books {
    ($( $osis:literal [$($en:literal),*] [$($cs:literal),*] ),* $(,)?) => {
        &[ $( BookNames { osis: $osis, en: &[$($en),*], cs: &[$($cs),*] } ),* ]
    };
}

/// Names and abbreviations of the 66 books; the first name is used for display.
static BOOKS: &[BookNames] = books![
    "Gen" ["Genesis", "Gen", "Gn"] ["Genesis", "1. Mojžíšova", "1 Mojžíšova", "1M", "Gn"],
    "Exod" ["Exodus", "Exod", "Ex"] ["Exodus", "2. Mojžíšova", "2 Mojžíšova", "2M", "Ex"],
    "Lev" ["Leviticus", "Lev", "Lv"] ["Leviticus", "3. Mojžíšova", "3 Mojžíšova", "3M", "Lv"],
    "Num" ["Numbers", "Num", "Nm"] ["Numeri", "4. Mojžíšova", "4 Mojžíšova", "4M", "Nu"],
    "Deut" ["Deuteronomy", "Deut", "Dt"] ["Deuteronomium", "5. Mojžíšova", "5 Mojžíšova", "5M", "Dt"],
    "Josh" ["Joshua", "Josh", "Jos"] ["Jozue", "Joz"],
    "Judg" ["Judges", "Judg", "Jdg"] ["Soudců", "Sd"],
    "Ruth" ["Ruth", "Rth"] ["Rút", "Rt"],
    "1Sam" ["1 Samuel", "1Sam", "1Sa"] ["1. Samuelova", "1 Samuelova", "1S", "1Sam"],
    "2Sam" ["2 Samuel", "2Sam", "2Sa"] ["2. Samuelova", "2 Samuelova", "2S", "2Sam"],
    "1Kgs" ["1 Kings", "1Kgs", "1Ki"] ["1. Královská", "1 Královská", "1Kr"],
    "2Kgs" ["2 Kings", "2Kgs", "2Ki"] ["2. Královská", "2 Královská", "2Kr"],
    "1Chr" ["1 Chronicles", "1Chr", "1Ch"] ["1. Paralipomenon", "1. Letopisů", "1Pa", "1Let"],
    "2Chr" ["2 Chronicles", "2Chr", "2Ch"] ["2. Paralipomenon", "2. Letopisů", "2Pa", "2Let"],
    "Ezra" ["Ezra", "Ezr"] ["Ezdráš", "Ezd"],
    "Neh" ["Nehemiah", "Neh"] ["Nehemjáš", "Nehemiáš", "Neh"],
    "Esth" ["Esther", "Esth", "Est"] ["Ester", "Est"],
    "Job" ["Job", "Jb"] ["Jób", "Jb"],
    "Ps" ["Psalms", "Psalm", "Ps", "Psa"] ["Žalmy", "Žalm", "Ž", "Žl"],
    "Prov" ["Proverbs", "Prov", "Pr"] ["Přísloví", "Př"],
    "Eccl" ["Ecclesiastes", "Eccl", "Ec", "Qoh"] ["Kazatel", "Kaz"],
    "Song" ["Song of Solomon", "Song of Songs", "Song", "SS"] ["Píseň písní", "Pís", "Pp"],
    "Isa" ["Isaiah", "Isa", "Is"] ["Izajáš", "Iz"],
    "Jer" ["Jeremiah", "Jer"] ["Jeremjáš", "Jeremiáš", "Jr"],
    "Lam" ["Lamentations", "Lam"] ["Pláč", "Pláč Jeremjášův", "Pl"],
    "Ezek" ["Ezekiel", "Ezek", "Eze"] ["Ezechiel", "Ez"],
    "Dan" ["Daniel", "Dan", "Dn"] ["Daniel", "Da", "Dan"],
    "Hos" ["Hosea", "Hos"] ["Ozeáš", "Oz"],
    "Joel" ["Joel", "Jl"] ["Jóel", "Jl"],
    "Amos" ["Amos", "Am"] ["Ámos", "Am"],
    "Obad" ["Obadiah", "Obad", "Ob"] ["Abdijáš", "Abd"],
    "Jonah" ["Jonah", "Jon"] ["Jonáš", "Jon"],
    "Mic" ["Micah", "Mic"] ["Micheáš", "Mi"],
    "Nah" ["Nahum", "Nah"] ["Nahum", "Na"],
    "Hab" ["Habakkuk", "Hab"] ["Abakuk", "Abk"],
    "Zeph" ["Zephaniah", "Zeph"] ["Sofonjáš", "Sofoniáš", "Sf"],
    "Hag" ["Haggai", "Hag"] ["Ageus", "Ag"],
    "Zech" ["Zechariah", "Zech"] ["Zacharjáš", "Zachariáš", "Za"],
    "Mal" ["Malachi", "Mal"] ["Malachiáš", "Mal"],
    "Matt" ["Matthew", "Matt", "Mt"] ["Matouš", "Mt"],
    "Mark" ["Mark", "Mk", "Mrk"] ["Marek", "Mk"],
    "Luke" ["Luke", "Lk"] ["Lukáš", "L", "Lk"],
    "John" ["John", "Jn", "Joh"] ["Jan", "J"],
    "Acts" ["Acts", "Act"] ["Skutky", "Skutky apoštolů", "Sk"],
    "Rom" ["Romans", "Rom", "Ro"] ["Římanům", "Ř", "Řím"],
    "1Cor" ["1 Corinthians", "1Cor", "1Co"] ["1. Korintským", "1 Korintským", "1K", "1Kor"],
    "2Cor" ["2 Corinthians", "2Cor", "2Co"] ["2. Korintským", "2 Korintským", "2K", "2Kor"],
    "Gal" ["Galatians", "Gal"] ["Galatským", "Ga", "Gal"],
    "Eph" ["Ephesians", "Eph"] ["Efezským", "Ef"],
    "Phil" ["Philippians", "Phil", "Php"] ["Filipským", "Fp", "Flp"],
    "Col" ["Colossians", "Col"] ["Koloským", "Ko", "Kol"],
    "1Thess" ["1 Thessalonians", "1Thess", "1Th"] ["1. Tesalonickým", "1 Tesalonickým", "1Te", "1Tes"],
    "2Thess" ["2 Thessalonians", "2Thess", "2Th"] ["2. Tesalonickým", "2 Tesalonickým", "2Te", "2Tes"],
    "1Tim" ["1 Timothy", "1Tim", "1Ti"] ["1. Timoteovi", "1 Timoteovi", "1Tm", "1Tim"],
    "2Tim" ["2 Timothy", "2Tim", "2Ti"] ["2. Timoteovi", "2 Timoteovi", "2Tm", "2Tim"],
    "Titus" ["Titus", "Tit"] ["Titovi", "Tt", "Tit"],
    "Phlm" ["Philemon", "Phlm", "Phm"] ["Filemonovi", "Fm", "Flm"],
    "Heb" ["Hebrews", "Heb"] ["Židům", "Žd", "Žid"],
    "Jas" ["James", "Jas", "Jm"] ["Jakubův", "Jakub", "Jk", "Jak"],
    "1Pet" ["1 Peter", "1Pet", "1Pe"] ["1. Petrův", "1 Petrův", "1Pt", "1Petr"],
    "2Pet" ["2 Peter", "2Pet", "2Pe"] ["2. Petrův", "2 Petrův", "2Pt", "2Petr"],
    "1John" ["1 John", "1Jn", "1Jo"] ["1. Janův", "1 Janův", "1J", "1Jan"],
    "2John" ["2 John", "2Jn", "2Jo"] ["2. Janův", "2 Janův", "2J", "2Jan"],
    "3John" ["3 John", "3Jn", "3Jo"] ["3. Janův", "3 Janův", "3J", "3Jan"],
    "Jude" ["Jude", "Jud"] ["Judův", "Juda", "Ju", "Jud"],
    "Rev" ["Revelation", "Rev", "Rv"] ["Zjevení", "Zjevení Janovo", "Zj"],
];

fn norm_book(s: &str) -> String {
    fold(s).chars().filter(|c| c.is_alphanumeric()).collect()
}

fn book_number_from_osis(osis: &str) -> Option<u32> {
    BOOKS.iter().position(|b| b.osis.eq_ignore_ascii_case(osis)).map(|i| i as u32 + 1)
}

/// Book name in the UI language for canonical book `number`.
pub fn book_display_name(number: u32, lang: &str) -> Option<&'static str> {
    let b = BOOKS.get(number.checked_sub(1)? as usize)?;
    Some(if lang == "cs" { b.cs[0] } else { b.en[0] })
}

// ---------------------------------------------------------------------------
// References

#[derive(Clone, Debug, PartialEq)]
pub struct VerseRange {
    pub book: usize,
    pub start: (u32, u32),
    /// verse 0 = to the end of the chapter
    pub end: (u32, u32),
}

impl Bible {
    pub fn new(meta: BibleMeta, mut books: Vec<Book>) -> Self {
        books.retain(|b| !b.chapters.is_empty());
        for b in &mut books {
            b.chapters.sort_by_key(|c| c.number);
            for c in &mut b.chapters {
                c.verses.sort_by_key(|v| v.number);
            }
        }
        let mut bible = Self { meta, books };
        bible.meta.language = normalize_language(&bible.meta.language);
        if bible.meta.language.is_empty() {
            bible.meta.language = bible.guess_language();
        }
        bible
    }

    /// Guesses the language from the most common short words of the text.
    pub fn guess_language(&self) -> String {
        guess_language(self.books.iter().flat_map(|b| &b.chapters).flat_map(|c| &c.verses).take(3000).map(|v| v.text.as_str()))
    }

    /// Finds a book by name or abbreviation (diacritics and case insensitive).
    pub fn find_book(&self, name: &str, lang: &str) -> Option<usize> {
        let q = norm_book(name);
        if q.is_empty() {
            return None;
        }
        let by_number = |n: u32| self.books.iter().position(|b| b.number == n);
        let tables: [fn(&BookNames) -> &'static [&'static str]; 2] = if lang == "cs" { [|b| b.cs, |b| b.en] } else { [|b| b.en, |b| b.cs] };
        // 1. exact match: the bible's own names, then UI language, then the other one
        if let Some(i) = self.books.iter().position(|b| norm_book(&b.name) == q || (!b.abbrev.is_empty() && norm_book(&b.abbrev) == q)) {
            return Some(i);
        }
        for names in tables {
            if let Some(n) = BOOKS.iter().position(|b| names(b).iter().any(|n| norm_book(n) == q))
                && let Some(i) = by_number(n as u32 + 1)
            {
                return Some(i);
            }
        }
        // 2. prefix of a full name
        if let Some(i) = self.books.iter().position(|b| norm_book(&b.name).starts_with(&q)) {
            return Some(i);
        }
        for names in tables {
            if let Some(n) = BOOKS.iter().position(|b| names(b).iter().any(|n| norm_book(n).starts_with(&q)))
                && let Some(i) = by_number(n as u32 + 1)
            {
                return Some(i);
            }
        }
        None
    }

    /// Parses references like `J 3,16-18`, `Jan 3:16`, `1 Kor 13`, `Gn 1:1-2:3`,
    /// `Ž 23,1.3.5`, `John 3:16,18; 4:1`. A `,` after the chapter (Czech style)
    /// makes `.` the verse list separator, a `:` makes it `,`.
    pub fn parse_reference(&self, input: &str, lang: &str) -> Option<Vec<VerseRange>> {
        let mut out = Vec::new();
        let mut book: Option<usize> = None;
        for segment in input.split(';') {
            let segment = segment.trim();
            if segment.is_empty() {
                continue;
            }
            let (book_part, nums) = split_book(segment);
            if !book_part.trim().is_empty() {
                book = Some(self.find_book(book_part, lang)?);
            }
            let book = book?;
            let nums: String = nums.chars().filter(|c| !c.is_whitespace()).collect();
            let nums = nums.replace(['–', '—'], "-");
            if nums.is_empty() {
                // whole book makes no sense to show, take chapter 1
                out.push(VerseRange { book, start: (1, 1), end: (1, 0) });
                continue;
            }
            let (chapter_str, rest, sep) = match nums.find([':', ',']) {
                Some(i) => (&nums[..i], Some(&nums[i + 1..]), nums.as_bytes()[i] as char),
                None => (nums.as_str(), None, ':'),
            };
            let list_sep = if sep == ',' { '.' } else { ',' };
            match rest {
                None => {
                    // `3` or `3-4` (chapters)
                    let (a, b) = chapter_str.split_once('-').unwrap_or((chapter_str, chapter_str));
                    let (a, b): (u32, u32) = (a.parse().ok()?, b.parse().ok()?);
                    out.push(VerseRange { book, start: (a, 1), end: (b, 0) });
                }
                Some(rest) => {
                    let chapter: u32 = chapter_str.parse().ok()?;
                    for item in rest.split(list_sep).filter(|s| !s.is_empty()) {
                        match item.split_once('-') {
                            None => {
                                let v: u32 = item.parse().ok()?;
                                out.push(VerseRange { book, start: (chapter, v), end: (chapter, v) });
                            }
                            Some((a, b)) => {
                                let a: u32 = a.parse().ok()?;
                                let end = match b.split_once([':', ',']) {
                                    Some((c, v)) => (c.parse().ok()?, v.parse().ok()?),
                                    None if b.is_empty() => (chapter, 0),
                                    None => (chapter, b.parse().ok()?),
                                };
                                out.push(VerseRange { book, start: (chapter, a), end });
                            }
                        }
                    }
                }
            }
        }
        if out.is_empty() { None } else { Some(out) }
    }

    pub fn verses_in(&self, range: &VerseRange) -> Vec<Hit> {
        let Some(book) = self.books.get(range.book) else { return vec![] };
        let mut out = Vec::new();
        for (ci, ch) in book.chapters.iter().enumerate() {
            if ch.number < range.start.0 || ch.number > range.end.0 {
                continue;
            }
            for (vi, v) in ch.verses.iter().enumerate() {
                let after_start = ch.number > range.start.0 || v.number >= range.start.1;
                let before_end = ch.number < range.end.0 || range.end.1 == 0 || v.number <= range.end.1;
                if after_start && before_end {
                    out.push(Hit { book: range.book, chapter: ci, verse: vi });
                }
            }
        }
        out
    }

    /// Every verse of the bible, in order.
    pub fn all_hits(&self) -> Vec<Hit> {
        let mut out = Vec::with_capacity(self.verse_count());
        for (bi, b) in self.books.iter().enumerate() {
            for (ci, c) in b.chapters.iter().enumerate() {
                for vi in 0..c.verses.len() {
                    out.push(Hit { book: bi, chapter: ci, verse: vi });
                }
            }
        }
        out
    }

    pub fn verse(&self, h: &Hit) -> Option<(&Book, &Chapter, &Verse)> {
        let b = self.books.get(h.book)?;
        let c = b.chapters.get(h.chapter)?;
        Some((b, c, c.verses.get(h.verse)?))
    }

    pub fn book_name(&self, book: usize, lang: &str) -> String {
        let b = &self.books[book];
        if b.name.is_empty() { book_display_name(b.number, lang).unwrap_or("?").to_string() } else { b.name.clone() }
    }

    /// Formats a list of verses as a compact reference, e.g. `Jan 3,16-18` (cs) or `John 3:16-18`.
    pub fn format_reference(&self, hits: &[Hit], lang: &str) -> String {
        let sep = if lang == "cs" { "," } else { ":" };
        let list = if lang == "cs" { "." } else { "," };
        let mut out = String::new();
        let mut i = 0;
        let mut last_book = usize::MAX;
        let mut last_chapter = usize::MAX;
        while i < hits.len() {
            // a run of consecutive verses
            let mut j = i;
            while j + 1 < hits.len()
                && hits[j + 1].book == hits[j].book
                && hits[j + 1].chapter == hits[j].chapter
                && hits[j + 1].verse == hits[j].verse + 1
            {
                j += 1;
            }
            let Some((_, c, v1)) = self.verse(&hits[i]) else { break };
            let v2 = self.verse(&hits[j]).map(|x| x.2.number).unwrap_or(v1.number);
            if hits[i].book != last_book {
                if !out.is_empty() {
                    out.push_str("; ");
                }
                out.push_str(&format!("{} {}{}", self.book_name(hits[i].book, lang), c.number, sep));
            } else if hits[i].chapter != last_chapter {
                out.push_str(&format!("; {}{}", c.number, sep));
            } else {
                out.push_str(list);
            }
            out.push_str(&v1.number.to_string());
            if v2 != v1.number {
                out.push_str(&format!("-{v2}"));
            }
            last_book = hits[i].book;
            last_chapter = hits[i].chapter;
            i = j + 1;
        }
        out
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = paths().bibles();
        write_atomic(&dir.join(format!("{}.json", self.meta.id)), &serde_json::to_vec(self).unwrap(), false)?;
        write_atomic(&dir.join(format!("{}.meta", self.meta.id)), &serde_json::to_vec(&self.meta).unwrap(), false)
    }

    pub fn load(id: &str) -> Option<Bible> {
        let bytes = std::fs::read(paths().bibles().join(format!("{id}.json"))).ok()?;
        let mut bible: Bible = serde_json::from_slice(&bytes).ok()?;
        if bible.meta.language.is_empty() {
            // bibles imported before languages were recorded
            bible.meta.language = bible.guess_language();
            if !bible.meta.language.is_empty() {
                let _ = bible.save();
            }
        }
        Some(bible)
    }

    pub fn delete(id: &str) {
        let _ = std::fs::remove_file(paths().bibles().join(format!("{id}.json")));
        let _ = std::fs::remove_file(paths().bibles().join(format!("{id}.meta")));
        SearchIndex::delete(id);
    }

    /// Changes the recorded language of a bible.
    pub fn set_language(id: &str, language: &str) -> std::io::Result<()> {
        let mut bible = Bible::load(id).ok_or_else(|| std::io::Error::other("bible not found"))?;
        bible.meta.language = language.to_string();
        bible.save()
    }

    pub fn verse_count(&self) -> usize {
        self.books.iter().flat_map(|b| &b.chapters).map(|c| c.verses.len()).sum()
    }
}

/// Splits `1 Kor 13:4` into (`1 Kor `, `13:4`): the book is the shortest prefix
/// containing a letter that is followed by a digit.
fn split_book(s: &str) -> (&str, &str) {
    let mut seen_letter = false;
    for (i, c) in s.char_indices() {
        if c.is_alphabetic() {
            seen_letter = true;
        } else if c.is_ascii_digit() && seen_letter {
            return (&s[..i], &s[i..]);
        }
    }
    if seen_letter { (s, "") } else { ("", s) }
}

pub fn list_bibles() -> Vec<BibleMeta> {
    let mut out: Vec<BibleMeta> = std::fs::read_dir(paths().bibles())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().and_then(|e| e.to_str()) == Some("meta"))
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
        .collect();
    out.sort_by(|a: &BibleMeta, b| a.name.cmp(&b.name));
    out
}

// ---------------------------------------------------------------------------
// Languages

/// Languages the program knows by name: (ISO 639-1, ISO 639-2 codes, English name, own name).
const LANGUAGES: &[(&str, &[&str], &str, &str)] = &[
    ("cs", &["ces", "cze", "czech", "cesky", "cestina"], "Czech", "Čeština"),
    ("sk", &["slk", "slo", "slovak", "slovensky", "slovencina"], "Slovak", "Slovenčina"),
    ("en", &["eng", "english"], "English", "English"),
    ("de", &["deu", "ger", "german", "deutsch"], "German", "Deutsch"),
    ("pl", &["pol", "polish", "polski"], "Polish", "Polski"),
    ("fr", &["fra", "fre", "french", "francais"], "French", "Français"),
    ("es", &["spa", "spanish", "espanol"], "Spanish", "Español"),
    ("it", &["ita", "italian", "italiano"], "Italian", "Italiano"),
    ("pt", &["por", "portuguese", "portugues"], "Portuguese", "Português"),
    ("nl", &["nld", "dut", "dutch", "nederlands"], "Dutch", "Nederlands"),
    ("hu", &["hun", "hungarian", "magyar"], "Hungarian", "Magyar"),
    ("ru", &["rus", "russian"], "Russian", "Русский"),
    ("uk", &["ukr", "ukrainian"], "Ukrainian", "Українська"),
    ("ro", &["ron", "rum", "romanian", "romana"], "Romanian", "Română"),
    ("la", &["lat", "latin", "latina"], "Latin", "Latina"),
    ("el", &["ell", "grc", "gre", "greek"], "Greek", "Ελληνικά"),
    ("he", &["heb", "hebrew"], "Hebrew", "עברית"),
];

/// Turns `CZE`, `ces`, `cs-CZ`, `Czech` … into an ISO 639-1 code (`cs`); unknown codes are kept lower-case.
pub fn normalize_language(code: &str) -> String {
    let c = fold(code.trim());
    let c = c.split(['-', '_']).next().unwrap_or("").trim().to_string();
    if c.is_empty() {
        return c;
    }
    LANGUAGES.iter().find(|(iso, other, ..)| *iso == c || other.contains(&c.as_str())).map(|(iso, ..)| iso.to_string()).unwrap_or(c)
}

/// Name of a language for the UI (in the language itself, plus the code).
pub fn language_name(code: &str) -> String {
    match LANGUAGES.iter().find(|(iso, ..)| *iso == code) {
        Some((_, _, _, own)) => (*own).to_string(),
        None if code.is_empty() => "?".into(),
        None => code.to_string(),
    }
}

/// Codes of the known languages, for choosing a bible's language by hand.
pub fn known_languages() -> Vec<&'static str> {
    LANGUAGES.iter().map(|(iso, ..)| *iso).collect()
}

/// Guesses the language of some text from letters typical for a language (ě ř ů, ł ą ę, ß, …)
/// and from frequent words that only that language has.
pub fn guess_language<'a>(texts: impl Iterator<Item = &'a str>) -> String {
    // (language, typical letters, typical words — folded, see [`fold`])
    const SIGNS: &[(&str, &str, &[&str])] = &[
        (
            "cs",
            "ěřů",
            &["se", "je", "jsem", "jest", "byl", "aby", "pak", "jeho", "nebo", "kteri", "ktery", "ktera", "jako", "kdyz", "neb", "take"],
        ),
        ("sk", "äôľĺŕ", &["sa", "som", "bol", "bolo", "bola", "ako", "alebo", "ktori", "ktory", "potom", "ked", "lebo", "tiez", "vsetci"]),
        ("pl", "łąęśżźń", &["sie", "nie", "jest", "przez", "jego", "ktory", "oraz", "tylko", "aby", "byl"]),
        ("en", "", &["the", "and", "of", "that", "shall", "unto", "he", "they", "his", "with", "for", "was", "lord"]),
        ("de", "ßöü", &["und", "die", "der", "das", "nicht", "ist", "ich", "mit", "sich", "dem", "auf", "sie"]),
        ("fr", "èêàçœë", &["et", "le", "les", "des", "que", "qui", "il", "ne", "pas", "vous", "dans", "est"]),
        ("es", "ñ¿¡", &["y", "el", "los", "las", "que", "su", "por", "con", "para", "del", "se"]),
        ("pt", "ãõ", &["e", "os", "as", "que", "nao", "do", "da", "seu", "para", "com"]),
        ("it", "", &["il", "che", "non", "per", "del", "della", "gli", "suo", "sono"]),
        ("nl", "", &["en", "de", "het", "van", "een", "hij", "zijn", "niet", "dat", "zij"]),
        ("hu", "őű", &["es", "az", "hogy", "nem", "egy", "meg", "ki", "volt"]),
        ("ro", "șțăâ", &["si", "nu", "lui", "care", "pe", "cu", "din"]),
        ("la", "", &["et", "in", "est", "non", "ad", "qui", "ut", "eius", "dominus", "cum", "quia", "super"]),
        ("ru", "ыэъ", &["и", "в", "не", "он", "что", "его", "как", "так", "было"]),
        ("uk", "іїєґ", &["і", "в", "не", "він", "що", "його", "як", "та", "було"]),
        ("el", "αβγδεζηθλμνξπρστφχψω", &[]),
        ("he", "אבגדהוזחטיכלמנסעפצקרשת", &[]),
    ];
    let mut counts: std::collections::HashMap<String, usize> = Default::default();
    let mut letters: std::collections::HashMap<char, usize> = Default::default();
    let mut total = 0usize;
    for t in texts {
        for c in t.chars() {
            if c.is_alphabetic() && !c.is_ascii() {
                *letters.entry(c.to_lowercase().next().unwrap_or(c)).or_default() += 1;
            }
        }
        for w in words(t) {
            total += 1;
            *counts.entry(w).or_default() += 1;
        }
    }
    let score = |(_, chars, list): &(&str, &str, &[&str])| {
        let w: usize = list.iter().map(|w| counts.get(*w).copied().unwrap_or(0)).sum();
        let c: usize = chars.chars().map(|c| letters.get(&c).copied().unwrap_or(0)).sum();
        w + 3 * c
    };
    SIGNS
        .iter()
        .map(|sign| (sign.0, score(sign)))
        .max_by_key(|(_, s)| *s)
        .filter(|(_, s)| total > 0 && *s * 25 >= total) // signs in at least ~4 % of the words
        .map(|(c, _)| c.to_string())
        .unwrap_or_default()
}

/// Words of a text for searching: folded (see [`fold`]) and split on anything that is
/// not a letter or digit.
pub fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(fold)
}

// ---------------------------------------------------------------------------
// Word search index

/// An inverted index of a bible's words: for every distinct (folded) word, the verses
/// that contain it. Saved next to the bible as `<id>.idx`. Made on demand, only for the
/// languages enabled in the settings.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchIndex {
    version: u32,
    /// Verse count of the bible when the index was made: a different count means it is stale.
    verse_count: usize,
    /// Sorted distinct words.
    words: Vec<String>,
    /// For each word, the numbers of the verses containing it (in bible order).
    postings: Vec<Vec<u32>>,
    /// Verse number -> (book, chapter, verse) indexes.
    #[serde(skip)]
    hits: Vec<Hit>,
}

const INDEX_VERSION: u32 = 1;

fn index_path(id: &str) -> std::path::PathBuf {
    paths().bibles().join(format!("{id}.idx"))
}

impl SearchIndex {
    pub fn build(bible: &Bible) -> Self {
        let mut map: std::collections::BTreeMap<String, Vec<u32>> = Default::default();
        let hits = bible.all_hits();
        for (n, h) in hits.iter().enumerate() {
            let Some((_, _, v)) = bible.verse(h) else { continue };
            for w in words(&v.text) {
                let list = map.entry(w).or_default();
                if list.last() != Some(&(n as u32)) {
                    list.push(n as u32);
                }
            }
        }
        let (words, postings) = map.into_iter().unzip();
        Self { version: INDEX_VERSION, verse_count: hits.len(), words, postings, hits }
    }

    /// Whether an index file for this bible exists (without loading it).
    pub fn exists(id: &str) -> bool {
        index_path(id).exists()
    }

    /// Loads the saved index; `None` if there is none or it does not fit the bible.
    pub fn load(bible: &Bible) -> Option<Self> {
        let mut idx: Self = serde_json::from_slice(&std::fs::read(index_path(&bible.meta.id)).ok()?).ok()?;
        let hits = bible.all_hits();
        if idx.version != INDEX_VERSION || idx.verse_count != hits.len() || idx.words.len() != idx.postings.len() {
            return None;
        }
        idx.hits = hits;
        Some(idx)
    }

    pub fn save(&self, id: &str) -> std::io::Result<()> {
        write_atomic(&index_path(id), &serde_json::to_vec(self).map_err(std::io::Error::other)?, false)
    }

    pub fn delete(id: &str) {
        let _ = std::fs::remove_file(index_path(id));
    }

    /// Verses that contain every word of the query. A query word also matches inside a
    /// longer word (`milova` finds `zamilovaný`), case and diacritics are ignored.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let mut result: Option<Vec<u32>> = None;
        for q in words(query) {
            // all verses with any indexed word containing q
            let mut verses: Vec<u32> = self
                .words
                .iter()
                .zip(&self.postings)
                .filter(|(w, _)| w.contains(q.as_str()))
                .flat_map(|(_, p)| p.iter().copied())
                .collect();
            verses.sort_unstable();
            verses.dedup();
            result = Some(match result {
                None => verses,
                Some(prev) => intersect(&prev, &verses),
            });
            if result.as_ref().is_some_and(Vec::is_empty) {
                break;
            }
        }
        result.unwrap_or_default().into_iter().take(limit).filter_map(|n| self.hits.get(n as usize).cloned()).collect()
    }
}

fn intersect(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

/// Bibles listed without a language (imported by an older version) get one guessed from
/// their text (and saved), so they can be grouped by language.
pub fn fill_missing_languages(list: &mut [BibleMeta]) {
    for m in list.iter_mut().filter(|m| m.language.is_empty()) {
        if let Some(b) = Bible::load(&m.id) {
            m.language = b.meta.language;
        }
    }
}

// ---------------------------------------------------------------------------
// Import

pub(crate) fn new_meta(name: &str, abbrev: &str, copyright: &str) -> BibleMeta {
    let abbrev = if abbrev.trim().is_empty() {
        name.split_whitespace().filter_map(|w| w.chars().next()).collect::<String>().to_uppercase()
    } else {
        abbrev.trim().to_string()
    };
    BibleMeta {
        id: crate::model::make_id(),
        name: name.trim().to_string(),
        abbrev,
        copyright: copyright.trim().to_string(),
        language: String::new(),
    }
}

fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Imports a bible file, detecting the format. `name` overrides the bible's own name.
pub fn import_file(path: &Path) -> Result<Bible, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "sqlite" || ext == "db" {
        return import_openlp_sqlite(path);
    }
    if ext == "txt" {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        return Ok(import_txt(&String::from_utf8_lossy(&bytes), &stem));
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(&text, opts).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    match root.tag_name().name() {
        "XMLBIBLE" => Ok(import_zefania(root, &stem)),
        "osis" => Ok(import_osis(root, &stem)),
        "bible" => Ok(import_opensong(root, &stem)),
        other => Err(format!("unknown bible format <{other}>")),
    }
}

fn child<'a>(n: roxmltree::Node<'a, 'a>, name: &str) -> Option<roxmltree::Node<'a, 'a>> {
    n.children().find(|c| c.is_element() && c.tag_name().name().eq_ignore_ascii_case(name))
}

fn elements<'a>(n: roxmltree::Node<'a, 'a>, name: &'a str) -> impl Iterator<Item = roxmltree::Node<'a, 'a>> + 'a {
    n.children().filter(move |c| c.is_element() && c.tag_name().name().eq_ignore_ascii_case(name))
}

/// Text of a node without notes / footnotes.
fn verse_text(n: roxmltree::Node) -> String {
    let mut s = String::new();
    fn walk(n: roxmltree::Node, s: &mut String) {
        for c in n.children() {
            if c.is_text() {
                s.push_str(c.text().unwrap_or(""));
            } else if c.is_element() {
                let name = c.tag_name().name().to_ascii_lowercase();
                if matches!(name.as_str(), "note" | "gram_note" | "xref" | "dl" | "media") {
                    continue;
                }
                if name == "br" {
                    s.push(' ');
                }
                walk(c, s);
            }
        }
    }
    walk(n, &mut s);
    clean(&s)
}

fn import_zefania(root: roxmltree::Node, stem: &str) -> Bible {
    let info = child(root, "INFORMATION");
    let info_text = |name: &str| info.and_then(|i| child(i, name)).and_then(|n| n.text()).unwrap_or("").to_string();
    let mut name = info_text("title");
    if name.trim().is_empty() {
        name = root.attribute("biblename").unwrap_or(stem).to_string();
    }
    let mut meta = new_meta(&name, &info_text("identifier"), &info_text("rights"));
    meta.language = info_text("language");
    let mut books = Vec::new();
    for (i, b) in elements(root, "BIBLEBOOK").enumerate() {
        let number = b.attribute("bnumber").and_then(|n| n.parse().ok()).unwrap_or(i as u32 + 1);
        let mut chapters = Vec::new();
        for (ci, c) in elements(b, "CHAPTER").enumerate() {
            let cn = c.attribute("cnumber").and_then(|n| n.parse().ok()).unwrap_or(ci as u32 + 1);
            let verses = elements(c, "VERS")
                .enumerate()
                .map(|(vi, v)| Verse {
                    number: v.attribute("vnumber").and_then(|n| n.parse().ok()).unwrap_or(vi as u32 + 1),
                    text: verse_text(v),
                })
                .collect();
            chapters.push(Chapter { number: cn, verses });
        }
        books.push(Book {
            number,
            name: b.attribute("bname").unwrap_or("").to_string(),
            abbrev: b.attribute("bsname").unwrap_or("").to_string(),
            chapters,
        });
    }
    Bible::new(meta, books)
}

fn import_opensong(root: roxmltree::Node, stem: &str) -> Bible {
    let meta = new_meta(stem, "", "");
    let mut books = Vec::new();
    for (i, b) in elements(root, "b").enumerate() {
        let mut chapters = Vec::new();
        for (ci, c) in elements(b, "c").enumerate() {
            let cn = c.attribute("n").and_then(|n| n.parse().ok()).unwrap_or(ci as u32 + 1);
            let verses = elements(c, "v")
                .enumerate()
                .map(|(vi, v)| Verse {
                    // "1-2" style combined verses keep their first number
                    number: v.attribute("n").and_then(|n| n.split('-').next().and_then(|x| x.trim().parse().ok())).unwrap_or(vi as u32 + 1),
                    text: verse_text(v),
                })
                .collect();
            chapters.push(Chapter { number: cn, verses });
        }
        books.push(Book { number: i as u32 + 1, name: b.attribute("n").unwrap_or("").to_string(), abbrev: String::new(), chapters });
    }
    Bible::new(meta, books)
}

fn import_osis(root: roxmltree::Node, stem: &str) -> Bible {
    let header_title = root
        .descendants()
        .find(|n| n.tag_name().name() == "work")
        .and_then(|w| child(w, "title"))
        .and_then(|t| t.text())
        .unwrap_or(stem)
        .to_string();
    let abbrev =
        root.descendants().find(|n| n.tag_name().name() == "osisText").and_then(|n| n.attribute("osisIDWork")).unwrap_or("").to_string();
    let rights = root.descendants().find(|n| n.tag_name().name() == "rights").and_then(|n| n.text()).unwrap_or("");
    let mut meta = new_meta(&header_title, &abbrev, rights);
    meta.language = root
        .descendants()
        .find(|n| n.tag_name().name() == "osisText")
        .and_then(|n| n.attributes().find(|a| a.name() == "lang").map(|a| a.value().to_string()))
        .or_else(|| root.descendants().find(|n| n.tag_name().name() == "language").and_then(|n| n.text()).map(String::from))
        .unwrap_or_default();

    // Verses are either containers (<verse osisID="Gen.1.1">text</verse>) or
    // milestones (<verse sID="Gen.1.1"/>text<verse eID="Gen.1.1"/>).
    struct State {
        books: Vec<Book>,
        current: Option<(String, u32, u32)>,
        text: String,
    }
    impl State {
        fn flush(&mut self) {
            if let Some((book, ch, v)) = self.current.take() {
                let text = clean(&std::mem::take(&mut self.text));
                let Some(number) = book_number_from_osis(&book) else { return };
                let bi = match self.books.iter().position(|b| b.number == number) {
                    Some(i) => i,
                    None => {
                        self.books.push(Book { number, name: String::new(), abbrev: String::new(), chapters: vec![] });
                        self.books.len() - 1
                    }
                };
                let b = &mut self.books[bi];
                let ci = match b.chapters.iter().position(|c| c.number == ch) {
                    Some(i) => i,
                    None => {
                        b.chapters.push(Chapter { number: ch, verses: vec![] });
                        b.chapters.len() - 1
                    }
                };
                if !text.is_empty() {
                    b.chapters[ci].verses.push(Verse { number: v, text });
                }
            }
        }
    }
    fn parse_id(id: &str) -> Option<(String, u32, u32)> {
        // "Gen.1.1" (a range "Gen.1.1-Gen.1.2" keeps the first)
        let id = id.split([' ', '-']).next()?;
        let mut parts = id.split('.');
        let book = parts.next()?.to_string();
        Some((book, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    }
    fn walk(n: roxmltree::Node, st: &mut State) {
        for c in n.children() {
            if c.is_text() {
                if st.current.is_some() {
                    st.text.push_str(c.text().unwrap_or(""));
                }
                continue;
            }
            if !c.is_element() {
                continue;
            }
            match c.tag_name().name() {
                "verse" => {
                    if let Some(sid) = c.attribute("sID") {
                        st.flush();
                        st.current = parse_id(sid);
                    } else if c.attribute("eID").is_some() {
                        st.flush();
                    } else if let Some(id) = c.attribute("osisID") {
                        st.flush();
                        st.current = parse_id(id);
                        walk(c, st);
                        st.flush();
                    }
                }
                "note" | "title" | "header" | "reference" if st.current.is_none() || c.tag_name().name() == "note" => {}
                "lb" | "l" => {
                    st.text.push(' ');
                    walk(c, st);
                }
                _ => walk(c, st),
            }
        }
    }
    let mut st = State { books: Vec::new(), current: None, text: String::new() };
    walk(root, &mut st);
    st.flush();
    Bible::new(meta, st.books)
}

/// Imports a bible from an OpenLP bible database (`*.sqlite`).
pub fn import_openlp_sqlite(path: &Path) -> Result<Bible, String> {
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    let meta_value = |key: &str| -> String {
        conn.query_row("SELECT value FROM metadata WHERE key = ?1", [key], |r| r.get::<_, Option<String>>(0))
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut name = meta_value("name");
    if name.is_empty() {
        name = stem;
    }
    let mut meta = new_meta(&name, "", &meta_value("copyright"));
    meta.language = meta_value("language");
    let mut books = Vec::new();
    let mut stmt = conn.prepare("SELECT id, book_reference_id, name FROM book ORDER BY book_reference_id").map_err(|e| e.to_string())?;
    let rows: Vec<(i64, i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0), r.get::<_, Option<String>>(2)?.unwrap_or_default())))
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();
    let mut vstmt =
        conn.prepare("SELECT chapter, verse, text FROM verse WHERE book_id = ?1 ORDER BY chapter, verse").map_err(|e| e.to_string())?;
    for (id, reference, bname) in rows {
        let mut chapters: Vec<Chapter> = Vec::new();
        let verses = vstmt
            .query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?.unwrap_or_default())))
            .map_err(|e| e.to_string())?;
        for (ch, v, text) in verses.flatten() {
            if chapters.last().map(|c| c.number) != Some(ch as u32) {
                chapters.push(Chapter { number: ch as u32, verses: vec![] });
            }
            chapters.last_mut().unwrap().verses.push(Verse { number: v as u32, text: clean(&text) });
        }
        books.push(Book { number: reference as u32, name: bname, abbrev: String::new(), chapters });
    }
    let bible = Bible::new(meta, books);
    if bible.books.is_empty() {
        return Err("no verses in database (OpenLP web bibles are not stored locally)".into());
    }
    Ok(bible)
}

// ---------------------------------------------------------------------------
// Plain text (best effort: any text file can be imported, verses are found where
// the lines look like them)

/// Canonical number of the book with this name or abbreviation (Czech or English).
fn book_number_from_name(name: &str) -> Option<u32> {
    let n = norm_book(name);
    if n.is_empty() {
        return None;
    }
    BOOKS.iter().position(|b| norm_book(b.osis) == n || b.en.iter().chain(b.cs.iter()).any(|x| norm_book(x) == n)).map(|i| i as u32 + 1)
}

/// `3:16` or `3,16` (also `3:16-17`, the first verse counts): chapter and verse.
fn chapter_verse(token: &str) -> Option<(u32, u32)> {
    let (c, v) = token.split_once([':', ','])?;
    let v = v.split('-').next()?;
    if c.is_empty() || v.is_empty() || !c.chars().all(|x| x.is_ascii_digit()) || !v.chars().all(|x| x.is_ascii_digit()) {
        return None;
    }
    Some((c.parse().ok()?, v.parse().ok()?))
}

/// A plain number, optionally followed by `.` or `)`: `12`, `12.`, `12)`.
fn plain_number(token: &str) -> Option<u32> {
    let t = token.trim_end_matches(['.', ')']);
    (!t.is_empty() && t.chars().all(|c| c.is_ascii_digit())).then(|| t.parse().ok()).flatten()
}

/// Reads a bible from plain text, the format [`Bible::to_txt`] writes:
///
/// ```text
/// # name: Bible 21
/// Genesis 1:1 In the beginning God created the heavens and the earth.
/// ```
///
/// Other texts are read as well as it goes: a line with only a book name starts that
/// book, `Chapter 3` / `Kapitola 3` / `Genesis 3` a chapter, `1:1 text` or `1 text` is
/// a verse, and any other line goes on with the last numbered verse (or, without one,
/// becomes the next verse).
pub fn import_txt(text: &str, stem: &str) -> Bible {
    struct State {
        books: Vec<Book>,
        book: Option<usize>,
        chapter: u32,
        /// The last verse was given a number (a following plain line continues it).
        numbered: bool,
        stem: String,
    }
    impl State {
        fn book(&mut self, name: &str) -> usize {
            let name = name.trim();
            if let Some(i) = self.books.iter().position(|b| b.name.eq_ignore_ascii_case(name)) {
                return i;
            }
            let number = book_number_from_name(name)
                .filter(|n| !self.books.iter().any(|b| b.number == *n))
                .unwrap_or(1000 + self.books.len() as u32);
            let name = if name.is_empty() { self.stem.clone() } else { name.to_string() };
            self.books.push(Book { number, name, abbrev: String::new(), chapters: vec![] });
            self.chapter = 1;
            self.books.len() - 1
        }
        fn current_book(&mut self) -> usize {
            match self.book {
                Some(b) => b,
                None => {
                    let b = self.book("");
                    self.book = Some(b);
                    b
                }
            }
        }
        fn chapter(&mut self) -> &mut Chapter {
            let b = self.current_book();
            let n = self.chapter;
            let chapters = &mut self.books[b].chapters;
            let i = match chapters.iter().position(|c| c.number == n) {
                Some(i) => i,
                None => {
                    chapters.push(Chapter { number: n, verses: vec![] });
                    chapters.len() - 1
                }
            };
            &mut chapters[i]
        }
        fn verse(&mut self, number: Option<u32>, text: &str) {
            let text = clean(text);
            if text.is_empty() {
                return;
            }
            let numbered = number.is_some();
            let ch = self.chapter();
            let number = number.unwrap_or_else(|| ch.verses.iter().map(|v| v.number).max().unwrap_or(0) + 1);
            ch.verses.push(Verse { number, text });
            self.numbered = numbered;
        }
        fn go_on(&mut self, text: &str) {
            let text = clean(text);
            if self.numbered
                && let Some(v) = self.chapter().verses.last_mut()
            {
                v.text.push(' ');
                v.text.push_str(&text);
                return;
            }
            self.verse(None, &text);
        }
    }
    let mut meta = new_meta(stem, "", "");
    let mut st = State { books: Vec::new(), book: None, chapter: 1, numbered: false, stem: stem.to_string() };
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            if let Some((key, value)) = comment.split_once(':') {
                let value = value.trim().to_string();
                match key.trim().to_lowercase().as_str() {
                    "name" if !value.is_empty() => meta.name = value,
                    "abbrev" if !value.is_empty() => meta.abbrev = value,
                    "language" => meta.language = value,
                    "copyright" => meta.copyright = value,
                    _ => {}
                }
            }
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        // "Genesis 1:1 text" / "1:1 text" (a book name has at most three words; this
        // keeps "had 1,000 sheep" in a sentence from being read as a reference)
        if let Some(i) = tokens.iter().take(4).position(|t| chapter_verse(t).is_some_and(|(c, v)| c > 0 && v > 0)) {
            let (c, v) = chapter_verse(tokens[i]).unwrap();
            if i > 0 {
                let b = st.book(&tokens[..i].join(" "));
                st.book = Some(b);
            }
            st.chapter = c;
            st.verse(Some(v), &tokens[i + 1..].join(" "));
            continue;
        }
        // a book name alone
        if book_number_from_name(line).is_some() {
            let b = st.book(line);
            st.book = Some(b);
            st.numbered = false;
            continue;
        }
        // "Chapter 3", "Kapitola 3", "Genesis 3", "3"
        if let Some(n) = tokens.last().and_then(|t| plain_number(t)) {
            let head = tokens[..tokens.len() - 1].join(" ");
            let word = fold(&head).trim_end_matches('.').to_string();
            if matches!(word.as_str(), "" | "chapter" | "kapitola" | "kap" | "ch") {
                st.chapter = n;
                st.numbered = false;
                continue;
            }
            if book_number_from_name(&head).is_some() {
                let b = st.book(&head);
                st.book = Some(b);
                st.chapter = n;
                st.numbered = false;
                continue;
            }
        }
        // "1 text": a verse number in the current chapter
        if tokens.len() > 1
            && let Some(v) = plain_number(tokens[0])
        {
            st.verse(Some(v), &tokens[1..].join(" "));
            continue;
        }
        st.go_on(line);
    }
    if meta.abbrev.trim().is_empty() {
        meta.abbrev = new_meta(&meta.name, "", "").abbrev;
    }
    Bible::new(meta, st.books)
}

impl Bible {
    /// The bible as plain text, one verse per line (`Genesis 1:1 text`), read back by
    /// [`import_txt`].
    pub fn to_txt(&self) -> String {
        let one_line = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut out = format!("# name: {}\n# abbrev: {}\n", one_line(&self.meta.name), one_line(&self.meta.abbrev));
        if !self.meta.language.is_empty() {
            out.push_str(&format!("# language: {}\n", self.meta.language));
        }
        if !self.meta.copyright.trim().is_empty() {
            out.push_str(&format!("# copyright: {}\n", one_line(&self.meta.copyright)));
        }
        for b in &self.books {
            let name = match one_line(&b.name) {
                n if !n.is_empty() => n,
                _ => book_display_name(b.number, &self.meta.language).map(String::from).unwrap_or_else(|| format!("{}", b.number)),
            };
            out.push('\n');
            for c in &b.chapters {
                for v in &c.verses {
                    out.push_str(&format!("{name} {}:{} {}\n", c.number, v.number, one_line(&v.text)));
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// History and pins

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    pub reference: String,
    pub bible: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Bookmarks {
    pub history: Vec<Bookmark>,
    pub pins: Vec<Bookmark>,
}

impl Bookmarks {
    pub fn load() -> Self {
        std::fs::read(paths().bookmarks_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = write_atomic(&paths().bookmarks_file(), &serde_json::to_vec_pretty(self).unwrap(), false);
    }

    pub fn visit(&mut self, b: Bookmark) {
        self.history.retain(|h| !(h.reference == b.reference && h.bible == b.bible));
        self.history.insert(0, b);
        self.history.truncate(50);
        self.save();
    }

    pub fn is_pinned(&self, reference: &str, bible: &str) -> bool {
        self.pins.iter().any(|p| p.reference == reference && p.bible == bible)
    }

    pub fn toggle_pin(&mut self, b: Bookmark) {
        if self.is_pinned(&b.reference, &b.bible) {
            self.pins.retain(|p| !(p.reference == b.reference && p.bible == b.bible));
        } else {
            self.pins.push(b);
        }
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Bible {
        let ch = |n, count: u32| Chapter {
            number: n,
            verses: (1..=count).map(|v| Verse { number: v, text: format!("text {n}:{v} Žalm") }).collect(),
        };
        Bible::new(
            new_meta("Test", "T", ""),
            vec![
                Book { number: 19, name: "Žalmy".into(), abbrev: String::new(), chapters: vec![ch(23, 6)] },
                Book { number: 43, name: String::new(), abbrev: String::new(), chapters: vec![ch(3, 36), ch(4, 54)] },
                Book { number: 46, name: String::new(), abbrev: String::new(), chapters: vec![ch(13, 13)] },
            ],
        )
    }

    #[test]
    fn czech_references() {
        let b = sample();
        let r = b.parse_reference("J 3,16-18", "cs").unwrap();
        assert_eq!(r, vec![VerseRange { book: 1, start: (3, 16), end: (3, 18) }]);
        assert_eq!(b.verses_in(&r[0]).len(), 3);
        let r = b.parse_reference("Ž 23,1.3.5", "cs").unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(b.parse_reference("zalm 23", "cs").unwrap()[0].book, 0);
        assert_eq!(b.parse_reference("1 Kor 13,4-7", "cs").unwrap()[0].book, 2);
        assert_eq!(b.parse_reference("1K 13", "cs").unwrap()[0].book, 2);
    }

    #[test]
    fn english_references() {
        let b = sample();
        let r = b.parse_reference("John 3:16,18; 4:1-2", "en").unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[2], VerseRange { book: 1, start: (4, 1), end: (4, 2) });
        let r = b.parse_reference("Jn 3:35-4:2", "en").unwrap();
        assert_eq!(b.verses_in(&r[0]).len(), 4);
        assert_eq!(b.parse_reference("1 Corinthians 13", "en").unwrap()[0].book, 2);
        assert!(b.parse_reference("Hello world", "en").is_none());
    }

    #[test]
    fn formatting() {
        let b = sample();
        let hits = b.verses_in(&b.parse_reference("J 3,16-18", "cs").unwrap()[0]);
        assert_eq!(b.format_reference(&hits, "cs"), "Jan 3,16-18");
        assert_eq!(b.format_reference(&hits, "en"), "John 3:16-18");
    }

    #[test]
    fn text_search() {
        let b = sample();
        let idx = SearchIndex::build(&b);
        assert_eq!(idx.search("zalm 3:16", 10).len(), 1);
        // all words must occur, in any order, also inside longer words, punctuation ignored
        assert_eq!(idx.search("ŽAL, 16 3", 10).len(), 1);
        assert_eq!(idx.search("alm", 1000).len(), b.verse_count());
        assert!(idx.search("zalm nothing", 10).is_empty());
        assert!(idx.search("  ,, ", 10).is_empty());
    }

    #[test]
    fn languages() {
        assert_eq!(normalize_language("CZE"), "cs");
        assert_eq!(normalize_language("ces"), "cs");
        assert_eq!(normalize_language("en-US"), "en");
        assert_eq!(normalize_language("Deutsch"), "de");
        assert_eq!(normalize_language(""), "");
        let cs = [
            "Na počátku stvořil Bůh nebe a zemi.",
            "Země pak byla nesličná a pustá, a tma byla nad propastí, a Duch Boží vznášel se nad vodami.",
        ];
        assert_eq!(guess_language(cs.into_iter()), "cs");
        let en = [
            "In the beginning God created the heaven and the earth.",
            "And the earth was without form, and void; and darkness was upon the face of the deep.",
        ];
        assert_eq!(guess_language(en.into_iter()), "en");
        assert_eq!(guess_language(["123 456"].into_iter()), "");
        let pl = [
            "Na początku stworzył Bóg niebo i ziemię.",
            "A ziemia była pustkowiem i chaosem; ciemność była nad otchłanią, a Duch Boży unosił się nad wodami.",
        ];
        assert_eq!(guess_language(pl.into_iter()), "pl");
        let de = ["Am Anfang schuf Gott Himmel und Erde.", "Und die Erde war wüst und leer, und es war finster auf der Tiefe."];
        assert_eq!(guess_language(de.into_iter()), "de");
        let sk =
            ["Na počiatku stvoril Boh nebo a zem.", "Zem bola pustá a prázdna, tma bola nad priepasťou a Duch Boží sa vznášal nad vodami."];
        assert_eq!(guess_language(sk.into_iter()), "sk");
    }
}
