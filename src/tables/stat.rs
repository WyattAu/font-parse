//! `stat` — style attributes: design axes and axis-value tables.

use crate::error::{ParseError, RawTag};
use crate::reader::{be_i32_at, be_u16_at, be_u32_at};
use alloc::vec::Vec;
use font_types::{Fixed, Tag};

pub(crate) const TAG: RawTag = *b"stat";

/// One design axis record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatAxis {
    /// Axis tag.
    pub tag: Tag,
    /// `name` table id of the axis name.
    pub name_id: u16,
    /// Sorting order.
    pub ordering: u16,
}

/// A decoded axis-value table (formats 1-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxisValue {
    /// Format 1 — one value along one axis.
    Value1 {
        /// Axis this value belongs to.
        axis_index: u16,
        /// Flags (bit 0 = elidable).
        flags: u16,
        /// `name` id of the value name.
        value_name_id: u16,
        /// The value.
        value: Fixed,
    },
    /// Format 2 — a value with an inclusive range.
    Range {
        /// Axis this value belongs to.
        axis_index: u16,
        /// Flags.
        flags: u16,
        /// `name` id of the value name.
        value_name_id: u16,
        /// Range minimum.
        nominal_min: Fixed,
        /// The nominal value.
        nominal: Fixed,
        /// Range maximum.
        nominal_max: Fixed,
    },
    /// Format 3 — a value with a linked "style-on" partner.
    Linked {
        /// Axis this value belongs to.
        axis_index: u16,
        /// Flags.
        flags: u16,
        /// `name` id of the value name.
        value_name_id: u16,
        /// The value.
        value: Fixed,
        /// The linked (auto-generated style) value.
        linked_value: Fixed,
    },
    /// Format 4 — a named style across multiple axes.
    Style {
        /// Flags.
        flags: u16,
        /// `name` id of the style name.
        value_name_id: u16,
        /// (axis index, value) pairs.
        values: Vec<(u16, Fixed)>,
    },
}

impl AxisValue {
    /// The value's name id.
    #[must_use]
    pub const fn value_name_id(&self) -> u16 {
        match self {
            AxisValue::Value1 { value_name_id, .. }
            | AxisValue::Range { value_name_id, .. }
            | AxisValue::Linked { value_name_id, .. }
            | AxisValue::Style { value_name_id, .. } => *value_name_id,
        }
    }
}

/// Zero-copy view over the `stat` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatTable<'a> {
    data: &'a [u8],
}

