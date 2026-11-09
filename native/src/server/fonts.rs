//! `/api/system-fonts`: the font families installed on this computer, for
//! Settings ▸ Terminal ▸ Font. The browser runs on this computer too, so it
//! can draw every family listed. Names and a few flags only.

use std::collections::HashMap;
use std::collections::btree_map::{BTreeMap, Entry};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use super::{no_store, read_allowed, Shared};

pub(super) fn routes() -> Router<Shared> {
    Router::new().route("/api/system-fonts", get(list))
}

async fn list(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if !read_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match tokio::task::spawn_blocking(system_fonts).await {
        Ok(fonts) => no_store(Json(fonts)),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// One installed family, as the Settings font list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SystemFont {
    /// The English family name, as CSS `font-family` takes it.
    family: String,
    /// Latin letters and digits have one width, as a terminal needs.
    mono: bool,
    /// Has Korean Hangul syllables.
    hangul: bool,
    /// Has Japanese kana.
    kana: bool,
    /// Has Chinese characters.
    han: bool,
    /// Has the Nerd Font icons that shell prompts use.
    icons: bool,
}

/// What a face's tables say about the glyphs it has.
#[derive(Debug, Default, PartialEq, Eq)]
struct Probe {
    /// Advance widths of `i`, `M`, and `0`; None for a missing glyph.
    latin: [Option<u16>; 3],
    hangul: bool,
    kana: bool,
    han: bool,
    icons: bool,
}

/// A face found on disk; `id` finds its data again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate<T> {
    family: String,
    weight: u16,
    italic: bool,
    /// The `post` table's isFixedPitch flag.
    fixed_pitch: bool,
    id: T,
}

/// Fonts the system hides from font menus start with a dot (macOS) or `#`.
fn listed(family: &str) -> bool {
    let name = family.trim();
    !name.is_empty() && !name.starts_with(['.', '#'])
}

/// `i`, `M`, and `0` have one width. A font without them (a symbols font)
/// counts as monospaced when its `post` table says so.
fn mono(latin: [Option<u16>; 3], fixed_pitch: bool) -> bool {
    match latin {
        [Some(i), Some(m), Some(zero)] => i > 0 && i == m && m == zero,
        [None, None, None] => fixed_pitch,
        _ => false,
    }
}

/// One face per family, the closest to upright weight 400, sorted by name
/// without case. Families that differ only in case are one family.
fn pick_faces<T>(faces: impl IntoIterator<Item = Candidate<T>>) -> Vec<Candidate<T>> {
    // Lower is better: upright first, then nearer 400; at one distance the
    // heavier face, as CSS looks at 500 before lighter weights.
    let rank = |face: &Candidate<T>| (face.italic, face.weight.abs_diff(400), face.weight < 400);
    let mut best: BTreeMap<String, Candidate<T>> = BTreeMap::new();
    for face in faces {
        match best.entry(face.family.to_lowercase()) {
            Entry::Vacant(slot) => {
                slot.insert(face);
            }
            Entry::Occupied(mut slot) => {
                if rank(&face) < rank(slot.get()) {
                    slot.insert(face);
                }
            }
        }
    }
    best.into_values().collect()
}

/// Reads widths and glyph coverage from face `index` of a font file.
fn probe(data: &[u8], index: u32) -> Option<Probe> {
    let face = ttf_parser::Face::parse(data, index).ok()?;
    let glyph = |c: char| face.glyph_index(c).filter(|id| id.0 != 0);
    let has = |c: char| glyph(c).is_some();
    Some(Probe {
        latin: ['i', 'M', '0'].map(|c| glyph(c).and_then(|id| face.glyph_hor_advance(id))),
        hangul: has('가'),
        kana: has('あ'),
        han: has('中'),
        // nf-fa-folder; Nerd Fonts and their symbols-only fonts have it.
        icons: has('\u{f07b}'),
    })
}

