//! `fvar`/`avar` — variable-font axis data and normalization maps.

use crate::error::{ParseError, RawTag};
use crate::reader::{be_i32_at, be_u16_at, be_u32_at};
use alloc::vec::Vec;
use font_types::{F2Dot14, Fixed, Tag};

pub(crate) const FVAR: RawTag = *b"fvar";
pub(crate) const AVAR: RawTag = *b"avar";

/// One design axis of a variable font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariationAxis {
    /// Axis tag (`wght`, `wdth`, `opsz`, custom).
    pub tag: Tag,
    /// Minimum coordinate (16.16).
    pub min: Fixed,
    /// Default coordinate (16.16).
    pub default: Fixed,
    /// Maximum coordinate (16.16).
    pub max: Fixed,
    /// Raw axis flags (bit 0 = hidden axis).
    pub flags: u16,
    /// `name` table id of the axis name.
    pub name_id: u16,
}

/// One named instance of a variable font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarInstance {
    /// `name` table id of the instance (style) name.
    pub name_id: u16,
    /// Raw instance flags.
    pub flags: u16,
    /// Coordinates per axis, in axis order (16.16).
    pub coordinates: Vec<Fixed>,
}

/// Zero-copy view over the `fvar` table.
#[derive(Debug, Clone, Copy)]
pub struct FvarTable<'a> {
    data: &'a [u8],
    axis_count: u16,
    axis_size: u16,
    instance_count: u16,
    instance_size: u16,
    data_offset: u16,
}

impl<'a> FvarTable<'a> {
    /// Parse and validate the `fvar` table.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the header is short;
    /// [`ParseError::InvalidGlyphFormat`] for inconsistent counts/sizes.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 16 {
            return Err(ParseError::TruncatedTable {
                tag: FVAR,
                expected: 16,
                got: data.len(),
            });
        }
        let axis_count = be_u16_at(data, 8, FVAR)?;
        let axis_size = be_u16_at(data, 10, FVAR)?;
        let instance_count = be_u16_at(data, 12, FVAR)?;
        let instance_size = be_u16_at(data, 14, FVAR)?;
        let data_offset = be_u16_at(data, 4, FVAR)?;
        if axis_count == 0 || axis_size < 20 {
            return Err(ParseError::InvalidGlyphFormat);
        }
        if instance_count > 0 {
            let needed = usize::from(axis_count)
                .checked_mul(4)
                .and_then(|v| v.checked_add(4))
                .ok_or(ParseError::InvalidGlyphFormat)?;
            if usize::from(instance_size) < needed {
                return Err(ParseError::InvalidGlyphFormat);
            }
        }
        Ok(Self {
            data,
            axis_count,
            axis_size,
            instance_count,
            instance_size,
            data_offset,
        })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Table version.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn version(&self) -> Result<u32, ParseError> {
        be_u32_at(self.data, 0, FVAR)
    }

    /// Number of design axes.
    #[must_use]
    pub const fn axis_count(&self) -> u16 {
        self.axis_count
    }

    /// All design axes.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the axis array is short.
    pub fn axes(&self) -> Result<Vec<VariationAxis>, ParseError> {
        let mut out = Vec::with_capacity(usize::from(self.axis_count));
        for i in 0..usize::from(self.axis_count) {
            let at = usize::from(self.data_offset) + i * usize::from(self.axis_size);
            let tag = be_u32_at(self.data, at, FVAR)?;
            let min = Fixed::from_bits(be_i32_at(self.data, at + 4, FVAR)?);
            let default = Fixed::from_bits(be_i32_at(self.data, at + 8, FVAR)?);
            let max = Fixed::from_bits(be_i32_at(self.data, at + 12, FVAR)?);
            let flags = be_u16_at(self.data, at + 16, FVAR)?;
            let name_id = be_u16_at(self.data, at + 18, FVAR)?;
            out.push(VariationAxis {
                tag: Tag::new(&tag.to_be_bytes()),
                min,
                default,
                max,
                flags,
                name_id,
            });
        }
        Ok(out)
    }

    /// All named instances.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the instance array is short.
    pub fn instances(&self) -> Result<Vec<VarInstance>, ParseError> {
        let mut out = Vec::with_capacity(usize::from(self.instance_count));
        let base = usize::from(self.data_offset)
            + usize::from(self.axis_count) * usize::from(self.axis_size);
        for i in 0..usize::from(self.instance_count) {
            let at = base + i * usize::from(self.instance_size);
            let name_id = be_u16_at(self.data, at, FVAR)?;
            let flags = be_u16_at(self.data, at + 2, FVAR)?;
            let mut coordinates = Vec::with_capacity(usize::from(self.axis_count));
            for a in 0..usize::from(self.axis_count) {
                let raw = be_i32_at(self.data, at + 4 + a * 4, FVAR)?;
                coordinates.push(Fixed::from_bits(raw));
            }
            out.push(VarInstance {
                name_id,
                flags,
                coordinates,
            });
        }
        Ok(out)
    }
}

