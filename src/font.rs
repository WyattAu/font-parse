//! Font containers: [`FontRef`] (zero-copy), [`OwnedFont`], and
//! [`FontCollection`] (TTC).

use crate::error::{ParseError, RawTag};
use crate::reader::Reader;
use crate::tables;
use crate::tables::glyf::{GlyphOutline, GlyphRef, MAX_COMPONENT_DEPTH, MAX_OUTLINE_POINTS};
use crate::tables::{Cmap, HeadTable};
use crate::tag::TableTag;
use alloc::vec::Vec;
use font_types::{F2Dot14, GlyphId, Tag};

const SFNT_VERSION_TRUE: u32 = 0x0001_0000;
const SFNT_VERSION_OTTO: u32 = 0x4F54_544F; // 'OTTO'
const SFNT_VERSION_TRUE_APPLE: u32 = 0x7472_7565; // 'true'
const TTC_TAG: RawTag = *b"ttcf";
const SFNT_TAG: RawTag = *b"sfnt";

/// Directory-entry size.
const ENTRY: usize = 16;

fn is_known_sfnt_version(version: u32) -> bool {
    matches!(
        version,
        SFNT_VERSION_TRUE | SFNT_VERSION_OTTO | SFNT_VERSION_TRUE_APPLE
    )
}

/// Zero-copy reference over an SFNT font.
///
/// The reference borrows the whole byte buffer; table access is a lifetime-
/// bound slice — nothing is copied, nothing is allocated. Views obtained
/// from it (typed tables, outlines) die with the borrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontRef<'a> {
    data: &'a [u8],
    /// Offset of the 12-byte offset table within `data` (non-zero for fonts
    /// inside a TTC, whose table offsets are file-relative).
    dir_offset: usize,
    num_tables: u16,
    sfnt_version: u32,
}

impl<'a> FontRef<'a> {
    /// Parse and validate the SFNT offset table and every table-directory
    /// entry.
    ///
    /// Accepts TrueType (`0x00010000`, `'true'`) and CFF-flavored (`'OTTO'`)
    /// fonts; for collections ('ttcf') use [`FontCollection`].
    ///
    /// # Errors
    /// [`ParseError::InvalidSfnt`] for a bad signature or zero table count;
    /// [`ParseError::TruncatedTable`] for a truncated directory or an entry
    /// whose `offset + length` exceeds the buffer.
    pub fn new(data: &'a [u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(data, SFNT_TAG);
        let version = r.u32()?;
        if !is_known_sfnt_version(version) {
            return Err(ParseError::InvalidSfnt);
        }
        let num_tables = r.u16()?;
        if num_tables == 0 {
            return Err(ParseError::InvalidSfnt);
        }
        r.skip(6)?; // searchRange, entrySelector, rangeShift (informational)
        validate_directory(data, 0, num_tables)?;
        Ok(Self {
            data,
            dir_offset: 0,
            num_tables,
            sfnt_version: version,
        })
    }

    /// The whole borrowed buffer.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// The `sfntVersion` field (`0x00010000`, `'OTTO'`, `'true'`).
    #[must_use]
    pub const fn sfnt_version(&self) -> u32 {
        self.sfnt_version
    }

    /// True for CFF-outline ('OTTO') fonts — `glyf`/`loca` are absent.
    #[must_use]
    pub const fn is_cff(&self) -> bool {
        self.sfnt_version == SFNT_VERSION_OTTO
    }

    /// True when the font declares variable-font tables (`fvar`).
    #[must_use]
    pub fn is_variable(&self) -> bool {
        self.table(TableTag::Fvar).is_some()
    }

    /// Number of directory entries.
    #[must_use]
    pub const fn num_tables(&self) -> u16 {
        self.num_tables
    }

