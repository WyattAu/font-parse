//! `maxp` — maximum profile: glyph count and memory-usage hints.

use crate::error::{ParseError, RawTag};
use crate::reader::be_u16_at;

pub(crate) const TAG: RawTag = *b"maxp";

/// Zero-copy view over the `maxp` table.
///
/// Version 0.5 (CFF fonts) carries only `numGlyphs` (6 bytes); version 1.0
/// (TrueType) adds the memory-hint fields (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaxpTable<'a> {
    data: &'a [u8],
    num_glyphs: u16,
}

impl<'a> MaxpTable<'a> {
    /// Parse and validate the `maxp` table bytes.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the table is shorter than its
    /// version's size (6 for 0.5, 32 for 1.0).
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 6 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 6,
                got: data.len(),
            });
        }
        let version = crate::reader::be_u32_at(data, 0, TAG)?;
        let min_len = match version {
            0x0000_5000 => 6,
            0x0001_0000 => 32,
            _ => {
                return Err(ParseError::InvalidSfnt);
            }
        };
        if data.len() < min_len {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: min_len,
                got: data.len(),
            });
        }
        Ok(Self {
            data,
            num_glyphs: be_u16_at(data, 4, TAG)?,
        })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Raw version field (`0x00005000` = 0.5, `0x00010000` = 1.0).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn version(&self) -> Result<u32, ParseError> {
        crate::reader::be_u32_at(self.data, 0, TAG)
    }

    /// The glyph count.
    #[must_use]
    pub const fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    /// `maxPoints` — version 1.0 only.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on a 0.5 table.
    pub fn max_points(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 6, TAG)
    }

    /// `maxContours` — version 1.0 only.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on a 0.5 table.
    pub fn max_contours(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 8, TAG)
    }

    /// `maxCompositePoints` — version 1.0 only.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on a 0.5 table.
    pub fn max_composite_points(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 10, TAG)
    }

    /// `maxCompositeContours` — version 1.0 only.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on a 0.5 table.
    pub fn max_composite_contours(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 12, TAG)
    }

    /// `maxComponentElements` — number of components in the most complex
    /// glyph; version 1.0 only.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on a 0.5 table.
    pub fn max_component_elements(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 28, TAG)
    }

    /// `maxComponentDepth` — nesting level of the most complex composite;
    /// version 1.0 only.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on a 0.5 table.
    pub fn max_component_depth(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 30, TAG)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::MaxpTable;
    use crate::error::ParseError;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    fn v1() -> [u8; 32] {
        let mut d = [0u8; 32];
        d[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        d[4..6].copy_from_slice(&42u16.to_be_bytes());
        d[6..8].copy_from_slice(&100u16.to_be_bytes());
        d[28..30].copy_from_slice(&4u16.to_be_bytes()); // maxComponentElements
        d[30..32].copy_from_slice(&2u16.to_be_bytes()); // maxComponentDepth
        d
    }

    #[test]
    fn v1_fields() {
        let d = v1();
        let m = MaxpTable::parse(&d).unwrap();
        assert_eq!(m.version().unwrap(), 0x0001_0000);
        assert_eq!(m.num_glyphs(), 42);
        assert_eq!(m.max_points().unwrap(), 100);
        assert_eq!(m.max_contours().unwrap(), 0);
        assert_eq!(m.max_composite_points().unwrap(), 0);
        assert_eq!(m.max_composite_contours().unwrap(), 0);
        assert_eq!(m.max_component_elements().unwrap(), 4);
        assert_eq!(m.max_component_depth().unwrap(), 2);
    }

    #[test]
    fn v0_5_table() {
        let mut d = [0u8; 6];
        d[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
        d[4..6].copy_from_slice(&7u16.to_be_bytes());
        let m = MaxpTable::parse(&d).unwrap();
        assert_eq!(m.num_glyphs(), 7);
        assert!(m.max_points().is_err()); // version 1.0 fields absent
    }

    #[test]
    fn errors() {
        // Truncated header.
        assert_eq!(
            MaxpTable::parse(&[0u8; 5]),
            Err(ParseError::TruncatedTable {
                tag: *b"maxp",
                expected: 6,
                got: 5
            })
        );
        // Unknown version.
        let mut d = [0u8; 32];
        d[0..4].copy_from_slice(&0x0002_0000u32.to_be_bytes());
        assert_eq!(MaxpTable::parse(&d), Err(ParseError::InvalidSfnt));
        // v1.0 declared but truncated.
        let mut d = v1();
        d[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        assert_eq!(
            MaxpTable::parse(&d[..20]),
            Err(ParseError::TruncatedTable {
                tag: *b"maxp",
                expected: 32,
                got: 20
            })
        );
    }
}
