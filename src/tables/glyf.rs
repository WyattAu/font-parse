//! `glyf` — glyph outlines: simple glyphs, composite glyphs, contour points.

use crate::error::{ParseError, RawTag};
use crate::reader::Reader;
use alloc::vec::Vec;
use font_types::{F2Dot14, GlyphId};

pub(crate) const TAG: RawTag = *b"glyf";

/// Simple-glyph flag: point is on the curve.
pub(crate) const FLAG_ON_CURVE: u8 = 0x01;
/// Simple-glyph flag: x delta is one byte.
pub(crate) const FLAG_X_SHORT: u8 = 0x02;
/// Simple-glyph flag: y delta is one byte.
pub(crate) const FLAG_Y_SHORT: u8 = 0x04;
/// Simple-glyph flag: next byte repeats this flag.
pub(crate) const FLAG_REPEAT: u8 = 0x08;
/// Simple-glyph flag: x is same (long form) / positive (short form).
pub(crate) const FLAG_X_SAME_OR_POS: u8 = 0x10;
/// Simple-glyph flag: y is same (long form) / positive (short form).
pub(crate) const FLAG_Y_SAME_OR_POS: u8 = 0x20;
/// Simple-glyph overlap flag (bit 6).
#[cfg(feature = "woff2")]
pub(crate) const FLAG_OVERLAP_SIMPLE: u8 = 0x40;

/// Composite-glyph flag: arguments are 16-bit words.
pub(crate) const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
/// Composite-glyph flag: arguments are x/y offsets (not point indices).
pub(crate) const ARGS_ARE_XY_VALUES: u16 = 0x0002;
/// Composite-glyph flag: more components follow.
pub(crate) const MORE_COMPONENTS: u16 = 0x0020;
/// Composite-glyph flag: scale is present (a == d).
pub(crate) const WE_HAVE_A_SCALE: u16 = 0x0008;
/// Composite-glyph flag: x/y scale present.
pub(crate) const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
/// Composite-glyph flag: full 2x2 matrix present.
pub(crate) const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
/// Composite-glyph flag: glyph instructions follow the components.
#[cfg(feature = "woff2")]
pub(crate) const WE_HAVE_INSTRUCTIONS: u16 = 0x0100;

/// Hard cap on composite nesting depth (spec recommends <= 13; malformed
/// fonts can declare cycles).
pub(crate) const MAX_COMPONENT_DEPTH: u8 = 16;
/// Hard cap on resolved outline points/components per glyph — bounds the
/// composite expansion of hostile inputs.
pub(crate) const MAX_OUTLINE_POINTS: usize = 1 << 20;

/// A resolved outline point: font-unit coordinates plus curve membership.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlinePoint {
    /// X coordinate in font units.
    pub x: f32,
    /// Y coordinate in font units.
    pub y: f32,
    /// True for on-curve points; False marks quadratic control points.
    pub on_curve: bool,
}

/// One contour of an outline: an ordered run of points.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    /// Contour points, in outline order.
    pub points: Vec<OutlinePoint>,
}

/// A fully resolved glyph outline (composite components applied).
///
/// Extracting an outline allocates; table access itself stays zero-copy.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GlyphOutline {
    /// Contours in glyph order.
    pub contours: Vec<Contour>,
}

impl GlyphOutline {
    /// Total point count across all contours.
    #[must_use]
    pub fn point_count(&self) -> usize {
        self.contours.iter().map(|c| c.points.len()).sum()
    }

    /// True when the outline has no contours (space-like glyph).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contours.is_empty()
    }
}