    /// Raw bytes of a known table (`None` when absent).
    #[must_use]
    pub fn table(&self, tag: TableTag) -> Option<&'a [u8]> {
        self.table_by_tag(tag.to_tag())
    }

    /// Raw bytes of an arbitrary table by raw tag.
    #[must_use]
    pub fn table_by_tag(&self, tag: Tag) -> Option<&'a [u8]> {
        let want = tag.to_be_bytes();
        for i in 0..usize::from(self.num_tables) {
            let at = self.dir_offset + 12 + i * ENTRY;
            let found = crate::reader::slice_at(self.data, at, 4, SFNT_TAG).ok()?;
            if found == want {
                let offset = crate::reader::be_u32_at(self.data, at + 8, SFNT_TAG).ok()?;
                let length = crate::reader::be_u32_at(self.data, at + 12, SFNT_TAG).ok()?;
                let offset = usize::try_from(offset).ok()?;
                let length = usize::try_from(length).ok()?;
                let end = offset.checked_add(length)?;
                return self.data.get(offset..end);
            }
        }
        None
    }

    /// All directory entries as `(tag, checksum, offset, length)`, in
    /// directory order. Tags are raw four-byte values (known or not).
    #[must_use]
    pub fn table_directory(&self) -> Vec<(RawTag, u32, u32, u32)> {
        let mut out = Vec::with_capacity(usize::from(self.num_tables));
        for i in 0..usize::from(self.num_tables) {
            let at = self.dir_offset + 12 + i * ENTRY;
            let (Ok(raw_tag), Ok(checksum), Ok(offset), Ok(length)) = (
                crate::reader::be_u32_at(self.data, at, SFNT_TAG),
                crate::reader::be_u32_at(self.data, at + 4, SFNT_TAG),
                crate::reader::be_u32_at(self.data, at + 8, SFNT_TAG),
                crate::reader::be_u32_at(self.data, at + 12, SFNT_TAG),
            ) else {
                continue;
            };
            out.push((raw_tag.to_be_bytes(), checksum, offset, length));
        }
        out
    }

    // -- typed required tables ------------------------------------------

    fn table_or_err(&self, tag: TableTag) -> Result<&'a [u8], ParseError> {
        self.table(tag)
            .ok_or(ParseError::UnknownTableTag(tag.to_bytes()))
    }

    /// `head` — units per em, loca format, style flags.
    ///
    /// # Errors
    /// Missing table ([`ParseError::UnknownTableTag`]) or parse errors.
    pub fn head(&self) -> Result<HeadTable<'a>, ParseError> {
        HeadTable::parse(self.table_or_err(TableTag::Head)?)
    }

    /// `hhea` — horizontal header.
    ///
    /// # Errors
    /// Missing table or parse errors.
    pub fn hhea(&self) -> Result<tables::HheaTable<'a>, ParseError> {
        tables::HheaTable::parse(self.table_or_err(TableTag::Hhea)?)
    }

    /// `hmtx` — advance widths and side bearings (wired to `hhea` + `maxp`).
    ///
    /// # Errors
    /// Missing tables or parse errors.
    pub fn hmtx(&self) -> Result<tables::HmtxTable<'a>, ParseError> {
        let num_h_metrics = self.hhea()?.number_of_h_metrics()?;
        tables::HmtxTable::parse(
            self.table_or_err(TableTag::Hmtx)?,
            num_h_metrics,
            self.num_glyphs(),
        )
    }

    /// `maxp` — glyph count and memory hints.
    ///
    /// # Errors
    /// Missing table or parse errors.
    pub fn maxp(&self) -> Result<tables::MaxpTable<'a>, ParseError> {
        tables::MaxpTable::parse(self.table_or_err(TableTag::Maxp)?)
    }

    /// Glyph count (0 when `maxp` is absent or invalid).
    #[must_use]
    pub fn num_glyphs(&self) -> u16 {
        self.table(TableTag::Maxp)
            .and_then(|d| tables::MaxpTable::parse(d).ok())
            .map_or(0, |m| m.num_glyphs())
    }

    /// `cmap` — character-to-glyph mapping.
    ///
    /// # Errors
    /// Missing table or parse errors.
    pub fn cmap(&self) -> Result<Cmap<'a>, ParseError> {
        Cmap::parse(self.table_or_err(TableTag::Cmap)?)
    }

    /// `name` — naming table.
    ///
    /// # Errors
    /// Missing table or parse errors.
    pub fn name(&self) -> Result<tables::NameTable<'a>, ParseError> {
        tables::NameTable::parse(self.table_or_err(TableTag::Name)?)
    }

    /// `OS/2` — weight/width classes and metrics.
    ///
    /// # Errors
    /// Missing table or parse errors.
    pub fn os2(&self) -> Result<tables::Os2Table<'a>, ParseError> {
        tables::Os2Table::parse(self.table_or_err(TableTag::Os2)?)
    }

    /// `post` — PostScript names and italic angle.
    ///
    /// # Errors
    /// Missing table or parse errors.
    pub fn post(&self) -> Result<tables::PostTable<'a>, ParseError> {
        tables::PostTable::parse(self.table_or_err(TableTag::Post)?)
    }

    // -- optional tables -------------------------------------------------

    /// `stat` — style attributes.
    ///
    /// # Errors
    /// Missing table or parse errors.
    pub fn stat(&self) -> Result<tables::StatTable<'a>, ParseError> {
        tables::StatTable::parse(self.table_or_err(TableTag::Stat)?)
    }

    /// `DSIG` — digital signature (header only).
    ///
    /// # Errors
    /// Missing table or parse errors.
    pub fn dsig(&self) -> Result<tables::DsigTable<'a>, ParseError> {
        tables::DsigTable::parse(self.table_or_err(TableTag::Dsig)?)
    }

    /// `GSUB`/`GPOS` layout structure by kind.
    ///
    /// # Errors
    /// Missing table ([`ParseError::UnknownTableTag`]) or parse errors.
    pub fn layout(&self, kind: tables::LayoutKind) -> Result<tables::LayoutTable<'a>, ParseError> {
        let tag = kind.tag();
        let data = self
            .table_by_tag(Tag::new(&tag))
            .ok_or(ParseError::UnknownTableTag(tag))?;
        tables::LayoutTable::parse(data, kind)
    }

    /// `fvar` + optional `avar`/`gvar` — the variable-font view.
    ///
    /// # Errors
    /// Missing `fvar` or parse errors.
    pub fn variable(&self) -> Result<VariableFont<'a>, ParseError> {
        let fvar = tables::FvarTable::parse(self.table_or_err(TableTag::Fvar)?)?;
        let avar = self
            .table(TableTag::Avar)
            .map(tables::AvarTable::parse)
            .transpose()?;
        let gvar = self
            .table(TableTag::Gvar)
            .map(|d| tables::GvarTable::parse(d, self.num_glyphs()))
            .transpose()?;
        Ok(VariableFont {
            font: *self,
            fvar,
            avar,
            gvar,
        })
    }

    // -- glyph access -----------------------------------------------------

    /// Byte range of glyph `gid` within `glyf` (`None` = empty glyph or an
    /// id beyond `numGlyphs`).
    ///
    /// # Errors
    /// Missing `glyf`/`loca` tables or malformed loca entries.
    pub fn glyph_data(&self, gid: GlyphId) -> Result<Option<&'a [u8]>, ParseError> {
        let Some(range) = self.locus(gid)? else {
            return Ok(None);
        };
        let glyf = self.table_or_err(TableTag::Glyf)?;
        Ok(glyf.get(range.start..range.end))
    }

    /// Parse glyph `gid` (simple or composite view; `None` = empty).
    ///
    /// # Errors
    /// Missing tables or malformed glyph records.
    pub fn glyph(&self, gid: GlyphId) -> Result<Option<GlyphRef<'a>>, ParseError> {
        match self.glyph_data(gid)? {
            None => Ok(None),
            Some(data) => GlyphRef::parse(data),
        }
    }

    /// The `loca` table wired to `head` + `maxp`.
    fn loca(&self) -> Result<tables::LocaTable<'a>, ParseError> {
        let long = self.head()?.index_to_loc_format()? == 1;
        tables::LocaTable::parse(self.table_or_err(TableTag::Loca)?, self.num_glyphs(), long)
    }

    /// Resolve `gid` to a byte range in `glyf`, or `None` for empty or
    /// out-of-range ids (ids beyond `u16::MAX` included).
    fn locus(&self, gid: GlyphId) -> Result<Option<core::ops::Range<usize>>, ParseError> {
        let Ok(id) = u16::try_from(gid.to_u32()) else {
            return Ok(None);
        };
        self.loca()?.glyph_range(id)
    }

    /// Advance width for `gid` via `hmtx`.
    ///
    /// # Errors
    /// Missing tables or malformed `hmtx`.
    pub fn advance_width(&self, gid: GlyphId) -> Result<u16, ParseError> {
        let Ok(id) = u16::try_from(gid.to_u32()) else {
            return Ok(0); // glyph 0 (.notdef) convention for absurd ids
        };
        self.hmtx()?.advance(id)
    }

    /// Fully resolved outline for `gid`: composite components (with their
    /// 2x2 transforms and offsets) are applied recursively.
    ///
    /// Returns `None` for empty glyphs or ids beyond `numGlyphs`. The result
    /// is allocated (points in font units); table access stays zero-copy.
    /// Point-matched composites (no `ARGS_ARE_XY_VALUES`) place components
    /// at the origin — full point matching is a rendering concern and out of
    /// scope for a parser.
    ///
    /// # Errors
    /// Malformed glyph records, or a composite nest that exceeds the depth
    /// cap ([`MAX_COMPONENT_DEPTH`]) or the point budget
    /// ([`MAX_OUTLINE_POINTS`]).
    pub fn outline(&self, gid: GlyphId) -> Result<Option<GlyphOutline>, ParseError> {
        self.outline_bounded(gid, 0, &mut 0)
    }

    fn outline_bounded(
        &self,
        gid: GlyphId,
        depth: u8,
        budget: &mut usize,
    ) -> Result<Option<GlyphOutline>, ParseError> {
        if depth > MAX_COMPONENT_DEPTH {
            return Err(ParseError::InvalidGlyphFormat);
        }
        let Some(glyph) = self.glyph(gid)? else {
            return Ok(None);
        };
        match glyph {
            GlyphRef::Simple(simple) => {
                let outline = simple.outline()?;
                *budget = budget.saturating_add(outline.point_count());
                if *budget > MAX_OUTLINE_POINTS {
                    return Err(ParseError::InvalidGlyphFormat);
                }
                Ok(Some(outline))
            }
            GlyphRef::Composite(composite) => {
                let mut contours = Vec::new();
                for component in composite.components()? {
                    *budget = budget.saturating_add(1);
                    if *budget > MAX_OUTLINE_POINTS {
                        return Err(ParseError::InvalidGlyphFormat);
                    }
                    let Some(inner) = self.outline_bounded(component.glyph, depth + 1, budget)?
                    else {
                        continue;
                    };
                    let (dx, dy) = if component.args_are_offsets() {
                        (
                            f32::from(i16::try_from(component.args.0).unwrap_or(0)),
                            f32::from(i16::try_from(component.args.1).unwrap_or(0)),
                        )
                    } else {
                        // Point-matched components: see `outline` docs.
                        (0.0, 0.0)
                    };
                    let [a, b, c, d] = component.transform;
                    for mut contour in inner.contours {
                        for pt in &mut contour.points {
                            let x = a * pt.x + c * pt.y + dx;
                            let y = b * pt.x + d * pt.y + dy;
                            pt.x = x;
                            pt.y = y;
                        }
                        contours.push(contour);
                    }
                }
                Ok(Some(GlyphOutline { contours }))
            }
        }
    }

    /// Variable-font deltas for `gid` via `gvar` (empty vec when the font is
    /// not variable or the glyph has no deltas). The glyph's point count is
    /// resolved from `glyf` automatically.
    ///
    /// # Errors
    /// Malformed gvar data.
    pub fn gvar_deltas(&self, gid: GlyphId) -> Result<Vec<tables::TupleVariation>, ParseError> {
        let Some(gvar) = self.table(TableTag::Gvar) else {
            return Ok(Vec::new());
        };
        let point_count = self.outline_point_count(gid)?;
        let gvar = tables::GvarTable::parse(gvar, self.num_glyphs())?;
        let Ok(id) = u16::try_from(gid.to_u32()) else {
            return Ok(Vec::new());
        };
        gvar.glyph_deltas(id, point_count)
    }

    /// Point count of `gid`'s glyph record for gvar sizing: outline points
    /// (components for composites) **plus the 4 phantom points** every glyph
    /// carries in variation data ("apply to all points" tuples encode a
    /// delta for each of them).
    fn outline_point_count(&self, gid: GlyphId) -> Result<u16, ParseError> {
        let Some(data) = self.glyph_data(gid)? else {
            return Ok(4); // empty glyph: phantom points only
        };
        let n = match GlyphRef::parse(data)? {
            None => 0,
            Some(GlyphRef::Simple(s)) => {
                let pts = s.points()?;
                usize::from(u16::try_from(pts.len()).map_err(|_| ParseError::InvalidGlyphFormat)?)
            }
            Some(GlyphRef::Composite(c)) => {
                let comps = c.components()?;
                usize::from(u16::try_from(comps.len()).map_err(|_| ParseError::InvalidGlyphFormat)?)
            }
        };
        u16::try_from(n + 4).map_err(|_| ParseError::InvalidGlyphFormat)
    }
}