impl<'a> StatTable<'a> {
    /// Parse and validate the `stat` header.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] for short headers;
    /// [`ParseError::InvalidGlyphFormat`] for inconsistent sizes.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 20 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 20,
                got: data.len(),
            });
        }
        match be_u16_at(data, 0, TAG)? {
            1 => {}
            _ => return Err(ParseError::InvalidGlyphFormat),
        }
        let design_axis_size = be_u16_at(data, 4, TAG)?;
        if design_axis_size < 8 {
            return Err(ParseError::InvalidGlyphFormat);
        }
        Ok(Self { data })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Version (1.0 or 1.1 — the minor field distinguishes them).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn version(&self) -> Result<(u16, u16), ParseError> {
        Ok((be_u16_at(self.data, 0, TAG)?, be_u16_at(self.data, 2, TAG)?))
    }

    /// Design axis records.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the array is short.
    pub fn design_axes(&self) -> Result<Vec<StatAxis>, ParseError> {
        let count = be_u16_at(self.data, 6, TAG)?;
        let offset = be_u32_at(self.data, 8, TAG)?;
        let size = usize::from(be_u16_at(self.data, 4, TAG)?);
        let base = usize::try_from(offset).map_err(|_| ParseError::InvalidGlyphFormat)?;
        let mut out = Vec::with_capacity(usize::from(count));
        for i in 0..usize::from(count) {
            let at = base + i * size;
            let raw = be_u32_at(self.data, at, TAG)?;
            out.push(StatAxis {
                tag: Tag::new(&raw.to_be_bytes()),
                name_id: be_u16_at(self.data, at + 4, TAG)?,
                ordering: be_u16_at(self.data, at + 6, TAG)?,
            });
        }
        Ok(out)
    }

    /// Axis-value tables.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] / [`ParseError::InvalidGlyphFormat`]
    /// for malformed value tables.
    pub fn axis_values(&self) -> Result<Vec<AxisValue>, ParseError> {
        let count = be_u16_at(self.data, 12, TAG)?;
        let offset = be_u32_at(self.data, 14, TAG)?;
        let base = usize::try_from(offset).map_err(|_| ParseError::InvalidGlyphFormat)?;
        let mut out = Vec::with_capacity(usize::from(count));
        for i in 0..usize::from(count) {
            let at = base + i * 2;
            let off = usize::from(be_u16_at(self.data, at, TAG)?);
            out.push(self.axis_value_at(base + off)?);
        }
        Ok(out)
    }

    fn axis_value_at(&self, at: usize) -> Result<AxisValue, ParseError> {
        // STAT values are 16.16 Fixed.
        let fixed_at = |off: usize| -> Result<Fixed, ParseError> {
            Ok(Fixed::from_bits(be_i32_at(self.data, off, TAG)?))
        };
        let format = be_u16_at(self.data, at, TAG)?;
        match format {
            1 => {
                let axis_index = be_u16_at(self.data, at + 2, TAG)?;
                let flags = be_u16_at(self.data, at + 4, TAG)?;
                let name = be_u16_at(self.data, at + 6, TAG)?;
                Ok(AxisValue::Value1 {
                    axis_index,
                    flags,
                    value_name_id: name,
                    value: fixed_at(at + 8)?,
                })
            }
            2 => {
                let axis_index = be_u16_at(self.data, at + 2, TAG)?;
                let flags = be_u16_at(self.data, at + 4, TAG)?;
                let name = be_u16_at(self.data, at + 6, TAG)?;
                Ok(AxisValue::Range {
                    axis_index,
                    flags,
                    value_name_id: name,
                    // Spec order: nominalValue, rangeMin, rangeMax.
                    nominal: fixed_at(at + 8)?,
                    nominal_min: fixed_at(at + 12)?,
                    nominal_max: fixed_at(at + 16)?,
                })
            }
            3 => {
                let axis_index = be_u16_at(self.data, at + 2, TAG)?;
                let flags = be_u16_at(self.data, at + 4, TAG)?;
                let name = be_u16_at(self.data, at + 6, TAG)?;
                Ok(AxisValue::Linked {
                    axis_index,
                    flags,
                    value_name_id: name,
                    value: fixed_at(at + 8)?,
                    linked_value: fixed_at(at + 12)?,
                })
            }
            4 => {
                let axis_count = be_u16_at(self.data, at + 2, TAG)?;
                let flags = be_u16_at(self.data, at + 4, TAG)?;
                let name = be_u16_at(self.data, at + 6, TAG)?;
                let mut values = Vec::with_capacity(usize::from(axis_count));
                for a in 0..usize::from(axis_count) {
                    let rec_at = at + 8 + a * 6;
                    values.push((be_u16_at(self.data, rec_at, TAG)?, fixed_at(rec_at + 2)?));
                }
                Ok(AxisValue::Style {
                    flags,
                    value_name_id: name,
                    values,
                })
            }
            _ => Err(ParseError::InvalidGlyphFormat),
        }
    }

    /// The elided fallback name id.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn elided_fallback_name_id(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 18, TAG)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::{AxisValue, StatTable};
    use crate::error::ParseError;
    use alloc::vec::Vec;
    use font_types::Fixed;

    fn sample() -> Vec<u8> {
        let mut d = Vec::new();
        // Header: version 1.0, designAxisSize 8, 2 axes, 4 axis values,
        // elidedFallbackNameID 2.
        let design_axis_offset = 20u32;
        let axis_values_offset = 60u32;
        d.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version 1.0
        d.extend_from_slice(&8u16.to_be_bytes()); // designAxisSize
        d.extend_from_slice(&2u16.to_be_bytes()); // designAxisCount
        d.extend_from_slice(&design_axis_offset.to_be_bytes());
        d.extend_from_slice(&4u16.to_be_bytes()); // axisValueCount
        d.extend_from_slice(&axis_values_offset.to_be_bytes());
        d.extend_from_slice(&2u16.to_be_bytes()); // elidedFallbackNameID
                                                  // Design axes @20 (header is exactly 20 bytes): (tag, nameID, ordering).
        d.extend_from_slice(b"wght");
        d.extend_from_slice(&256u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(b"wdth");
        d.extend_from_slice(&257u16.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes());
        // Axis-value offsets @60 (before @60? values at 60): 4 u16 offsets.
        // We're at 36; pad to 60.
        d.extend_from_slice(&[0u8; 24]);
        // Value offsets (4 entries) relative to the axis-values base (60),
        // then the four value tables at exactly those offsets.
        d.extend_from_slice(&8u16.to_be_bytes()); // -> 68
        d.extend_from_slice(&28u16.to_be_bytes()); // -> 88
        d.extend_from_slice(&52u16.to_be_bytes()); // -> 112
        d.extend_from_slice(&72u16.to_be_bytes()); // -> 132
                                                   // fmt1 (12 bytes) @68: format, axisIndex, flags, name, value(Fixed)
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&2u16.to_be_bytes());
        d.extend_from_slice(&262u16.to_be_bytes());
        d.extend_from_slice(&700i32.to_be_bytes()); // Fixed 700.0
                                                    // pad 80..88
        d.extend_from_slice(&[0u8; 8]);
        // fmt2 (20 bytes) @88: format, axisIndex, flags, name, nominal, min, max
        d.extend_from_slice(&2u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&2u16.to_be_bytes());
        d.extend_from_slice(&263u16.to_be_bytes());
        d.extend_from_slice(&400i32.to_be_bytes());
        d.extend_from_slice(&350i32.to_be_bytes());
        d.extend_from_slice(&450i32.to_be_bytes());
        // pad 108..112
        d.extend_from_slice(&[0u8; 4]);
        // fmt3 (16 bytes) @112: format, axisIndex, flags, name, value, linked
        d.extend_from_slice(&3u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&2u16.to_be_bytes());
        d.extend_from_slice(&264u16.to_be_bytes());
        d.extend_from_slice(&300i32.to_be_bytes());
        d.extend_from_slice(&400i32.to_be_bytes());
        // pad 128..132
        d.extend_from_slice(&[0u8; 4]);
        // fmt4 (8 + 6*2 = 20 bytes) @132: format, axisCount, flags, name,
        // then (axisIndex, value) records.
        d.extend_from_slice(&4u16.to_be_bytes());
        d.extend_from_slice(&2u16.to_be_bytes());
        d.extend_from_slice(&2u16.to_be_bytes());
        d.extend_from_slice(&265u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&500i32.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&75i32.to_be_bytes());
        d
    }

    #[test]
    fn header_and_axes() {
        let d = sample();
        let t = StatTable::parse(&d).unwrap();
        assert_eq!(t.version().unwrap(), (1, 0));
        assert_eq!(t.elided_fallback_name_id().unwrap(), 2);
        let axes = t.design_axes().unwrap();
        assert_eq!(axes.len(), 2);
        assert_eq!(axes[0].tag.to_be_bytes(), *b"wght");
        assert_eq!(axes[0].name_id, 256);
        assert_eq!(axes[0].ordering, 0);
        assert_eq!(axes[1].tag.to_be_bytes(), *b"wdth");
        assert_eq!(axes[1].ordering, 1);
    }

    #[test]
    fn axis_value_formats() {
        let d = sample();
        let t = StatTable::parse(&d).unwrap();
        let values = t.axis_values().unwrap();
        assert_eq!(values.len(), 4);
        assert_eq!(
            values[0],
            AxisValue::Value1 {
                axis_index: 0,
                flags: 2,
                value_name_id: 262,
                value: Fixed::from_bits(700)
            }
        );
        match &values[1] {
            AxisValue::Range {
                axis_index,
                nominal_min,
                nominal,
                nominal_max,
                ..
            } => {
                assert_eq!(*axis_index, 0);
                assert_eq!(*nominal, Fixed::from_bits(400));
                assert_eq!(*nominal_min, Fixed::from_bits(350));
                assert_eq!(*nominal_max, Fixed::from_bits(450));
            }
            other => panic!("expected range, got {other:?}"),
        }
        match &values[2] {
            AxisValue::Linked {
                value,
                linked_value,
                ..
            } => {
                assert_eq!(*value, Fixed::from_bits(300));
                assert_eq!(*linked_value, Fixed::from_bits(400));
            }
            other => panic!("expected linked, got {other:?}"),
        }
        match &values[3] {
            AxisValue::Style {
                value_name_id,
                values: vals,
                ..
            } => {
                assert_eq!(*value_name_id, 265);
                assert_eq!(vals.len(), 2);
                assert_eq!(vals[0].0, 0);
                assert_eq!(vals[0].1, Fixed::from_bits(500));
            }
            other => panic!("expected style, got {other:?}"),
        }
        // Every value exposes its name id.
        assert_eq!(values[0].value_name_id(), 262);
    }

    #[test]
    fn errors() {
        assert!(matches!(
            StatTable::parse(&[0u8; 19]),
            Err(ParseError::TruncatedTable { .. })
        ));
        let mut d = sample();
        d[0..2].copy_from_slice(&2u16.to_be_bytes()); // bad version
        assert_eq!(
            StatTable::parse(&d).unwrap_err(),
            ParseError::InvalidGlyphFormat
        );
        // designAxisSize below 8.
        let mut d = sample();
        d[4..6].copy_from_slice(&4u16.to_be_bytes());
        assert_eq!(
            StatTable::parse(&d).unwrap_err(),
            ParseError::InvalidGlyphFormat
        );
        // Unknown axis-value format.
        let mut d = sample();
        let fmt4_at = 60 + 72;
        d[fmt4_at..fmt4_at + 2].copy_from_slice(&9u16.to_be_bytes());
        let t = StatTable::parse(&d).unwrap();
        assert_eq!(t.axis_values().unwrap_err(), ParseError::InvalidGlyphFormat);
    }
}