/// One axis segment map: sorted (from, to) normalization pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentMap {
    /// Breakpoint pairs, ascending by `from`.
    pub pairs: Vec<(F2Dot14, F2Dot14)>,
}

impl SegmentMap {
    /// Apply the map: piecewise-linear interpolation between breakpoints.
    /// Identity outside the declared range (spec requires a mapping for the
    /// full -1..1 domain; unsorted/short maps degrade to identity).
    #[must_use]
    pub fn map(&self, value: F2Dot14) -> F2Dot14 {
        if self.pairs.is_empty() {
            return value; // identity for absent/empty maps
        }
        let v = value.to_bits();
        let mut prev = (0i16, 0i16);
        let mut have_prev = false;
        for &(from, to) in &self.pairs {
            let (x, y) = (from.to_bits(), to.to_bits());
            if v <= x {
                if !have_prev {
                    // Before the first breakpoint: clamp to its target.
                    return F2Dot14::from_bits(y);
                }
                if x == prev.0 {
                    return F2Dot14::from_bits(y);
                }
                // Piecewise-linear interpolation between prev and (x, y).
                let t = f32::from(v - prev.0) / f32::from(x - prev.0);
                let y_out = f32::from(prev.1) + t * (f32::from(y) - f32::from(prev.1));
                // Round half away from zero (core has no f32::round); the
                // float->int cast truncates toward zero.
                let shifted = if y_out < 0.0 {
                    y_out - 0.5
                } else {
                    y_out + 0.5
                };
                return F2Dot14::from_bits(shifted as i16);
            }
            prev = (x, y);
            have_prev = true;
        }
        // Beyond the last breakpoint: clamp to its target.
        F2Dot14::from_bits(prev.1)
    }
}

/// Zero-copy view over the `avar` table.
#[derive(Debug, Clone, Copy)]
pub struct AvarTable<'a> {
    data: &'a [u8],
    axis_count: u16,
}

