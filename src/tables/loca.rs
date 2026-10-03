//! `loca` — index to location: glyph byte ranges within `glyf`.

use crate::error::{ParseError, RawTag};

pub(crate) const TAG: RawTag = *b"loca";

/// Zero-copy view over the `loca` table.
///
/// Short format (`indexToLocFormat == 0`): `numGlyphs + 1` `u16` entries,
/// byte offsets divided by two. Long format (`== 1`): raw `u32` offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocaTable<'a> {
    data: &'a [u8],
    num_glyphs: u16,
    long: bool,
}

impl<'a> LocaTable<'a> {
    /// Parse and validate the `loca` table bytes.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the table cannot hold
    /// `numGlyphs + 1` entries in the declared format.
    pub fn parse(data: &'a [u8], num_glyphs: u16, long: bool) -> Result<Self, ParseError> {
        let count = usize::from(num_glyphs)
            .checked_add(1)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        let expected = count
            .checked_mul(if long { 4 } else { 2 })
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if data.len() < expected {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected,
                got: data.len(),
            });
        }
        Ok(Self {
            data,
            num_glyphs,
            long,
        })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Byte range of glyph `gid` within `glyf`; `None` marks an empty glyph
    /// (equal start/end offsets) or an id beyond `numGlyphs`.
    ///
    /// # Errors
    /// [`ParseError::InvalidGlyphFormat`] when offsets descend (glyph data
    /// would read backwards) or `gid` maps past the table.
    pub fn glyph_range(&self, gid: u16) -> Result<Option<core::ops::Range<usize>>, ParseError> {
        if gid >= self.num_glyphs {
            return Ok(None);
        }
        let start = self.offset_at(gid)?;
        let end = self.offset_at(gid.checked_add(1).ok_or(ParseError::InvalidGlyphFormat)?)?;
        if end < start {
            return Err(ParseError::InvalidGlyphFormat);
        }
        if start == end {
            Ok(None)
        } else {
            Ok(Some(start..end))
        }
    }

    fn offset_at(&self, i: u16) -> Result<usize, ParseError> {
        if self.long {
            let at = usize::from(i) * 4;
            let v = crate::reader::be_u32_at(self.data, at, TAG)?;
            usize::try_from(v).map_err(|_| ParseError::InvalidGlyphFormat)
        } else {
            let at = usize::from(i) * 2;
            let v = crate::reader::be_u16_at(self.data, at, TAG)?;
            Ok(usize::from(v) * 2)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::LocaTable;
    use crate::error::ParseError;
    use alloc::vec::Vec;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    #[test]
    fn short_format() {
        // Offsets stored divided by two: 0, 40/2, 40/2, 80/2.
        let mut d = Vec::new();
        for v in [0u16, 20, 20, 40, 40] {
            d.extend_from_slice(&v.to_be_bytes());
        }
        let l = LocaTable::parse(&d, 4, false).unwrap();
        assert_eq!(l.glyph_range(0).unwrap(), Some(0..40));
        assert_eq!(l.glyph_range(1).unwrap(), None); // empty glyph
        assert_eq!(l.glyph_range(2).unwrap(), Some(40..80));
        assert_eq!(l.glyph_range(3).unwrap(), None);
        assert_eq!(l.glyph_range(4).unwrap(), None); // beyond numGlyphs
    }

    #[test]
    fn long_format() {
        let mut d = Vec::new();
        for v in [0u32, 100, 250] {
            d.extend_from_slice(&v.to_be_bytes());
        }
        let l = LocaTable::parse(&d, 2, true).unwrap();
        assert_eq!(l.glyph_range(0).unwrap(), Some(0..100));
        assert_eq!(l.glyph_range(1).unwrap(), Some(100..250));
        // Descending offsets are rejected.
        let bad = [0u32, 50, 40];
        let bad_bytes: Vec<u8> = bad.iter().flat_map(|v| v.to_be_bytes()).collect();
        let l = LocaTable::parse(&bad_bytes, 2, true).unwrap();
        assert_eq!(l.glyph_range(1), Err(ParseError::InvalidGlyphFormat));
    }

    #[test]
    fn truncated() {
        let d = [0u8; 9];
        assert_eq!(
            LocaTable::parse(&d, 4, false),
            Err(ParseError::TruncatedTable {
                tag: *b"loca",
                expected: 10,
                got: 9
            })
        );
    }
}