/// A zero-copy view over one glyph record of the `glyf` table.
#[derive(Debug, Clone, Copy)]
pub enum GlyphRef<'a> {
    /// A simple glyph: raw contours with delta-coded coordinates.
    Simple(SimpleGlyph<'a>),
    /// A composite glyph: references to component glyphs plus transforms.
    Composite(CompositeGlyph<'a>),
}

impl<'a> GlyphRef<'a> {
    /// Parse one glyph record (a `loca`-resolved byte range).
    ///
    /// An empty range decodes to `Ok(None)` (empty glyph).
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] / [`ParseError::InvalidGlyphFormat`]
    /// for malformed records.
    pub fn parse(data: &'a [u8]) -> Result<Option<Self>, ParseError> {
        if data.is_empty() {
            return Ok(None);
        }
        if data.len() < 10 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 10,
                got: data.len(),
            });
        }
        let n_contours = crate::reader::be_i16_at(data, 0, TAG)?;
        if n_contours >= 0 {
            Ok(Some(Self::Simple(SimpleGlyph::parse(data)?)))
        } else if n_contours == -1 {
            Ok(Some(Self::Composite(CompositeGlyph::parse(data)?)))
        } else {
            Err(ParseError::InvalidGlyphFormat)
        }
    }

    /// The declared bounding box (`xMin, yMin, xMax, yMax`).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn bounds(&self) -> Result<[i16; 4], ParseError> {
        match self {
            Self::Simple(g) => g.bounds(),
            Self::Composite(g) => g.bounds(),
        }
    }
}

/// A simple (non-composite) glyph record.
#[derive(Debug, Clone, Copy)]
pub struct SimpleGlyph<'a> {
    data: &'a [u8],
    n_contours: u16,
}

impl<'a> SimpleGlyph<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        let n = crate::reader::be_u16_at(data, 0, TAG)?;
        Ok(Self {
            data,
            n_contours: n,
        })
    }

    /// Raw record bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Number of contours.
    #[must_use]
    pub const fn num_contours(&self) -> u16 {
        self.n_contours
    }

    /// Declared bounding box.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn bounds(&self) -> Result<[i16; 4], ParseError> {
        Ok([
            crate::reader::be_i16_at(self.data, 2, TAG)?,
            crate::reader::be_i16_at(self.data, 4, TAG)?,
            crate::reader::be_i16_at(self.data, 6, TAG)?,
            crate::reader::be_i16_at(self.data, 8, TAG)?,
        ])
    }

    /// The `endPtsOfContours` array.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the array exceeds the record.
    pub fn end_points(&self) -> Result<Vec<u16>, ParseError> {
        let n = usize::from(self.n_contours);
        let at = 10usize;
        let end = at
            .checked_add(n * 2)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if self.data.len() < end {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: end,
                got: self.data.len(),
            });
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            out.push(crate::reader::be_u16_at(self.data, at + i * 2, TAG)?);
        }
        Ok(out)
    }

    /// Decode contour points (flags expanded, delta coordinates resolved).
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] / [`ParseError::InvalidGlyphFormat`]
    /// for short records or inconsistent coordinate streams.
    pub fn points(&self) -> Result<Vec<OutlinePoint>, ParseError> {
        let mut r = Reader::new(self.data, TAG);
        r.skip(10)?;
        let ends = self.end_points()?;
        let n_points = ends.last().map_or(0, |&e| usize::from(e) + 1);
        r.skip(ends.len() * 2)?;
        // Instructions.
        let instr_len = usize::from(r.u16()?);
        r.skip(instr_len)?;
        // Flags with repeat expansion.
        let mut flags = Vec::with_capacity(n_points);
        while flags.len() < n_points {
            let flag = r.u8()?;
            flags.push(flag);
            if flag & FLAG_REPEAT != 0 {
                let repeat = usize::from(r.u8()?);
                for _ in 0..repeat {
                    if flags.len() >= n_points {
                        return Err(ParseError::InvalidGlyphFormat);
                    }
                    flags.push(flag);
                }
            }
        }
        // X coordinates (delta coded).
        let mut xs = Vec::with_capacity(n_points);
        let mut x: i32 = 0;
        for &flag in &flags {
            if flag & FLAG_X_SHORT != 0 {
                let d = i32::from(r.u8()?);
                x += if flag & FLAG_X_SAME_OR_POS != 0 {
                    d
                } else {
                    -d
                };
            } else if flag & FLAG_X_SAME_OR_POS == 0 {
                x += i32::from(r.i16()?);
            }
            xs.push(x);
        }
        // Y coordinates, zipped with the resolved X deltas.
        let mut pts = Vec::with_capacity(n_points);
        let mut y: i32 = 0;
        for (i, &flag) in flags.iter().enumerate() {
            if flag & FLAG_Y_SHORT != 0 {
                let d = i32::from(r.u8()?);
                y += if flag & FLAG_Y_SAME_OR_POS != 0 {
                    d
                } else {
                    -d
                };
            } else if flag & FLAG_Y_SAME_OR_POS == 0 {
                y += i32::from(r.i16()?);
            }
            if i >= MAX_OUTLINE_POINTS {
                return Err(ParseError::InvalidGlyphFormat);
            }
            let x = *xs.get(i).unwrap_or(&0);
            pts.push(OutlinePoint {
                x: x as f32,
                y: y as f32,
                on_curve: flag & FLAG_ON_CURVE != 0,
            });
        }
        Ok(pts)
    }

    /// Collect the glyph's contours into an outline.
    ///
    /// # Errors
    /// See [`points`](Self::points).
    pub fn outline(&self) -> Result<GlyphOutline, ParseError> {
        let pts = self.points()?;
        let ends = self.end_points()?;
        let mut contours = Vec::with_capacity(ends.len());
        let mut start = 0usize;
        for &e in &ends {
            let end = usize::from(e) + 1;
            if end > pts.len() || start > end {
                return Err(ParseError::InvalidGlyphFormat);
            }
            let slice = pts.get(start..end).ok_or(ParseError::InvalidGlyphFormat)?;
            contours.push(Contour {
                points: slice.to_vec(),
            });
            start = end;
        }
        Ok(GlyphOutline { contours })
    }
}