/// The installed families, sorted by name: the files CoreText knows, kept
/// to the families it offers. Font Book can turn a family off, and macOS
/// can download fonts later; fontdb's folder scan sees neither.
#[cfg(target_os = "macos")]
pub(super) fn system_fonts() -> Vec<SystemFont> {
    let mut db = fontdb::Database::new();
    for path in core_text::font_files() {
        // A file that went away or does not parse is skipped.
        let _ = db.load_font_file(path);
    }
    keep_offered(read_fonts(&db), &core_text::family_names())
}

/// The installed families, sorted by name: the system font folders, and
/// fontconfig's on Linux.
#[cfg(not(target_os = "macos"))]
pub(super) fn system_fonts() -> Vec<SystemFont> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    read_fonts(&db)
}

/// One entry per listed family in `db`, from its most regular face.
fn read_fonts(db: &fontdb::Database) -> Vec<SystemFont> {
    let faces: Vec<_> = db
        .faces()
        .filter_map(|face| {
            let (family, _) = face.families.first()?;
            listed(family).then(|| Candidate {
                family: family.trim().to_owned(),
                weight: face.weight.0,
                italic: face.style != fontdb::Style::Normal,
                fixed_pitch: face.monospaced,
                id: face.id,
            })
        })
        .collect();
    pick_faces(faces)
        .into_iter()
        .filter_map(|face| {
            let probe = db.with_face_data(face.id, probe)??;
            Some(SystemFont {
                mono: mono(probe.latin, face.fixed_pitch),
                family: face.family,
                hangul: probe.hangul,
                kana: probe.kana,
                han: probe.han,
                icons: probe.icons,
            })
        })
        .collect()
}

/// Keeps the families the OS offers to apps, matched without case. An
/// offered family with no readable file (macOS makes PingFang from a
/// private font) is listed without flags.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn keep_offered(fonts: Vec<SystemFont>, offered: &[String]) -> Vec<SystemFont> {
    let mut found: HashMap<String, SystemFont> = fonts.into_iter().map(|font| (font.family.to_lowercase(), font)).collect();
    let mut kept: BTreeMap<String, SystemFont> = BTreeMap::new();
    for name in offered.iter().map(|name| name.trim()).filter(|name| listed(name)) {
        let key = name.to_lowercase();
        if kept.contains_key(&key) {
            continue;
        }
        let font = found.remove(&key).unwrap_or_else(|| SystemFont {
            family: name.to_owned(),
            mono: false,
            hangul: false,
            kana: false,
            han: false,
            icons: false,
        });
        kept.insert(key, font);
    }
    kept.into_values().collect()
}

#[cfg(target_os = "macos")]
mod core_text {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use core_foundation::array::{CFArray, CFArrayRef};
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_foundation::url::CFURL;

    #[link(name = "CoreText", kind = "framework")]
    unsafe extern "C" {
        fn CTFontManagerCopyAvailableFontFamilyNames() -> CFArrayRef;
        fn CTFontManagerCopyAvailableFontURLs() -> CFArrayRef;
    }

    /// Wraps an array a CoreText Copy function returned; null is empty.
    fn owned<T: TCFType>(array: CFArrayRef) -> Option<CFArray<T>> {
        // SAFETY: Copy functions return a new reference the caller owns, and
        // both arrays above hold the element type the caller names.
        (!array.is_null()).then(|| unsafe { CFArray::wrap_under_create_rule(array) })
    }

    /// The families apps may use by name, as Font Book lists them.
    pub(super) fn family_names() -> Vec<String> {
        // SAFETY: no arguments; returns an owned CFArray of CFStrings or null.
        let names = owned::<CFString>(unsafe { CTFontManagerCopyAvailableFontFamilyNames() });
        names.map(|names| names.iter().map(|name| name.to_string()).collect()).unwrap_or_default()
    }

