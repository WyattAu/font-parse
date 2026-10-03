//! `head` — font header: units per em, loca format, style flags.

use crate::error::{ParseError, RawTag};
use crate::reader::{be_i16_at, be_u16_at};
use font_types::{Fixed, Version16Dot16};

pub(crate) const TAG: RawTag = *b"head";

/// Minimum `head` table size (through `glyphDataFormat`).
const MIN_LEN: usize = 54;

/// Zero-copy view over the `head` table.
///
/// Field offsets (OpenType 1.9): version 0, fontRevision 4, checkSumAdjustment
/// 8, magicNumber 12, flags 16, unitsPerEm 18, created 20, modified 28,
/// xMin 36, yMin 38, xMax 40, yMax 42, macStyle 44, lowestRecPPEM 46,
/// fontDirectionHint 48, indexToLocFormat 50, glyphDataFormat 52.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadTable<'a> {
    data: &'a [u8],
}

impl<'a> HeadTable<'a> {
    /// Parse and validate the `head` table bytes.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the table is shorter than 54
    /// bytes; [`ParseError::InvalidUnitsPerEm`] when `unitsPerEm` is zero or
    /// above 16384; [`ParseError::InvalidGlyphFormat`] when
    /// `indexToLocFormat` is neither 0 (short) nor 1 (long).
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < MIN_LEN {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: MIN_LEN,
                got: data.len(),
            });
        }
        let upem = be_u16_at(data, 18, TAG)?;
        if upem == 0 || upem > 16_384 {
            return Err(ParseError::InvalidUnitsPerEm(upem));
        }
        match be_i16_at(data, 50, TAG)? {
            0 | 1 => {}
            _ => return Err(ParseError::InvalidGlyphFormat),
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
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn version(&self) -> Result<Version16Dot16, ParseError> {
        Ok(Version16Dot16::new(
            be_u16_at(self.data, 0, TAG)?,
            be_u16_at(self.data, 2, TAG)?,
        ))
    }

    /// Font revision set by the font manufacturer.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn font_revision(&self) -> Result<Fixed, ParseError> {
        Ok(Fixed::from_bits(self.i32_at(4)?))
    }

    /// Checksum adjustment; computed over the whole font with this field
    /// zeroed (`0xB1B0AFBA - sum`).
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn check_sum_adjustment(&self) -> Result<u32, ParseError> {
        crate::reader::be_u32_at(self.data, 8, TAG)
    }

    /// Magic number (`0x5F0F3CF5`).
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn magic_number(&self) -> Result<u32, ParseError> {
        crate::reader::be_u32_at(self.data, 12, TAG)
    }

    /// `head.flags` — font-wide behavior bits (bit 11 signals a
    /// losslessly-transformed font, e.g. after WOFF2 decoding).
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn flags(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 16, TAG)
    }

    /// Units per em: 16..=16384, validated nonzero.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn units_per_em(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 18, TAG)
    }

    /// International date (seconds since 1904-01-01) the font was created.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn created(&self) -> Result<i64, ParseError> {
        self.i64_at(20)
    }

    /// International date (seconds since 1904-01-01) the font was modified.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn modified(&self) -> Result<i64, ParseError> {
        self.i64_at(28)
    }

    /// Glyph bounding box: `xMin`.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn x_min(&self) -> Result<i16, ParseError> {
        self.i16_at(36)
    }

    /// Glyph bounding box: `yMin`.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn y_min(&self) -> Result<i16, ParseError> {
        self.i16_at(38)
    }

    /// Glyph bounding box: `xMax`.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn x_max(&self) -> Result<i16, ParseError> {
        self.i16_at(40)
    }

    /// Glyph bounding box: `yMax`.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn y_max(&self) -> Result<i16, ParseError> {
        self.i16_at(42)
    }

    /// `macStyle` — bold (bit 0), italic (bit 1) style flags.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn mac_style(&self) -> Result<u16, ParseError> {
        self.u16_at(44)
    }

    /// Smallest readable size in pixels.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn lowest_rec_ppem(&self) -> Result<u16, ParseError> {
        self.u16_at(46)
    }

    /// Deprecated font direction hint.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn font_direction_hint(&self) -> Result<i16, ParseError> {
        self.i16_at(48)
    }

    /// 0 = short (`u16`, offsets/2), 1 = long (`u32`) `loca` entries.
    /// Validated to be one of the two in [`parse`](Self::parse).
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn index_to_loc_format(&self) -> Result<i16, ParseError> {
        self.i16_at(50)
    }

    /// Always 0; reserved for future glyph data formats.
    ///
    /// # Errors
    /// Truncation (structurally impossible post-[`parse`](Self::parse)).
    pub fn glyph_data_format(&self) -> Result<i16, ParseError> {
        self.i16_at(52)
    }

    fn u16_at(&self, at: usize) -> Result<u16, ParseError> {
        be_u16_at(self.data, at, TAG)
    }

    fn i16_at(&self, at: usize) -> Result<i16, ParseError> {
        be_i16_at(self.data, at, TAG)
    }

    fn i32_at(&self, at: usize) -> Result<i32, ParseError> {
        let raw = self
            .data
            .get(at..at + 4)
            .ok_or(ParseError::TruncatedTable {
                tag: TAG,
                expected: at + 4,
                got: self.data.len(),
            })?;
        font_types::BigEndian::<i32>::from_slice(raw)
            .map(|b| b.get())
            .ok_or(ParseError::TruncatedTable {
                tag: TAG,
                expected: at + 4,
                got: self.data.len(),
            })
    }

    fn i64_at(&self, at: usize) -> Result<i64, ParseError> {
        let raw = self
            .data
            .get(at..at + 8)
            .ok_or(ParseError::TruncatedTable {
                tag: TAG,
                expected: at + 8,
                got: self.data.len(),
            })?;
        font_types::BigEndian::<i64>::from_slice(raw)
            .map(|b| b.get())
            .ok_or(ParseError::TruncatedTable {
                tag: TAG,
                expected: at + 8,
                got: self.data.len(),
            })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::HeadTable;
    use crate::error::ParseError;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    fn sample() -> [u8; 54] {
        let mut d = [0u8; 54];
        d[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        d[4..8].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // revision 1.0
        d[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        d[16..18].copy_from_slice(&0x080Bu16.to_be_bytes()); // flags
        d[18..20].copy_from_slice(&1000u16.to_be_bytes());
        d[20..28].copy_from_slice(&3725107200i64.to_be_bytes()); // 2012-01-01ish
        d[36..38].copy_from_slice(&(-10i16).to_be_bytes());
        d[38..40].copy_from_slice(&(-20i16).to_be_bytes());
        d[40..42].copy_from_slice(&800i16.to_be_bytes());
        d[42..44].copy_from_slice(&900i16.to_be_bytes());
        d[44..46].copy_from_slice(&1u16.to_be_bytes()); // italic
        d[46..48].copy_from_slice(&8u16.to_be_bytes());
        d[48..50].copy_from_slice(&2i16.to_be_bytes());
        d[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca
        d[52..54].copy_from_slice(&0i16.to_be_bytes());
        d
    }

    #[test]
    fn parses_fields() {
        let d = sample();
        let h = HeadTable::parse(&d).unwrap();
        assert_eq!(h.units_per_em().unwrap(), 1000);
        assert_eq!(h.magic_number().unwrap(), 0x5F0F_3CF5);
        assert_eq!(h.index_to_loc_format().unwrap(), 1);
        assert_eq!(h.mac_style().unwrap(), 1);
        assert_eq!(h.x_min().unwrap(), -10);
        assert_eq!(h.y_max().unwrap(), 900);
        assert_eq!(h.version().unwrap(), font_types::Version16Dot16::new(1, 0));
        assert!((h.font_revision().unwrap().to_f32() - 1.0).abs() < 1e-6);
        assert_eq!(h.lowest_rec_ppem().unwrap(), 8);
        assert_eq!(h.created().unwrap(), 3_725_107_200);
    }

    #[test]
    fn validation_errors() {
        // Too short.
        let d = [0u8; 53];
        assert_eq!(
            HeadTable::parse(&d),
            Err(ParseError::TruncatedTable {
                tag: *b"head",
                expected: 54,
                got: 53
            })
        );
        // Zero unitsPerEm.
        let mut d = sample();
        d[18..20].copy_from_slice(&0u16.to_be_bytes());
        assert_eq!(HeadTable::parse(&d), Err(ParseError::InvalidUnitsPerEm(0)));
        // Over 16384.
        let mut d = sample();
        d[18..20].copy_from_slice(&20000u16.to_be_bytes());
        assert_eq!(
            HeadTable::parse(&d),
            Err(ParseError::InvalidUnitsPerEm(20000))
        );
        // Bad loca format.
        let mut d = sample();
        d[50..52].copy_from_slice(&7i16.to_be_bytes());
        assert_eq!(HeadTable::parse(&d), Err(ParseError::InvalidGlyphFormat));
    }

    #[test]
    fn as_bytes_round_trip() {
        let d = sample();
        let h = HeadTable::parse(&d).unwrap();
        assert_eq!(h.as_bytes().len(), 54);
    }
}
