//! Fuzz target — the main font parse entry point plus the container
//! decoders. The lib's totality contract: `Err`-not-panic on hostile input.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // SFNT entry point, then every typed accessor on whatever parses.
    if let Ok(font) = font_parse::FontRef::new(data) {
        let _ = font.num_tables();
        let _ = font.num_glyphs();
        for tag in font_parse::TableTag::ALL {
            let _ = font.table(tag);
        }
        if let Ok(cmap) = font.cmap() {
            for c in ['\u{0}', 'A', '\u{4E2D}', '\u{10FFFF}'] {
                let _ = cmap.codepoint(c);
            }
        }
        for gid in [0u32, 1, 2, 3, 0xFFFF, u32::MAX] {
            let g = font_parse::GlyphId::new(gid);
            let _ = font.glyph_data(g);
            let _ = font.outline(g);
            let _ = font.advance_width(g);
            let _ = font.gvar_deltas(g);
        }
        if let Ok(vf) = font.variable() {
            let _ = vf.axes();
            let _ = vf.instances();
            if let Some(avar) = vf.avar() {
                let _ = avar.segment_maps();
            }
            if let Some(gvar) = vf.gvar() {
                let _ = gvar.shared_tuples();
            }
        }
        let _ = font.head();
        let _ = font.hmtx();
        let _ = font.name();
        let _ = font.os2();
        let _ = font.post();
        let _ = font.stat();
        let _ = font.dsig();
        let _ = font.layout(font_parse::LayoutKind::Gsub);
        let _ = font.layout(font_parse::LayoutKind::Gpos);
    }

    // Collections.
    if let Ok(coll) = font_parse::FontCollection::parse(data) {
        for i in 0..coll.len() {
            if let Ok(f) = coll.font(i) {
                let _ = f.num_glyphs();
            }
        }
    }
    let _ = font_parse::OwnedFont::from_collection(data, 0);

    // Containers (feature-gated; the fuzz crate enables all features).
    #[cfg(feature = "woff")]
    {
        if let Ok(font) = font_parse::OwnedFont::from_woff(data) {
            let _ = font.as_font_ref().map(|f| f.num_glyphs());
        }
    }
    #[cfg(feature = "woff2")]
    {
        if let Ok(font) = font_parse::OwnedFont::from_woff2(data) {
            let _ = font.as_font_ref().map(|f| f.num_glyphs());
        }
    }
});
