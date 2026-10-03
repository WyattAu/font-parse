//! Typed, exhaustive error taxonomy for every parser in the crate.
//!
//! All decoding is bounds-checked and total: every malformed, truncated, or
//! hostile input yields one of these variants — never a panic, never an
//! out-of-bounds read. The enum is deliberately **exhaustive** (no
//! `#[non_exhaustive]`): callers are expected to match every variant, and a
//! new variant is a semver-minor event by design.
//!
//! `TruncatedTable` conventions: `tag` is the table tag (or container
//! pseudo-tag, e.g. `*b"wOFF"`) that was being read, `expected` is the
//! required absolute end offset in the buffer being read, and `got` is the
//! number of bytes actually available. Where the exact tag is not known (e.g.
//! the SFNT offset table itself) the closest container tag is used.

use alloc::format;
use alloc::string::String;
use core::fmt;

/// A four-byte table tag, as raw bytes (used in [`ParseError::TruncatedTable`]).
pub type RawTag = [u8; 4];

/// The single crate-level error for all font parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// The SFNT offset table (or WOFF/WOFF2 header) is malformed: bad
    /// signature, impossible table count, or a directory entry whose
    /// offset/length pair is out of range.
    InvalidSfnt,
    /// A four-byte tag carried a value outside the known table set, where a
    /// known tag was required. Raw (unknown) table access is available via
    /// [`crate::FontRef::table_by_tag`] — this variant applies to typed
    /// lookups only.
    UnknownTableTag(RawTag),
    /// A structure declared (or a read required) more bytes than the buffer
    /// holds. Bounds are checked at every read; `tag` is the table (or
    /// container) being read, `expected` the required absolute end offset,
    /// `got` the bytes actually available.
    TruncatedTable {
        /// Tag of the table (or container pseudo-tag) being read.
        tag: RawTag,
        /// Required absolute end offset in the buffer.
        expected: usize,
        /// Bytes actually available.
        got: usize,
    },
    /// A `cmap` subtable carries a format this parser does not implement
    /// (formats 2, 8, 10, 13, 14) or a structurally invalid format number.
    /// Supported formats: 0, 4, 6, 12.
    InvalidCmapFormat(u16),
    /// A `glyf`/`loca`/`gvar` record is structurally invalid: bad coordinate
    /// runs, a composite component loop that exceeds the nesting cap, a loca
    /// entry descending, or a WOFF2 glyph substream inconsistency.
    InvalidGlyphFormat,
    /// A WOFF2 table directory entry declares a transformation version that
    /// is not defined for its tag (e.g. transformed `name`), so the entry
    /// cannot be decoded. Only glyf/loca (version 0) and hmtx (version 1)
    /// transforms are defined by WOFF2 2.0.
    UnsupportedWoff2Transform,
    /// A collection (TTC) index beyond the number of fonts declared by the
    /// `ttcf` header.
    CollectionIndexOutOfBounds {
        /// Index that was requested.
        index: usize,
        /// Number of fonts the collection declares.
        count: usize,
    },
    /// The `head` table's `unitsPerEm` is zero or above the OpenType maximum
    /// of 16384.
    InvalidUnitsPerEm(u16),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::InvalidSfnt => write!(f, "invalid SFNT/WOFF container"),
            ParseError::UnknownTableTag(tag) => {
                write!(
                    f,
                    "unknown table tag {:?}",
                    char::from_u32(u32::from_be_bytes(*tag)).unwrap_or(char::REPLACEMENT_CHARACTER)
                )
            }
            ParseError::TruncatedTable { tag, expected, got } => {
                let name = tag_display(tag);
                write!(
                    f,
                    "truncated table {name}: need {expected} bytes, have {got}"
                )
            }
            ParseError::InvalidCmapFormat(fmt_) => {
                write!(f, "unsupported cmap subtable format {fmt_}")
            }
            ParseError::InvalidGlyphFormat => write!(f, "invalid glyph/gvar record"),
            ParseError::UnsupportedWoff2Transform => {
                write!(f, "unsupported WOFF2 table transform")
            }
            ParseError::CollectionIndexOutOfBounds { index, count } => {
                write!(f, "collection index {index} out of bounds ({count} fonts)")
            }
            ParseError::InvalidUnitsPerEm(upem) => {
                write!(f, "invalid unitsPerEm {upem} (must be 1..=16384)")
            }
        }
    }
}

/// Render a raw tag as its closest printable form, for diagnostics.
pub(crate) fn tag_display(tag: &[u8; 4]) -> String {
    let printable: String = tag
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            }
        })
        .collect();
    format!("'{printable}'")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::ParseError;
    use alloc::string::ToString;

    // Test harness: assertions legitimately panic; the parse paths under test
    // (the lib target) remain lint-clean.

    #[test]
    fn display_is_informative_and_total() {
        // Every variant renders without panicking — the error type itself is
        // part of the never-panic surface.
        let cases = [
            ParseError::InvalidSfnt,
            ParseError::UnknownTableTag(*b"\x00\x01\x02\xff"),
            ParseError::TruncatedTable {
                tag: *b"head",
                expected: 54,
                got: 12,
            },
            ParseError::InvalidCmapFormat(14),
            ParseError::InvalidGlyphFormat,
            ParseError::UnsupportedWoff2Transform,
            ParseError::CollectionIndexOutOfBounds { index: 3, count: 2 },
            ParseError::InvalidUnitsPerEm(0),
        ];
        for e in cases {
            let s = e.to_string();
            assert!(!s.is_empty(), "{e:?} rendered empty");
        }
    }
}