/// An owned font buffer, validated at construction.
///
/// The allocating counterpart of [`FontRef`]: produced by SFNT validation of
/// an owned buffer, WOFF/WOFF2 decoding, or extraction from a collection.
/// Access the zero-copy view with [`as_font_ref`](Self::as_font_ref).
#[derive(Debug, Clone)]
pub struct OwnedFont {
    data: Vec<u8>,
}

impl OwnedFont {
    /// Validate an owned SFNT buffer.
    ///
    /// # Errors
    /// See [`FontRef::new`].
    pub fn from_sfnt(data: Vec<u8>) -> Result<Self, ParseError> {
        FontRef::new(&data)?;
        Ok(Self { data })
    }

    /// Decode a WOFF 1.0 container (feature `woff`).
    ///
    /// # Errors
    /// [`ParseError::InvalidSfnt`] for a bad header, table, or zlib stream.
    #[cfg(feature = "woff")]
    pub fn from_woff(data: &[u8]) -> Result<Self, ParseError> {
        crate::woff::decode(data)
    }

    /// Decode a WOFF 2.0 container, reversing the glyf/loca/hmtx transforms
    /// (feature `woff2`).
    ///
    /// # Errors
    /// [`ParseError::InvalidSfnt`] for container errors;
    /// [`ParseError::UnsupportedWoff2Transform`] for undefined transform
    /// versions; [`ParseError::InvalidGlyphFormat`] for glyph-transform
    /// inconsistencies.
    #[cfg(feature = "woff2")]
    pub fn from_woff2(data: &[u8]) -> Result<Self, ParseError> {
        crate::woff2::decode(data)
    }