    /// The font files behind those families, each once.
    pub(super) fn font_files() -> BTreeSet<PathBuf> {
        // SAFETY: no arguments; returns an owned CFArray of CFURLs or null.
        let urls = owned::<CFURL>(unsafe { CTFontManagerCopyAvailableFontURLs() });
        urls.map(|urls| urls.iter().filter_map(|url| url.to_path()).collect()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::header;

    use super::*;
    use crate::server::tests::test_state;

    fn face(family: &str, weight: u16, italic: bool, id: u32) -> Candidate<u32> {
        Candidate { family: family.to_owned(), weight, italic, fixed_pitch: false, id }
    }

    #[test]
    fn hidden_system_families_are_not_listed() {
        assert!(listed("D2Coding"));
        assert!(listed("Apple SD Gothic Neo"));
        assert!(!listed(".SF NS Mono"));
        assert!(!listed(".AppleSystemUIFont"));
        assert!(!listed("#GungSeo"));
        assert!(!listed(""));
        assert!(!listed("  "));
    }

    #[test]
    fn mono_means_one_latin_width() {
        assert!(mono([Some(1200), Some(1200), Some(1200)], false), "D2Coding sets no fixed-pitch flag");
        assert!(!mono([Some(500), Some(1700), Some(1150)], true), "widths win over the flag");
        assert!(!mono([Some(0), Some(0), Some(0)], false), "empty glyphs are not text");
        assert!(mono([None, None, None], true), "a symbols font relies on its flag");
        assert!(!mono([None, None, None], false));
        assert!(!mono([Some(1200), None, Some(1200)], false));
    }

    #[test]
    fn each_family_keeps_its_regular_face_in_name_order() {
        let picked = pick_faces([
            face("Menlo", 700, false, 1),
            face("Menlo", 400, true, 2),
            face("Menlo", 400, false, 3),
            face("d2coding", 400, false, 4),
            face("D2Coding", 700, false, 5),
            face("Apple SD Gothic Neo", 500, false, 6),
            face("Apple SD Gothic Neo", 300, false, 7),
        ]);
        let ids: Vec<u32> = picked.iter().map(|face| face.id).collect();
        assert_eq!(ids, [6, 4, 3], "Apple…, d2coding, Menlo; at one distance 500 wins, as in CSS");
    }

    fn font(family: &str, mono: bool) -> SystemFont {
        SystemFont { family: family.to_owned(), mono, hangul: false, kana: false, han: false, icons: false }
    }

    #[test]
    fn only_families_the_system_offers_are_kept() {
        let found = vec![font("Athelas", false), font("D2Coding", true), font("Menlo", true)];
        let offered = ["menlo", "PingFang SC", "D2Coding", ".PingFang UI SC", "D2Coding"].map(String::from);
        assert_eq!(
            keep_offered(found, &offered),
            [font("D2Coding", true), font("Menlo", true), font("PingFang SC", false)],
            "Athelas is on disk but turned off; PingFang has no file of its own",
        );
    }

    #[test]
    fn a_file_that_is_not_a_font_has_no_probe() {
        assert_eq!(probe(b"not a font", 0), None);
        assert_eq!(probe(&[], 0), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_fonts_are_listed_with_their_glyphs() {
        let fonts = system_fonts();
        let find = |name: &str| fonts.iter().find(|font| font.family == name).unwrap_or_else(|| panic!("{name} is installed"));
        let menlo = find("Menlo");
        assert!(menlo.mono && !menlo.hangul);
        let gothic = find("Apple SD Gothic Neo");
        assert!(gothic.hangul && !gothic.mono);
        assert!(find("Hiragino Sans").kana);
        assert!(fonts.iter().any(|font| font.family == "PingFang SC"));
        assert!(fonts.iter().all(|font| listed(&font.family)));
        let keys: Vec<String> = fonts.iter().map(|font| font.family.to_lowercase()).collect();
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]), "sorted without case, no repeats");
    }

    #[tokio::test]
    async fn the_list_is_for_the_dashboard_page_only() {
        let state: Shared = Arc::new(test_state(&["http://127.0.0.1:4317"]));
        let mut ours = HeaderMap::new();
        ours.insert(header::HOST, "127.0.0.1:4317".parse().unwrap());
        assert_eq!(list(State(state.clone()), ours.clone()).await.status(), StatusCode::OK);

        let mut cross = ours.clone();
        cross.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(list(State(state.clone()), cross).await.status(), StatusCode::FORBIDDEN);

        let mut rebound = HeaderMap::new();
        rebound.insert(header::HOST, "evil.example:4317".parse().unwrap());
        assert_eq!(list(State(state), rebound).await.status(), StatusCode::FORBIDDEN);
    }
}
