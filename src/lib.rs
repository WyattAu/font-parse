//! SFNT/OTF/TrueType/WOFF/WOFF2 binary font parser — offset tables,
//! character maps, glyph outlines, variable fonts.
//!
//! font-parse reads font files **in place**: a [`FontRef`] is a lifetime-
//! bound view over `&[u8]` — table access touches nothing, allocates
//! nothing, copies nothing. Typed table views ([`HeadTable`], [`Cmap`],
//! [`GlyphOutline`], …) read fields on demand from the borrowed bytes and
//! die with the borrow.
//!
//! # Totality
//!
//! Every parser is **total**: malformed, truncated, hostile input produces a
//! typed [`ParseError`] — never a panic, never an out-of-bounds read. Bounds
//! are checked at every field read (deny-level `indexing_slicing` /
//! `unwrap_used` lints enforce this), so fuzzing asserts `Err`-not-panic
//! (see `fuzz/`).
//!
//! # Containers
//!
//! | Container | Entry point | Feature |
//! |---|---|---|
//! | SFNT / TrueType / OTF | [`FontRef::new`] | — (no_std) |
//! | TTC collection | [`FontCollection::parse`], [`OwnedFont::from_collection`] | — |
//! | WOFF 1.0 (zlib) | [`OwnedFont::from_woff`] | `woff` (std) |
//! | WOFF 2.0 (Brotli + glyf/loca/hmtx transforms) | [`OwnedFont::from_woff2`] | `woff2` (std) |
//!
//! WOFF/WOFF2 decoding reconstructs a standalone SFNT ([`OwnedFont`]) with
//! recomputed table checksums and `head.checkSumAdjustment`.
//!
//! # Example
//!
//! ```no_run
//! use font_parse::{FontRef, TableTag, GlyphId};
//!
//! # fn main() -> Result<(), font_parse::ParseError> {
//! let bytes = std::fs::read("tests/fixtures/minimal.ttf").expect("fixture");
//! let font = FontRef::new(&bytes)?;
//!
//! // Zero-copy table access.
//! let head = font.head()?;
//! assert_eq!(head.units_per_em()?, 1000);
//!
//! // Character mapping.
//! let cmap = font.cmap()?;
//! let gid = cmap.codepoint('A').expect("A is mapped");
//!
//! // Metrics and outlines.
//! let advance = font.advance_width(gid)?;
//! if let Some(outline) = font.outline(gid)? {
//!     let _ = outline.point_count();
//! }
//! let _raw: Option<&[u8]> = font.table(TableTag::Glyf);
//! let _ = GlyphId::new(0);
//! # Ok(())
//! # }
//! ```
//!
//! # Out of scope
//!
//! CFF outline decoding (OTTO fonts parse; their `CFF` table is exposed raw),
//! rasterization, glyph instruction (hinting) interpretation, cmap lookup
//! subtable formats 2/8/10/13/14, GSUB/GPOS lookup subtable types, and WOFF2
//! collection directories. This crate is the estate's font edge: consumers
//! bring the buffers and take the glyphs.
//!
//! # Layer
//!
//! L0 — leaf. Zero estate-internal dependencies; external deps are
//! `font-types` (scalar types) + `thiserror`, with `miniz_oxide`/`brotli-
//! decompressor` behind the container features.

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
extern crate alloc;

mod error;
mod font;
mod reader;
pub mod tables;
mod tag;

pub use error::{ParseError, RawTag};
pub use font::{FontCollection, FontRef, OwnedFont, VariableFont};
pub use tables::{
    AvarTable, AxisValue, Cmap, CmapSubtable, Component, CompositeGlyph, Contour, DsigTable,
    EncodingRecord, Feature, FeatureList, FvarTable, GlyphOutline, GlyphRef, GvarTable, HeadTable,
    HheaTable, HmtxTable, LangSys, LayoutKind, LayoutTable, LocaTable, Lookup, LookupList,
    MaxpTable, NameRecord, NameTable, Os2Table, OutlinePoint, PostTable, Script, ScriptList,
    SimpleGlyph, StatAxis, StatTable, TupleVariation, VarInstance, VariationAxis,
};
pub use tag::TableTag;

// Scalar types re-exported from Google Fonts' `font-types` — the crate's
// numeric vocabulary (16.16 `Fixed`, 2.14 `F2Dot14`, `GlyphId`, `Tag`, …).
pub use font_types::{F2Dot14, Fixed, GlyphId, GlyphId16, Tag, Version16Dot16};

/// SFNT table checksum: u32 sum of big-endian words, zero-padded to four
/// bytes. Exposed for container verification and directory checks.
#[must_use]
pub fn table_checksum_public(bytes: &[u8]) -> u32 {
    font::table_checksum(bytes)
}

#[cfg(feature = "woff")]
mod woff;
#[cfg(feature = "woff2")]
mod woff2;