    /// Extract font `index` from a TTC collection into a standalone SFNT.
    ///
    /// # Errors
    /// [`ParseError::InvalidSfnt`] for a bad TTC header;
    /// [`ParseError::CollectionIndexOutOfBounds`] when `index` is beyond the
    /// declared font count.
    pub fn from_collection(data: &[u8], index: usize) -> Result<Self, ParseError> {
        let collection = FontCollection::parse(data)?;
        let font = collection.font(index)?;
        let mut tables_to_build: Vec<(RawTag, Vec<u8>)> = Vec::new();
        for (tag, _checksum, offset, length) in font.table_directory() {
            let off = usize::try_from(offset).map_err(|_| ParseError::InvalidSfnt)?;
            let len = usize::try_from(length).map_err(|_| ParseError::InvalidSfnt)?;
            let bytes = crate::reader::slice_at(font.data, off, len, tag)?;
            tables_to_build.push((tag, bytes.to_vec()));
        }
        let flavor = font.sfnt_version();
        build_sfnt(flavor, &mut tables_to_build).map(|data| Self { data })
    }

    /// The zero-copy view over the owned buffer.
    ///
    /// # Errors
    /// [`ParseError::InvalidSfnt`] — structurally impossible (the buffer was
    /// validated at construction), kept for API symmetry with fallible
    /// constructors.
    pub fn as_font_ref(&self) -> Result<FontRef<'_>, ParseError> {
        FontRef::new(&self.data)
    }

    /// The owned buffer.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Consume into the raw buffer.
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }
}