/// One component of a composite glyph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Component {
    /// Referenced glyph.
    pub glyph: GlyphId,
    /// Raw flag word.
    pub flags: u16,
    /// Arguments: x/y offsets (font units) when
    /// `flags & ARGS_ARE_XY_VALUES`, else point indices.
    pub args: (i32, i32),
    /// 2x2 transform (`a b c d`): `x' = a*x + c*y + e`.
    pub transform: [f32; 4],
}

impl Component {
    /// True when the args are x/y placement offsets.
    #[must_use]
    pub const fn args_are_offsets(&self) -> bool {
        self.flags & ARGS_ARE_XY_VALUES != 0
    }
}

/// A composite glyph record.
#[derive(Debug, Clone, Copy)]
pub struct CompositeGlyph<'a> {
    data: &'a [u8],
}

impl<'a> CompositeGlyph<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        Ok(Self { data })
    }

    /// Raw record bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Declared bounding box.
    ///
    /// # Errors
    /// Truncation (impossible post-[`GlyphRef::parse`]).
    pub fn bounds(&self) -> Result<[i16; 4], ParseError> {
        Ok([
            crate::reader::be_i16_at(self.data, 2, TAG)?,
            crate::reader::be_i16_at(self.data, 4, TAG)?,
            crate::reader::be_i16_at(self.data, 6, TAG)?,
            crate::reader::be_i16_at(self.data, 8, TAG)?,
        ])
    }

    /// Decode the component list (flags, args, transform matrices).
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] / [`ParseError::InvalidGlyphFormat`]
    /// for truncated records or runaway component lists.
    pub fn components(&self) -> Result<Vec<Component>, ParseError> {
        let mut r = Reader::new(self.data, TAG);
        r.skip(10)?;
        let mut out = Vec::new();
        loop {
            if out.len() > MAX_OUTLINE_POINTS {
                return Err(ParseError::InvalidGlyphFormat);
            }
            let flags = r.u16()?;
            let glyph = GlyphId::new(u32::from(r.u16()?));
            let (a1, a2) = if flags & ARG_1_AND_2_ARE_WORDS != 0 {
                (i32::from(r.i16()?), i32::from(r.i16()?))
            } else {
                (i32::from(r.i8()?), i32::from(r.i8()?))
            };
            let scale = |r: &mut Reader<'a>| -> Result<F2Dot14, ParseError> { r.f2dot14() };
            let transform = if flags & WE_HAVE_A_SCALE != 0 {
                let a = scale(&mut r)?;
                [a.to_f32(), 0.0, 0.0, a.to_f32()]
            } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
                let a = scale(&mut r)?;
                let d = scale(&mut r)?;
                [a.to_f32(), 0.0, 0.0, d.to_f32()]
            } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
                let a = scale(&mut r)?;
                let b = scale(&mut r)?;
                let c = scale(&mut r)?;
                let d = scale(&mut r)?;
                [a.to_f32(), b.to_f32(), c.to_f32(), d.to_f32()]
            } else {
                [1.0, 0.0, 0.0, 1.0]
            };
            out.push(Component {
                glyph,
                flags,
                args: (a1, a2),
                transform,
            });
            if flags & MORE_COMPONENTS == 0 {
                break;
            }
        }
        Ok(out)
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

    use super::{Component, GlyphOutline, GlyphRef, OutlinePoint, SimpleGlyph};
    use crate::error::ParseError;
    use alloc::vec;
    use alloc::vec::Vec;
    use font_types::GlyphId;

    /// One-contour triangle, 3 on-curve points, deltas: x (100, 0, 100),
    /// y (0, 200, -200). Header + ends + instrLen(0) + flags + coords.
    fn triangle() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&1i16.to_be_bytes()); // contours
        d.extend_from_slice(&0i16.to_be_bytes());
        d.extend_from_slice(&100i16.to_be_bytes());
        d.extend_from_slice(&300i16.to_be_bytes());
        d.extend_from_slice(&200i16.to_be_bytes());
        d.extend_from_slice(&2u16.to_be_bytes()); // endPts [2]
        d.extend_from_slice(&0u16.to_be_bytes()); // instructionLength
                                                  // p0/p1: on-curve + x/y short positive (0x37); p2: y short negative
                                                  // (0x17).
        d.extend_from_slice(&[0x37, 0x37, 0x17]);
        // x deltas: +100, +0, +100 (one unsigned byte each).
        d.extend_from_slice(&[100, 0, 100]);
        // y deltas: +0, +200, -200 (unsigned byte, sign from the flag).
        d.extend_from_slice(&[0, 200, 200]);
        d
    }

    #[test]
    fn simple_glyph_points_and_contours() {
        let d = triangle();
        let g = GlyphRef::parse(&d).unwrap().unwrap();
        let super::GlyphRef::Simple(simple) = g else {
            panic!("expected simple glyph");
        };
        assert_eq!(simple.num_contours(), 1);
        assert_eq!(simple.bounds().unwrap(), [0, 100, 300, 200]);
        assert_eq!(simple.end_points().unwrap(), vec![2]);
        let pts = simple.points().unwrap();
        assert_eq!(
            pts,
            vec![
                OutlinePoint {
                    x: 100.0,
                    y: 0.0,
                    on_curve: true
                },
                OutlinePoint {
                    x: 100.0,
                    y: 200.0,
                    on_curve: true
                },
                OutlinePoint {
                    x: 200.0,
                    y: 0.0,
                    on_curve: true
                },
            ]
        );
        let outline: GlyphOutline = simple.outline().unwrap();
        assert_eq!(outline.contours.len(), 1);
        assert_eq!(outline.point_count(), 3);
    }

    #[test]
    fn repeat_flags_and_off_curve() {
        // Two points sharing coordinates via the flag repeat mechanism.
        let mut g = Vec::new();
        g.extend_from_slice(&1i16.to_be_bytes());
        g.extend_from_slice(&0i16.to_be_bytes());
        g.extend_from_slice(&0i16.to_be_bytes());
        g.extend_from_slice(&200i16.to_be_bytes());
        g.extend_from_slice(&100i16.to_be_bytes());
        g.extend_from_slice(&1u16.to_be_bytes()); // endPts [1]
        g.extend_from_slice(&0u16.to_be_bytes());
        // 0x3F = on-curve + x/y short positive + repeat; one repeat -> 2 points.
        g.extend_from_slice(&[0x3F, 1]);
        g.extend_from_slice(&[100, 100]); // x short (per point)
        g.extend_from_slice(&[200, 200]); // y short (per point)
        let glyph = GlyphRef::parse(&g).unwrap().unwrap();
        let super::GlyphRef::Simple(simple) = glyph else {
            panic!("expected simple");
        };
        let pts = simple.points().unwrap();
        // Deltas accumulate: p0 (100, 200), p1 (+100, +200).
        assert_eq!(
            pts,
            vec![
                OutlinePoint {
                    x: 100.0,
                    y: 200.0,
                    on_curve: true
                },
                OutlinePoint {
                    x: 200.0,
                    y: 400.0,
                    on_curve: true
                },
            ]
        );
    }

    #[test]
    fn composite_components_and_flags() {
        // Composite referencing gid 7 with point-matched args (words) and a
        // 2x2 transform.
        let mut d = Vec::new();
        d.extend_from_slice(&(-1i16).to_be_bytes());
        d.extend_from_slice(&0i16.to_be_bytes());
        d.extend_from_slice(&0i16.to_be_bytes());
        d.extend_from_slice(&400i16.to_be_bytes());
        d.extend_from_slice(&400i16.to_be_bytes());
        // flags: ARGS_ARE_WORDS | WE_HAVE_A_TWO_BY_TWO (no MORE)
        let flags: u16 = 0x0001 | 0x0080;
        d.extend_from_slice(&(flags | 0x0100u16).to_be_bytes()); // + instructions
        d.extend_from_slice(&7u16.to_be_bytes());
        d.extend_from_slice(&3i16.to_be_bytes()); // arg1 (word, point index)
        d.extend_from_slice(&9i16.to_be_bytes());
        // 2x2 F2Dot14: a=1.0 (0x4000), b=0, c=0, d=0.5 (0x2000)
        d.extend_from_slice(&0x4000i16.to_be_bytes());
        d.extend_from_slice(&0i16.to_be_bytes());
        d.extend_from_slice(&0i16.to_be_bytes());
        d.extend_from_slice(&0x2000i16.to_be_bytes());
        d.extend_from_slice(&2u16.to_be_bytes()); // instruction length
        d.extend_from_slice(&[0xAA, 0xBB]);
        let glyph = GlyphRef::parse(&d).unwrap().unwrap();
        let super::GlyphRef::Composite(composite) = glyph else {
            panic!("expected composite");
        };
        let comps: Vec<Component> = composite.components().unwrap();
        assert_eq!(comps.len(), 1);
        let c = comps[0];
        assert_eq!(c.glyph, GlyphId::new(7));
        assert!(!c.args_are_offsets());
        assert_eq!(c.args, (3, 9));
        assert!((c.transform[0] - 1.0).abs() < 1e-6);
        assert!((c.transform[3] - 0.5).abs() < 1e-6);
        assert_eq!(composite.bounds().unwrap(), [0, 0, 400, 400]);
    }

    #[test]
    fn malformed_records() {
        // nContours < -1 is invalid (after the 10-byte header).
        let mut d = Vec::new();
        d.extend_from_slice(&(-3i16).to_be_bytes());
        d.extend_from_slice(&[0u8; 9]);
        assert!(matches!(
            GlyphRef::parse(&d),
            Err(ParseError::InvalidGlyphFormat)
        ));
        // Short header.
        assert!(matches!(
            GlyphRef::parse(&[0, 1, 0]),
            Err(ParseError::TruncatedTable { .. })
        ));
        // Empty record range -> None.
        assert!(GlyphRef::parse(&[]).unwrap().is_none());
        // Simple glyph whose endPts exceed the data.
        let mut g = triangle();
        g[10..12].copy_from_slice(&50u16.to_be_bytes()); // endPts [50]
        let glyph = GlyphRef::parse(&g).unwrap().unwrap();
        let super::GlyphRef::Simple(simple) = glyph else {
            panic!("expected simple");
        };
        assert!(simple.points().is_err()); // flag/x stream runs off the record
        let _ = SimpleGlyph::parse(&g).unwrap().num_contours();
    }
}