impl<'a> AvarTable<'a> {
    /// Parse and validate the `avar` table.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] / [`ParseError::InvalidGlyphFormat`]
    /// for short or inconsistent tables.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 8 {
            return Err(ParseError::TruncatedTable {
                tag: AVAR,
                expected: 8,
                got: data.len(),
            });
        }
        // Layout: version u32, reserved u16 (historically the axis count),
        // axisCount u16 at offset 6.
        let axis_count = be_u16_at(data, 6, AVAR)?;
        Ok(Self { data, axis_count })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Declared axis count of the segment map array.
    #[must_use]
    pub const fn axis_count(&self) -> u16 {
        self.axis_count
    }

    /// The per-axis segment maps.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when a map is short.
    pub fn segment_maps(&self) -> Result<Vec<SegmentMap>, ParseError> {
        let mut out = Vec::with_capacity(usize::from(self.axis_count));
        let mut at = 8usize;
        for _ in 0..usize::from(self.axis_count) {
            let count = usize::from(be_u16_at(self.data, at, AVAR)?);
            at += 2;
            let mut pairs = Vec::with_capacity(count);
            for _ in 0..count {
                let from = F2Dot14::from_bits(crate::reader::be_i16_at(self.data, at, AVAR)?);
                let to = F2Dot14::from_bits(crate::reader::be_i16_at(self.data, at + 2, AVAR)?);
                pairs.push((from, to));
                at += 4;
            }
            out.push(SegmentMap { pairs });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{AvarTable, FvarTable, SegmentMap};
    use crate::error::ParseError;
    use alloc::vec;
    use alloc::vec::Vec;
    use font_types::{F2Dot14, Tag};

    /// fvar: 1 axis (wght 400..900 default 400), 1 instance (600) with a
    /// postScriptNameID.
    fn fvar() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
        d.extend_from_slice(&16u16.to_be_bytes()); // offsetToData
        d.extend_from_slice(&2u16.to_be_bytes()); // countSizePairs
        d.extend_from_slice(&1u16.to_be_bytes()); // fontAxisCount
        d.extend_from_slice(&20u16.to_be_bytes()); // axisSize
        d.extend_from_slice(&1u16.to_be_bytes()); // fontInstanceCount
        d.extend_from_slice(&12u16.to_be_bytes()); // instanceSize (incl. psNameID)
                                                   // Axis @16: tag, min, default, max, flags, nameID
        d.extend_from_slice(b"wght");
        d.extend_from_slice(&400i32.to_be_bytes()); // min (Fixed)
        d.extend_from_slice(&400i32.to_be_bytes()); // default
        d.extend_from_slice(&900i32.to_be_bytes()); // max
        d.extend_from_slice(&0u16.to_be_bytes()); // flags
        d.extend_from_slice(&256u16.to_be_bytes()); // nameID
                                                    // Instance @36: subfamilyNameID, flags, coords, psNameID
        d.extend_from_slice(&257u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&600i32.to_be_bytes());
        d.extend_from_slice(&291u16.to_be_bytes());
        d
    }

    #[test]
    fn fvar_axes_and_instances() {
        let d = fvar();
        let t = FvarTable::parse(&d).unwrap();
        assert_eq!(t.version().unwrap(), 0x0001_0000);
        assert_eq!(t.axis_count(), 1);
        let axes = t.axes().unwrap();
        assert_eq!(axes.len(), 1);
        let a = &axes[0];
        assert_eq!(a.tag, Tag::new(b"wght"));
        assert_eq!(a.min.to_bits(), 400);
        assert_eq!(a.default.to_bits(), 400);
        assert_eq!(a.max.to_bits(), 900);
        assert_eq!(a.name_id, 256);
        let instances = t.instances().unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].name_id, 257);
        assert_eq!(instances[0].coordinates[0].to_bits(), 600);
        assert_eq!(t.as_bytes().len(), d.len());
    }

    #[test]
    fn fvar_errors() {
        assert!(matches!(
            FvarTable::parse(&[0u8; 15]),
            Err(ParseError::TruncatedTable { .. })
        ));
        // Zero axes.
        let mut d = fvar();
        d[8..10].copy_from_slice(&0u16.to_be_bytes());
        assert_eq!(
            FvarTable::parse(&d).unwrap_err(),
            ParseError::InvalidGlyphFormat
        );
        // axisSize below 20.
        let mut d = fvar();
        d[10..12].copy_from_slice(&8u16.to_be_bytes());
        assert_eq!(
            FvarTable::parse(&d).unwrap_err(),
            ParseError::InvalidGlyphFormat
        );
        // Instance too small for the axis count.
        let mut d = fvar();
        d[14..16].copy_from_slice(&6u16.to_be_bytes());
        assert_eq!(
            FvarTable::parse(&d).unwrap_err(),
            ParseError::InvalidGlyphFormat
        );
        // Truncated axis array surfaces from axes().
        let mut d = fvar();
        d.truncate(30);
        let t = FvarTable::parse(&d).unwrap();
        assert!(t.axes().is_err());
    }

    #[test]
    fn avar_maps_and_interpolation() {
        let mut d = Vec::new();
        d.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
        d.extend_from_slice(&0u16.to_be_bytes()); // reserved
        d.extend_from_slice(&1u16.to_be_bytes()); // axisCount
                                                  // Axis 0 map: 4 pairs.
        d.extend_from_slice(&4u16.to_be_bytes());
        for (from, to) in [(-1.0f32, -1.0), (0.0, 0.0), (0.5, 0.6), (1.0, 1.0)] {
            d.extend_from_slice(&F2Dot14::from_f32(from).to_bits().to_be_bytes());
            d.extend_from_slice(&F2Dot14::from_f32(to).to_bits().to_be_bytes());
        }
        let t = AvarTable::parse(&d).unwrap();
        assert_eq!(t.axis_count(), 1);
        let maps: Vec<SegmentMap> = t.segment_maps().unwrap();
        assert_eq!(maps.len(), 1);
        let m = &maps[0];
        assert_eq!(m.pairs.len(), 4);
        let f2 = |v: f32| F2Dot14::from_f32(v);
        assert_eq!(m.map(f2(-1.0)), f2(-1.0));
        assert_eq!(m.map(f2(0.0)), f2(0.0));
        assert_eq!(m.map(f2(1.0)), f2(1.0));
        let mapped = m.map(f2(0.5));
        assert!((mapped.to_f32() - 0.6).abs() < 0.001);
        // Identity for empty maps (below the pair minimum).
        let empty = SegmentMap { pairs: vec![] };
        assert_eq!(empty.map(f2(0.5)), f2(0.5));
        // Clamp below the first breakpoint.
        assert_eq!(m.map(f2(-2.0)), f2(-1.0));
        assert_eq!(m.map(f2(2.0)), f2(1.0));
    }

    #[test]
    fn avar_errors() {
        assert!(matches!(
            AvarTable::parse(&[0u8; 7]),
            Err(ParseError::TruncatedTable { .. })
        ));
        // Map truncated mid-pairs.
        let mut d = Vec::new();
        d.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&2u16.to_be_bytes()); // 2 pairs but only 1 present
        d.extend_from_slice(&[0xC0, 0x00, 0xC0, 0x00]);
        let t = AvarTable::parse(&d).unwrap();
        assert!(t.segment_maps().is_err());
    }
}