/// A parsed TTC collection: N font directories sharing table data.
#[derive(Debug, Clone)]
pub struct FontCollection<'a> {
    data: &'a [u8],
    offsets: Vec<usize>,
    version: u32,
}

impl<'a> FontCollection<'a> {
    /// Parse and validate the TTC header.
    ///
    /// # Errors
    /// [`ParseError::InvalidSfnt`] for a bad signature or version;
    /// [`ParseError::TruncatedTable`] when the offset array is short.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 12 {
            return Err(ParseError::TruncatedTable {
                tag: TTC_TAG,
                expected: 12,
                got: data.len(),
            });
        }
        let tag = crate::reader::slice_at(data, 0, 4, TTC_TAG)?;
        if tag != TTC_TAG.as_slice() {
            return Err(ParseError::InvalidSfnt);
        }
        let version = crate::reader::be_u32_at(data, 4, TTC_TAG)?;
        let num = crate::reader::be_u32_at(data, 8, TTC_TAG)?;
        let num = usize::try_from(num).map_err(|_| ParseError::InvalidSfnt)?;
        let end = 12usize
            .checked_add(num.checked_mul(4).ok_or(ParseError::InvalidSfnt)?)
            .ok_or(ParseError::InvalidSfnt)?;
        if data.len() < end {
            return Err(ParseError::TruncatedTable {
                tag: TTC_TAG,
                expected: end,
                got: data.len(),
            });
        }
        let mut offsets = Vec::with_capacity(num);
        for i in 0..num {
            let off = crate::reader::be_u32_at(data, 12 + i * 4, TTC_TAG)?;
            let off = usize::try_from(off).map_err(|_| ParseError::InvalidSfnt)?;
            offsets.push(off);
        }
        Ok(Self {
            data,
            offsets,
            version,
        })
    }

    /// The TTC version (`0x00010000` or `0x00020000`).
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Number of fonts in the collection.
    #[must_use]
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// True when the collection declares no fonts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Font `index` as a zero-copy reference over the whole collection
    /// buffer (TTC table offsets are file-relative, so sharing works).
    ///
    /// # Errors
    /// [`ParseError::CollectionIndexOutOfBounds`] beyond the font count;
    /// [`ParseError::InvalidSfnt`] / [`ParseError::TruncatedTable`] for a
    /// malformed directory.
    pub fn font(&self, index: usize) -> Result<FontRef<'a>, ParseError> {
        let dir_offset =
            *self
                .offsets
                .get(index)
                .ok_or(ParseError::CollectionIndexOutOfBounds {
                    index,
                    count: self.offsets.len(),
                })?;
        let sub = self.data.get(dir_offset..).ok_or(ParseError::InvalidSfnt)?;
        let mut r = Reader::new(sub, TTC_TAG);
        let version = r.u32()?;
        if !is_known_sfnt_version(version) {
            return Err(ParseError::InvalidSfnt);
        }
        let num_tables = r.u16()?;
        if num_tables == 0 {
            return Err(ParseError::InvalidSfnt);
        }
        r.skip(6)?;
        validate_directory(self.data, dir_offset, num_tables)?;
        Ok(FontRef {
            data: self.data,
            dir_offset,
            num_tables,
            sfnt_version: version,
        })
    }

    /// All fonts (see [`font`](Self::font)).
    ///
    /// # Errors
    /// First error from any font directory.
    pub fn fonts(&self) -> Result<Vec<FontRef<'a>>, ParseError> {
        let mut out = Vec::with_capacity(self.offsets.len());
        for i in 0..self.offsets.len() {
            out.push(self.font(i)?);
        }
        Ok(out)
    }

    /// One-shot parse of a TTC buffer into font references.
    ///
    /// # Errors
    /// See [`FontCollection::parse`] / [`fonts`](Self::fonts).
    pub fn fonts_in(data: &'a [u8]) -> Result<Vec<FontRef<'a>>, ParseError> {
        Self::parse(data)?.fonts()
    }
}

