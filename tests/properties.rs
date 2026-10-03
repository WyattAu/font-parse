//! Totality property tests — arbitrary and mutated input produces `Err` or
//! `Ok`, never a panic (500 cases per property).

// Test harness: assertions legitimately panic; the parse paths under test
// (the lib target) remain lint-clean.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use font_parse::{FontCollection, FontRef, GlyphId, LayoutKind, OwnedFont, ParseError, TableTag};
use proptest::prelude::*;

const TTF: &[u8] = include_bytes!("fixtures/minimal.ttf");
const TTC: &[u8] = include_bytes!("fixtures/collection.ttc");
#[cfg(feature = "woff")]
const WOFF: &[u8] = include_bytes!("fixtures/minimal.woff");
#[cfg(feature = "woff2")]
const WOFF2: &[u8] = include_bytes!("fixtures/minimal.woff2");
const VF: &[u8] = include_bytes!("fixtures/variable.ttf");

/// Walk every parse surface of a font; must not panic.
fn exercise(font: &FontRef<'_>) {
    let _ = font.num_tables();
    let _ = font.num_glyphs();
    let _ = font.sfnt_version();
    let _ = font.is_cff();
    let _ = font.is_variable();
    let _ = font.table_directory();
    for tag in TableTag::ALL {
        let _ = font.table(tag);
    }
    let _ = font.head();
    let _ = font.hhea();
    let _ = font.hmtx();
    let _ = font.maxp();
    if let Ok(cmap) = font.cmap() {
        for c in ['A', '\u{0}', '\u{10FFFF}', '\u{1F600}'] {
            let _ = cmap.codepoint(c);
        }
        for i in 0..8 {
            let _ = cmap.subtable(i);
        }
    }
    let _ = font.name();
    let _ = font.os2();
    let _ = font.post();
    let _ = font.stat();
    let _ = font.dsig();
    let _ = font.layout(LayoutKind::Gsub);
    let _ = font.layout(LayoutKind::Gpos);
    if let Ok(vf) = font.variable() {
        let _ = vf.axes();
        let _ = vf.instances();
        if let Some(avar) = vf.avar() {
            let _ = avar.segment_maps();
            let _ = avar.axis_count();
        }
        if let Some(gvar) = vf.gvar() {
            let _ = gvar.shared_tuples();
            for gid in 0u16..6 {
                let _ = gvar.glyph_deltas(gid, 12);
            }
        }
    }
    for gid in [0u32, 1, 2, 3, 4, u32::MAX] {
        let g = GlyphId::new(gid);
        let _ = font.glyph_data(g);
        let _ = font.glyph(g);
        let _ = font.outline(g);
        let _ = font.advance_width(g);
        let _ = font.gvar_deltas(g);
    }
}

proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(500))]

    /// Arbitrary bytes into `FontRef::new`: total — Err or Ok, never panic.
    #[test]
    fn font_new_is_total_on_arbitrary_input(buf in proptest::collection::vec(any::<u8>(), 0..512)) {
        if let Ok(font) = FontRef::new(&buf) {
            exercise(&font);
        }
    }

    /// Arbitrary bytes into the collection parser: total.
    #[test]
    fn collection_is_total_on_arbitrary_input(buf in proptest::collection::vec(any::<u8>(), 0..512)) {
        if let Ok(coll) = FontCollection::parse(&buf) {
            for i in 0..coll.len() + 1 {
                if let Ok(f) = coll.font(i) {
                    exercise(&f);
                }
            }
        }
        let _ = FontCollection::fonts_in(&buf);
        let _ = OwnedFont::from_collection(&buf, 0);
    }

    /// Valid TTF corrupted at any single byte stays typed; undisturbed
    /// header fields keep their values.
    #[test]
    fn ttf_single_byte_corruption_is_typed(flip_byte in 0usize..TTF.len(), delta in any::<u8>()) {
        let mut buf = TTF.to_vec();
        buf[flip_byte] = buf[flip_byte].wrapping_add(delta);
        if let Ok(font) = FontRef::new(&buf) {
            exercise(&font);
            // The sfntVersion bytes are never reinterpreted.
            if flip_byte >= 4 {
                assert_eq!(font.sfnt_version(), 0x0001_0000);
            }
        }
    }

    /// Truncations of a valid TTF are always typed errors.
    #[test]
    fn ttf_truncations_are_typed(end in 0..TTF.len()) {
        match FontRef::new(&TTF[..end]) {
            Err(ParseError::TruncatedTable { expected, got, .. }) => {
                prop_assert!(got <= expected);
            }
            Ok(_) => prop_assert!(end >= TTF.len()),
            Err(_) => {}
        }
    }

    /// WOFF/WOFF2/variable-font corpora through the container decoders:
    /// total on arbitrary corruption.
    #[test]
    fn containers_are_total_on_corruption(flip_byte in 0usize..64, delta in any::<u8>()) {
        #[cfg(any(feature = "woff", feature = "woff2"))]
        let _ = &WOFF;
        #[cfg(feature = "woff2")]
        let _ = &WOFF2;
        for src in [
            #[cfg(feature = "woff")]
            WOFF,
            #[cfg(feature = "woff2")]
            WOFF2,
            VF,
            TTC,
        ] {
            let mut buf = src.to_vec();
            let idx = flip_byte % buf.len();
            buf[idx] = buf[idx].wrapping_add(delta);
            #[cfg(feature = "woff")]
            {
                let _ = OwnedFont::from_woff(&buf);
            }
            #[cfg(feature = "woff2")]
            {
                let _ = OwnedFont::from_woff2(&buf);
            }
            if let Ok(f) = FontRef::new(&buf) {
                exercise(&f);
            }
        }
    }

    /// Two-byte mutations anywhere in the variable font stay total.
    #[test]
    fn variable_font_mutations_are_total(
        a in 0usize..VF.len(),
        b in 0usize..VF.len(),
        delta in any::<u8>(),
    ) {
        let mut buf = VF.to_vec();
        buf[a] = buf[a].wrapping_add(delta);
        buf[b] = buf[b].wrapping_add(delta);
        if let Ok(font) = FontRef::new(&buf) {
            exercise(&font);
        }
    }

    /// The intact fixtures always parse and agree on the fixture invariants —
    /// guards against the corruption properties silently masking regressions.
    #[test]
    fn fixtures_still_parse(_unused in 0..1u8) {
        let font = FontRef::new(TTF).unwrap();
        prop_assert_eq!(font.num_glyphs(), 4);
        let cmap = font.cmap().unwrap();
        prop_assert_eq!(cmap.codepoint('A').map(GlyphId::to_u32), Some(1));
        let coll = FontCollection::parse(TTC).unwrap();
        prop_assert_eq!(coll.len(), 2);
        #[cfg(feature = "woff")]
        prop_assert!(OwnedFont::from_woff(WOFF).is_ok());
        #[cfg(feature = "woff2")]
        prop_assert!(OwnedFont::from_woff2(WOFF2).is_ok());
    }
}
