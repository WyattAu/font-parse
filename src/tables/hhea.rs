//! `hhea` — horizontal header: line metrics and `numberOfHMetrics`.

use crate::error::{ParseError, RawTag};
use crate::reader::{be_i16_at, be_u16_at};

pub(crate) const TAG: RawTag = *b"hhea";

/// Minimum `hhea` table size (36 bytes, through `numberOfHMetrics`).
const MIN_LEN: usize = 36;

/// Zero-copy view over the `hhea` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HheaTable<'a> {
    data: &'a [u8],
}

impl<'a> HheaTable<'a> {
    /// Parse and validate the `hhea` table bytes.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the table is shorter than 36
    /// bytes.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < MIN_LEN {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: MIN_LEN,
                got: data.len(),
            });
        }
        Ok(Self { data })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Table version (typically 1.0).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn version(&self) -> Result<u32, ParseError> {
        crate::reader::be_u32_at(self.data, 0, TAG)
    }

    /// Typographic ascender, in font units.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn ascender(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 4, TAG)
    }

    /// Typographic descender (negative), in font units.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn descender(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 6, TAG)
    }

    /// Typographic line gap.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn line_gap(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 8, TAG)
    }

    /// Maximum advance width, in font units.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn advance_width_max(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 10, TAG)
    }

    /// Minimum left side bearing across glyphs.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn min_left_side_bearing(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 12, TAG)
    }

    /// Minimum right side bearing across glyphs.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn min_right_side_bearing(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 14, TAG)
    }

    /// `xMaxExtent` — `max(lsb + xMax)`.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn x_max_extent(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 16, TAG)
    }

    /// Caret slope rise (typically 1).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn caret_slope_rise(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 18, TAG)
    }

    /// Caret slope run (typically 0).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn caret_slope_run(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 20, TAG)
    }

    /// Caret offset.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn caret_offset(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 22, TAG)
    }

    /// Number of `hmtx` entries in the first (advance + lsb) array; the
    /// remaining glyphs reuse the last advance.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn number_of_h_metrics(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 34, TAG)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::HheaTable;
    use crate::error::ParseError;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    fn sample() -> [u8; 36] {
        let mut d = [0u8; 36];
        d[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        d[4..6].copy_from_slice(&800i16.to_be_bytes());
        d[6..8].copy_from_slice(&(-200i16).to_be_bytes());
        d[8..10].copy_from_slice(&50i16.to_be_bytes());
        d[10..12].copy_from_slice(&1000u16.to_be_bytes());
        d[34..36].copy_from_slice(&3u16.to_be_bytes());
        d
    }

    #[test]
    fn parses_fields() {
        let d = sample();
        let h = HheaTable::parse(&d).unwrap();
        assert_eq!(h.version().unwrap(), 0x0001_0000);
        assert_eq!(h.ascender().unwrap(), 800);
        assert_eq!(h.descender().unwrap(), -200);
        assert_eq!(h.line_gap().unwrap(), 50);
        assert_eq!(h.advance_width_max().unwrap(), 1000);
        assert_eq!(h.number_of_h_metrics().unwrap(), 3);
        assert_eq!(h.caret_slope_rise().unwrap(), 0);
        assert_eq!(h.caret_slope_run().unwrap(), 0);
        assert_eq!(h.caret_offset().unwrap(), 0);
        assert_eq!(h.min_left_side_bearing().unwrap(), 0);
        assert_eq!(h.min_right_side_bearing().unwrap(), 0);
        assert_eq!(h.x_max_extent().unwrap(), 0);
        assert_eq!(h.as_bytes().len(), 36);
    }

    #[test]
    fn truncated() {
        let d = [0u8; 35];
        assert_eq!(
            HheaTable::parse(&d),
            Err(ParseError::TruncatedTable {
                tag: *b"hhea",
                expected: 36,
                got: 35
            })
        );
    }
}