/// Validate a table directory (at `dir_offset` in `data`) entry by entry.
fn validate_directory(data: &[u8], dir_offset: usize, num_tables: u16) -> Result<(), ParseError> {
    let dir_end = dir_offset
        .checked_add(12 + usize::from(num_tables) * ENTRY)
        .ok_or(ParseError::InvalidSfnt)?;
    if data.len() < dir_end {
        return Err(ParseError::TruncatedTable {
            tag: SFNT_TAG,
            expected: dir_end,
            got: data.len(),
        });
    }
    for i in 0..usize::from(num_tables) {
        let at = dir_offset + 12 + i * ENTRY;
        let offset = crate::reader::be_u32_at(data, at + 8, SFNT_TAG)?;
        let length = crate::reader::be_u32_at(data, at + 12, SFNT_TAG)?;
        let offset = usize::try_from(offset).map_err(|_| ParseError::InvalidSfnt)?;
        let length = usize::try_from(length).map_err(|_| ParseError::InvalidSfnt)?;
        let end = offset.checked_add(length).ok_or(ParseError::InvalidSfnt)?;
        if end > data.len() {
            let mut tag: RawTag = *b"????";
            if let Some(found) = data.get(at..at + 4) {
                tag.copy_from_slice(found);
            }
            return Err(ParseError::TruncatedTable {
                tag,
                expected: end,
                got: data.len(),
            });
        }
    }
    Ok(())
}

/// The variable-font view: `fvar` axes and instances plus optional `avar`
/// segment maps and `gvar` glyph deltas.
#[derive(Debug, Clone, Copy)]
pub struct VariableFont<'a> {
    font: FontRef<'a>,
    fvar: tables::FvarTable<'a>,
    avar: Option<tables::AvarTable<'a>>,
    gvar: Option<tables::GvarTable<'a>>,
}

impl<'a> VariableFont<'a> {
    /// Design axes (tag, min, default, max, flags, name id).
    ///
    /// # Errors
    /// Truncated axis array.
    pub fn axes(&self) -> Result<Vec<tables::VariationAxis>, ParseError> {
        self.fvar.axes()
    }

    /// Named instances (name id + per-axis coordinates).
    ///
    /// # Errors
    /// Truncated instance array.
    pub fn instances(&self) -> Result<Vec<tables::VarInstance>, ParseError> {
        self.fvar.instances()
    }

    /// The `avar` table, when present.
    #[must_use]
    pub const fn avar(&self) -> Option<tables::AvarTable<'a>> {
        self.avar
    }

    /// The `gvar` table, when present.
    #[must_use]
    pub const fn gvar(&self) -> Option<tables::GvarTable<'a>> {
        self.gvar
    }

    /// The underlying font reference.
    #[must_use]
    pub const fn font(&self) -> FontRef<'a> {
        self.font
    }

    /// Normalize a user coordinate to the [-1, 1] (`F2Dot14`) design-space
    /// scale, applying `avar` segment mapping when present.
    ///
    /// # Errors
    /// [`ParseError::InvalidGlyphFormat`] for an out-of-range axis index;
    /// truncated `avar` maps otherwise.
    pub fn normalize(&self, axis: usize, value: font_types::Fixed) -> Result<F2Dot14, ParseError> {
        let axes = self.fvar.axes()?;
        let axis_rec = axes.get(axis).ok_or(ParseError::InvalidGlyphFormat)?;
        let min = axis_rec.min.to_f64();
        let def = axis_rec.default.to_f64();
        let max = axis_rec.max.to_f64();
        let v = value.to_f64();
        // Spec normalization: lower half then upper half, clamped to [-1, 1].
        // Degenerate halves pin to the extreme (fontTools varLib convention).
        let normalized: f64 = if v < def {
            if (def - min).abs() < f64::EPSILON {
                -1.0
            } else {
                (v - def) / (def - min)
            }
        } else if (max - def).abs() < f64::EPSILON {
            1.0
        } else {
            (v - def) / (max - def)
        };
        // Round half away from zero without std: the float->int `as` cast
        // truncates toward zero, and core has no f64::round/trunc.
        let scaled = normalized.clamp(-1.0, 1.0) * 16_384.0;
        let shifted = if scaled < 0.0 {
            scaled - 0.5
        } else {
            scaled + 0.5
        };
        let bits_wide = shifted as i64; // in [-16385, 16385); truncation = round
        let bits = i16::try_from(bits_wide).map_err(|_| ParseError::InvalidGlyphFormat)?;
        let coord = F2Dot14::from_bits(bits);
        match self.avar {
            Some(avar) => {
                let maps = avar.segment_maps()?;
                Ok(maps.get(axis).map_or(coord, |m| m.map(coord)))
            }
            None => Ok(coord),
        }
    }
}

/// Assemble a standalone SFNT from `(tag, bytes)` pairs: sorted directory,
/// computed checksums, and a correct `head` checkSumAdjustment.
pub(crate) fn build_sfnt(
    flavor: u32,
    tables_in: &mut [(RawTag, Vec<u8>)],
) -> Result<Vec<u8>, ParseError> {
    if tables_in.is_empty() {
        return Err(ParseError::InvalidSfnt);
    }
    tables_in.sort_by_key(|t| t.0);
    let n = tables_in.len();
    let entry_selector = 31 - (n as u32).leading_zeros(); // floor(log2(n))
    let search_range: u32 = 16 << entry_selector;
    let range_shift = n as u32 * 16 - search_range;

    let dir_len = 12 + n * ENTRY;
    let mut out = Vec::with_capacity(dir_len + 1024);
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&(search_range as u16).to_be_bytes());
    out.extend_from_slice(&(entry_selector as u16).to_be_bytes());
    out.extend_from_slice(&(range_shift as u16).to_be_bytes());

    // Offsets: directory first, then 4-byte aligned tables.
    let mut offset: u32 = dir_len as u32;
    let mut entries = Vec::with_capacity(n);
    for (tag, bytes) in tables_in.iter() {
        entries.push((*tag, table_checksum(bytes), offset, bytes.len() as u32));
        // Pad so the NEXT table starts 4-aligned (i.e. align this table's end).
        let end = offset
            .checked_add(bytes.len() as u32)
            .ok_or(ParseError::InvalidSfnt)?;
        let pad = (4 - (end % 4)) % 4;
        offset = end.checked_add(pad).ok_or(ParseError::InvalidSfnt)?;
    }
    for (tag, checksum, off, len) in &entries {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&off.to_be_bytes());
        out.extend_from_slice(&len.to_be_bytes());
    }
    for (_, bytes) in tables_in.iter() {
        out.extend_from_slice(bytes);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    // head.checkSumAdjustment: zero the field, checksum the whole font,
    // store 0xB1B0AFBA - total.
    let head_off = entries
        .iter()
        .find(|(t, _, _, _)| t == b"head")
        .map(|(_, _, off, _)| *off as usize);
    if let Some(head_at) = head_off {
        set_bytes(&mut out, head_at + 8, &[0, 0, 0, 0])?;
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(table_checksum(&out));
        set_bytes(&mut out, head_at + 8, &adjustment.to_be_bytes())?;
    }
    Ok(out)
}

/// Write 4 bytes at `at`, bounds-checked (no panics).
fn set_bytes(out: &mut [u8], at: usize, bytes: &[u8; 4]) -> Result<(), ParseError> {
    out.get_mut(at..at + 4)
        .ok_or(ParseError::InvalidSfnt)?
        .copy_from_slice(bytes);
    Ok(())
}

/// SFNT table checksum: u32 sum of big-endian words, zero-padded to 4.
pub(crate) fn table_checksum(bytes: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let mut chunks = bytes.chunks_exact(4);
    for c in chunks.by_ref() {
        if let Ok(word) = <[u8; 4]>::try_from(c) {
            sum = sum.wrapping_add(u32::from_be_bytes(word));
        }
    }
    let rest = chunks.remainder();
    if !rest.is_empty() {
        let mut last = [0u8; 4];
        for (i, &b) in rest.iter().enumerate() {
            if let Some(slot) = last.get_mut(i) {
                *slot = b;
            }
        }
        sum = sum.wrapping_add(u32::from_be_bytes(last));
    }
    sum
}
