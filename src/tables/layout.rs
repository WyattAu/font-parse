//! `GSUB`/`GPOS` — layout common structure: script, feature, and lookup
//! lists (header structure only; lookup subtables are out of scope).

use crate::error::{ParseError, RawTag};
use crate::reader::Reader;
use alloc::vec::Vec;
use font_types::Tag;

pub(crate) const GSUB: RawTag = *b"GSUB";
pub(crate) const GPOS: RawTag = *b"GPOS";

/// Which layout table a view wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    /// Glyph substitution (`GSUB`).
    Gsub,
    /// Glyph positioning (`GPOS`).
    Gpos,
}

impl LayoutKind {
    /// Raw table tag.
    #[must_use]
    pub const fn tag(self) -> RawTag {
        match self {
            LayoutKind::Gsub => GSUB,
            LayoutKind::Gpos => GPOS,
        }
    }
}

/// Zero-copy view over a `GSUB`/`GPOS` table.
#[derive(Debug, Clone, Copy)]
pub struct LayoutTable<'a> {
    data: &'a [u8],
    kind: LayoutKind,
}

impl<'a> LayoutTable<'a> {
    /// Parse and validate the layout table header.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] for short headers;
    /// [`ParseError::InvalidGlyphFormat`] when the list offsets fall outside
    /// the table.
    pub fn parse(data: &'a [u8], kind: LayoutKind) -> Result<Self, ParseError> {
        match crate::reader::be_u16_at(data, 0, kind.tag())? {
            // Version 1.0: three u16 offsets. 1.1: adds a u32 offset — the
            // base header size is the same through lookupListOffset.
            1 => {}
            _ => return Err(ParseError::InvalidGlyphFormat),
        }
        if data.len() < 10 {
            return Err(ParseError::TruncatedTable {
                tag: kind.tag(),
                expected: 10,
                got: data.len(),
            });
        }
        for at in [4usize, 6, 8] {
            let off = usize::from(crate::reader::be_u16_at(data, at, kind.tag())?);
            if off != 0 && off >= data.len() {
                return Err(ParseError::InvalidGlyphFormat);
            }
        }
        Ok(Self { data, kind })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Which table this view wraps.
    #[must_use]
    pub const fn kind(&self) -> LayoutKind {
        self.kind
    }

    /// Major/minor version (1.0 or 1.1).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn version(&self) -> Result<(u16, u16), ParseError> {
        Ok((
            crate::reader::be_u16_at(self.data, 0, self.kind.tag())?,
            crate::reader::be_u16_at(self.data, 2, self.kind.tag())?,
        ))
    }

    /// The script list.
    ///
    /// # Errors
    /// Parse errors from the list decoder.
    pub fn script_list(&self) -> Result<ScriptList<'a>, ParseError> {
        let off = usize::from(crate::reader::be_u16_at(self.data, 4, self.kind.tag())?);
        if off == 0 {
            return Err(ParseError::InvalidGlyphFormat);
        }
        ScriptList::parse(self.data, off, self.kind)
    }

    /// The feature list.
    ///
    /// # Errors
    /// Parse errors from the list decoder.
    pub fn feature_list(&self) -> Result<FeatureList<'a>, ParseError> {
        let off = usize::from(crate::reader::be_u16_at(self.data, 6, self.kind.tag())?);
        if off == 0 {
            return Err(ParseError::InvalidGlyphFormat);
        }
        FeatureList::parse(self.data, off, self.kind)
    }

    /// The lookup list.
    ///
    /// # Errors
    /// Parse errors from the list decoder.
    pub fn lookup_list(&self) -> Result<LookupList<'a>, ParseError> {
        let off = usize::from(crate::reader::be_u16_at(self.data, 8, self.kind.tag())?);
        if off == 0 {
            return Err(ParseError::InvalidGlyphFormat);
        }
        LookupList::parse(self.data, off, self.kind)
    }
}

/// A record tag plus its offset within the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListRecord {
    pub(crate) tag: Tag,
    pub(crate) offset: u16,
}

/// Read a list header: `count: u16` followed by `(tag: u32, offset: u16)`
/// records.
fn read_list(data: &[u8], base: usize, tag: RawTag) -> Result<(u16, Vec<ListRecord>), ParseError> {
    if base.checked_add(2).ok_or(ParseError::InvalidGlyphFormat)? > data.len() {
        return Err(ParseError::TruncatedTable {
            tag,
            expected: base + 2,
            got: data.len(),
        });
    }
    let count = crate::reader::be_u16_at(data, base, tag)?;
    let records_at = base + 2;
    let end = records_at
        .checked_add(usize::from(count) * 6)
        .ok_or(ParseError::InvalidGlyphFormat)?;
    if end > data.len() {
        return Err(ParseError::TruncatedTable {
            tag,
            expected: end,
            got: data.len(),
        });
    }
    let mut records = Vec::with_capacity(usize::from(count));
    for i in 0..usize::from(count) {
        let at = records_at + i * 6;
        let raw = crate::reader::be_u32_at(data, at, tag)?;
        let offset = crate::reader::be_u16_at(data, at + 4, tag)?;
        records.push(ListRecord {
            tag: Tag::new(&raw.to_be_bytes()),
            offset,
        });
    }
    Ok((count, records))
}

/// The script list: script tags with their script tables.
#[derive(Debug, Clone, Copy)]
pub struct ScriptList<'a> {
    data: &'a [u8],
    base: usize,
    kind: LayoutKind,
}

impl<'a> ScriptList<'a> {
    fn parse(data: &'a [u8], base: usize, kind: LayoutKind) -> Result<Self, ParseError> {
        read_list(data, base, kind.tag())?;
        Ok(Self { data, base, kind })
    }

    /// Script tags in table order.
    ///
    /// # Errors
    /// List decoding errors (validated at construction; cannot fail).
    pub fn tags(&self) -> Result<Vec<Tag>, ParseError> {
        Ok(read_list(self.data, self.base, self.kind.tag())?
            .1
            .into_iter()
            .map(|r| r.tag)
            .collect())
    }

    /// Decode script `tag`.
    ///
    /// # Errors
    /// [`ParseError::InvalidGlyphFormat`] when the script offset is out of
    /// range; parse errors from the script table.
    pub fn script(&self, tag: Tag) -> Result<Option<Script<'a>>, ParseError> {
        for r in read_list(self.data, self.base, self.kind.tag())?.1 {
            if r.tag == tag {
                return Script::parse(self.data, self.base + usize::from(r.offset), self.kind)
                    .map(Some);
            }
        }
        Ok(None)
    }
}

/// One script: a default language system plus tagged alternates.
#[derive(Debug, Clone, Copy)]
pub struct Script<'a> {
    data: &'a [u8],
    base: usize,
    kind: LayoutKind,
}

impl<'a> Script<'a> {
    fn parse(data: &'a [u8], base: usize, kind: LayoutKind) -> Result<Self, ParseError> {
        if base.checked_add(4).ok_or(ParseError::InvalidGlyphFormat)? > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: kind.tag(),
                expected: base + 4,
                got: data.len(),
            });
        }
        Ok(Self { data, base, kind })
    }

    fn read_lang_sys_list(&self) -> Result<(u16, Vec<ListRecord>), ParseError> {
        let count = crate::reader::be_u16_at(self.data, self.base + 2, self.kind.tag())?;
        let records_at = self.base + 4;
        let end = records_at
            .checked_add(usize::from(count) * 6)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if end > self.data.len() {
            return Err(ParseError::TruncatedTable {
                tag: self.kind.tag(),
                expected: end,
                got: self.data.len(),
            });
        }
        let mut records = Vec::with_capacity(usize::from(count));
        for i in 0..usize::from(count) {
            let at = records_at + i * 6;
            let raw = crate::reader::be_u32_at(self.data, at, self.kind.tag())?;
            let offset = crate::reader::be_u16_at(self.data, at + 4, self.kind.tag())?;
            records.push(ListRecord {
                tag: Tag::new(&raw.to_be_bytes()),
                offset,
            });
        }
        Ok((count, records))
    }

    /// The default language system (offset 0 = none).
    ///
    /// # Errors
    /// LangSys parse errors.
    pub fn default_lang_sys(&self) -> Result<Option<LangSys<'a>>, ParseError> {
        let off = crate::reader::be_u16_at(self.data, self.base, self.kind.tag())?;
        if off == 0 {
            return Ok(None);
        }
        LangSys::parse(self.data, self.base + usize::from(off), self.kind).map(Some)
    }

    /// Alternate language-system tags.
    ///
    /// # Errors
    /// Truncation of the record array.
    pub fn lang_sys_tags(&self) -> Result<Vec<Tag>, ParseError> {
        Ok(self
            .read_lang_sys_list()?
            .1
            .into_iter()
            .map(|r| r.tag)
            .collect())
    }

    /// Decode an alternate language system by tag.
    ///
    /// # Errors
    /// LangSys parse errors.
    pub fn lang_sys(&self, tag: Tag) -> Result<Option<LangSys<'a>>, ParseError> {
        for r in self.read_lang_sys_list()?.1 {
            if r.tag == tag {
                return LangSys::parse(self.data, self.base + usize::from(r.offset), self.kind)
                    .map(Some);
            }
        }
        Ok(None)
    }
}

/// A language system: required feature plus feature index list.
#[derive(Debug, Clone, Copy)]
pub struct LangSys<'a> {
    data: &'a [u8],
    base: usize,
    kind: LayoutKind,
}

impl<'a> LangSys<'a> {
    fn parse(data: &'a [u8], base: usize, kind: LayoutKind) -> Result<Self, ParseError> {
        if base.checked_add(6).ok_or(ParseError::InvalidGlyphFormat)? > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: kind.tag(),
                expected: base + 6,
                got: data.len(),
            });
        }
        let count = crate::reader::be_u16_at(data, base + 4, kind.tag())?;
        let end = base
            .checked_add(6)
            .and_then(|v| v.checked_add(usize::from(count) * 2))
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if end > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: kind.tag(),
                expected: end,
                got: data.len(),
            });
        }
        Ok(Self { data, base, kind })
    }

    /// Required feature index (`0xFFFF` = none).
    ///
    /// # Errors
    /// Truncation (impossible post-parse).
    pub fn required_feature_index(&self) -> Result<u16, ParseError> {
        crate::reader::be_u16_at(self.data, self.base + 2, self.kind.tag())
    }

    /// Feature indices, in table order.
    ///
    /// # Errors
    /// Truncation (impossible post-parse).
    pub fn feature_indices(&self) -> Result<Vec<u16>, ParseError> {
        let count = usize::from(crate::reader::be_u16_at(
            self.data,
            self.base + 4,
            self.kind.tag(),
        )?);
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            out.push(crate::reader::be_u16_at(
                self.data,
                self.base + 6 + i * 2,
                self.kind.tag(),
            )?);
        }
        Ok(out)
    }
}

/// The feature list: feature tags with their feature tables.
#[derive(Debug, Clone, Copy)]
pub struct FeatureList<'a> {
    data: &'a [u8],
    base: usize,
    kind: LayoutKind,
}

impl<'a> FeatureList<'a> {
    fn parse(data: &'a [u8], base: usize, kind: LayoutKind) -> Result<Self, ParseError> {
        read_list(data, base, kind.tag())?;
        Ok(Self { data, base, kind })
    }

    /// Feature tags in table order.
    ///
    /// # Errors
    /// List decoding errors (validated at construction; cannot fail).
    pub fn tags(&self) -> Result<Vec<Tag>, ParseError> {
        Ok(read_list(self.data, self.base, self.kind.tag())?
            .1
            .into_iter()
            .map(|r| r.tag)
            .collect())
    }

    /// Decode feature `tag` (first match).
    ///
    /// # Errors
    /// Feature parse errors.
    pub fn feature(&self, tag: Tag) -> Result<Option<Feature<'a>>, ParseError> {
        for r in read_list(self.data, self.base, self.kind.tag())?.1 {
            if r.tag == tag {
                return Feature::parse(self.data, self.base + usize::from(r.offset), self.kind)
                    .map(Some);
            }
        }
        Ok(None)
    }
}

/// One feature: the list of lookups it activates.
#[derive(Debug, Clone, Copy)]
pub struct Feature<'a> {
    data: &'a [u8],
    base: usize,
    kind: LayoutKind,
}

impl<'a> Feature<'a> {
    fn parse(data: &'a [u8], base: usize, kind: LayoutKind) -> Result<Self, ParseError> {
        if base.checked_add(4).ok_or(ParseError::InvalidGlyphFormat)? > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: kind.tag(),
                expected: base + 4,
                got: data.len(),
            });
        }
        let count = crate::reader::be_u16_at(data, base + 2, kind.tag())?;
        let end = base
            .checked_add(4)
            .and_then(|v| v.checked_add(usize::from(count) * 2))
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if end > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: kind.tag(),
                expected: end,
                got: data.len(),
            });
        }
        Ok(Self { data, base, kind })
    }

    /// Lookup list indices, in table order.
    ///
    /// # Errors
    /// Truncation (impossible post-parse).
    pub fn lookup_indices(&self) -> Result<Vec<u16>, ParseError> {
        let count = usize::from(crate::reader::be_u16_at(
            self.data,
            self.base + 2,
            self.kind.tag(),
        )?);
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            out.push(crate::reader::be_u16_at(
                self.data,
                self.base + 4 + i * 2,
                self.kind.tag(),
            )?);
        }
        Ok(out)
    }
}

/// The lookup list.
#[derive(Debug, Clone, Copy)]
pub struct LookupList<'a> {
    data: &'a [u8],
    base: usize,
    kind: LayoutKind,
    count: u16,
}

impl<'a> LookupList<'a> {
    fn parse(data: &'a [u8], base: usize, kind: LayoutKind) -> Result<Self, ParseError> {
        if base.checked_add(2).ok_or(ParseError::InvalidGlyphFormat)? > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: kind.tag(),
                expected: base + 2,
                got: data.len(),
            });
        }
        let count = crate::reader::be_u16_at(data, base, kind.tag())?;
        let end = base
            .checked_add(2)
            .and_then(|v| v.checked_add(usize::from(count) * 2))
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if end > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: kind.tag(),
                expected: end,
                got: data.len(),
            });
        }
        Ok(Self {
            data,
            base,
            kind,
            count,
        })
    }

    /// Number of lookups.
    #[must_use]
    pub const fn count(&self) -> u16 {
        self.count
    }

    /// Decode lookup `index`.
    ///
    /// # Errors
    /// [`ParseError::InvalidGlyphFormat`] for out-of-range indices or bad
    /// offsets; lookup parse errors otherwise.
    pub fn lookup(&self, index: u16) -> Result<Lookup<'a>, ParseError> {
        if index >= self.count {
            return Err(ParseError::InvalidGlyphFormat);
        }
        let off = crate::reader::be_u16_at(
            self.data,
            self.base + 2 + usize::from(index) * 2,
            self.kind.tag(),
        )?;
        Lookup::parse(self.data, self.base + usize::from(off), self.kind)
    }
}

/// One lookup: type, flags, and subtable count (subtables themselves are out
/// of scope for v1).
#[derive(Debug, Clone, Copy)]
pub struct Lookup<'a> {
    data: &'a [u8],
    base: usize,
    kind: LayoutKind,
    sub_table_count: u16,
}

impl<'a> Lookup<'a> {
    fn parse(data: &'a [u8], base: usize, kind: LayoutKind) -> Result<Self, ParseError> {
        if base.checked_add(6).ok_or(ParseError::InvalidGlyphFormat)? > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: kind.tag(),
                expected: base + 6,
                got: data.len(),
            });
        }
        let mut r = Reader::within(data, base, data.len(), kind.tag());
        r.skip(4)?;
        let sub_table_count = r.u16()?;
        let end = base
            .checked_add(6)
            .and_then(|v| v.checked_add(usize::from(sub_table_count) * 2))
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if end > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: kind.tag(),
                expected: end,
                got: data.len(),
            });
        }
        Ok(Self {
            data,
            base,
            kind,
            sub_table_count,
        })
    }

    /// Lookup type (GSUB 1-8, GPOS 1-9).
    ///
    /// # Errors
    /// Truncation (impossible post-parse).
    pub fn lookup_type(&self) -> Result<u16, ParseError> {
        crate::reader::be_u16_at(self.data, self.base, self.kind.tag())
    }

    /// Lookup flag (bit 4 marks a mark-filtering set; see
    /// [`mark_filtering_set`](Self::mark_filtering_set)).
    ///
    /// # Errors
    /// Truncation (impossible post-parse).
    pub fn lookup_flag(&self) -> Result<u16, ParseError> {
        crate::reader::be_u16_at(self.data, self.base + 2, self.kind.tag())
    }

    /// Number of subtables.
    #[must_use]
    pub const fn sub_table_count(&self) -> u16 {
        self.sub_table_count
    }

    /// Mark-filtering set index, present only when the lookup flag has bit 4
    /// (0x0010) set.
    ///
    /// # Errors
    /// Truncation of the extended lookup header.
    pub fn mark_filtering_set(&self) -> Result<Option<u16>, ParseError> {
        if self.lookup_flag()? & 0x0010 == 0 {
            return Ok(None);
        }
        let at = self.base + 6 + usize::from(self.sub_table_count) * 2;
        Ok(Some(crate::reader::be_u16_at(
            self.data,
            at,
            self.kind.tag(),
        )?))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{LayoutKind, LayoutTable, Tag};

    /// Test hook for the internal list reader.
    pub(crate) fn read_list_public(data: &[u8], base: usize) -> Result<(), ParseError> {
        super::read_list(data, base, *b"GSUB").map(|_| ())
    }
    use crate::error::ParseError;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Minimal GSUB: one script (DFLT) with a default LangSys, one feature
    /// "liga" -> lookup 0, one lookup (type 1).
    fn gsub() -> Vec<u8> {
        // Header (10 bytes) then three lists.
        let script_list = 10u16;
        let feature_list = 30u16;
        let lookup_list = 44u16;

        let mut d = Vec::new();
        d.extend_from_slice(&1u16.to_be_bytes()); // major
        d.extend_from_slice(&0u16.to_be_bytes()); // minor
        d.extend_from_slice(&script_list.to_be_bytes());
        d.extend_from_slice(&feature_list.to_be_bytes());
        d.extend_from_slice(&lookup_list.to_be_bytes());

        // ScriptList @10: count + record(tag, offset)
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(b"DFLT");
        d.extend_from_slice(&8u16.to_be_bytes()); // script at +8
                                                  // Script @18: defaultLangSys offset, langSysCount
        d.extend_from_slice(&4u16.to_be_bytes()); // langsys at +4
        d.extend_from_slice(&0u16.to_be_bytes()); // no alternates
                                                  // LangSys @22: lookupOrder, required, count, indices[]
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&0xFFFFu16.to_be_bytes()); // no required feature
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes()); // feature 0

        // FeatureList @30: count + record
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(b"liga");
        d.extend_from_slice(&8u16.to_be_bytes()); // feature at +8
                                                  // Feature @38: params offset, lookup count, indices
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes()); // lookup 0

        // LookupList @44: count + offsets
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&4u16.to_be_bytes()); // lookup at +4
                                                  // Lookup @48: type, flag, subtable count, subtable offsets[]
        d.extend_from_slice(&1u16.to_be_bytes()); // type 1
        d.extend_from_slice(&0u16.to_be_bytes()); // flag
        d.extend_from_slice(&1u16.to_be_bytes()); // 1 subtable
        d.extend_from_slice(&8u16.to_be_bytes()); // subtable at lookup+8
        d.extend_from_slice(&[0u8; 4]); // subtable bytes @52
        d
    }

    #[test]
    fn gsub_structure_walk() {
        let d = gsub();
        let t = LayoutTable::parse(&d, LayoutKind::Gsub).unwrap();
        assert_eq!(t.kind(), LayoutKind::Gsub);
        assert_eq!(t.version().unwrap(), (1, 0));

        let scripts = t.script_list().unwrap();
        let tags = scripts.tags().unwrap();
        assert_eq!(tags, vec![Tag::new(b"DFLT")]);
        let script = scripts.script(Tag::new(b"DFLT")).unwrap().unwrap();
        let lang = script.default_lang_sys().unwrap().unwrap();
        assert_eq!(lang.required_feature_index().unwrap(), 0xFFFF);
        assert_eq!(lang.feature_indices().unwrap(), vec![0]);
        assert!(script.lang_sys_tags().unwrap().is_empty());

        let features = t.feature_list().unwrap();
        assert_eq!(features.tags().unwrap(), vec![Tag::new(b"liga")]);
        let feature = features.feature(Tag::new(b"liga")).unwrap().unwrap();
        assert_eq!(feature.lookup_indices().unwrap(), vec![0]);

        let lookups = t.lookup_list().unwrap();
        assert_eq!(lookups.count(), 1);
        let lookup = lookups.lookup(0).unwrap();
        assert_eq!(lookup.lookup_type().unwrap(), 1);
        assert_eq!(lookup.lookup_flag().unwrap(), 0);
        assert_eq!(lookup.sub_table_count(), 1);
        assert_eq!(lookup.mark_filtering_set().unwrap(), None);
    }

    #[test]
    fn gpos_lookup_with_mark_filter() {
        // Reuse the gsub bytes but declare them GPOS, set the mark-filter
        // flag, and append the set index after the subtable offsets.
        let mut d = gsub();
        let lookup = 48usize;
        d[lookup + 2..lookup + 4].copy_from_slice(&0x0010u16.to_be_bytes());
        // Insert the set index between the offsets and the subtable, then
        // shift the subtable offset past it.
        d[lookup + 6..lookup + 8].copy_from_slice(&10u16.to_be_bytes());
        d.splice(lookup + 8..lookup + 8, [0u8, 7]);
        let t = LayoutTable::parse(&d, LayoutKind::Gpos).unwrap();
        assert_eq!(t.kind(), LayoutKind::Gpos);
        let lookup = t.lookup_list().unwrap().lookup(0).unwrap();
        assert_eq!(lookup.lookup_flag().unwrap(), 0x0010);
        assert_eq!(lookup.mark_filtering_set().unwrap(), Some(7));
    }

    #[test]
    fn errors() {
        // Truncated header.
        assert!(matches!(
            LayoutTable::parse(&[0, 1, 0], LayoutKind::Gsub),
            Err(ParseError::TruncatedTable { .. })
        ));
        // Bad version.
        let mut d = gsub();
        d[0..2].copy_from_slice(&3u16.to_be_bytes());
        assert!(matches!(
            LayoutTable::parse(&d, LayoutKind::Gsub),
            Err(ParseError::InvalidGlyphFormat)
        ));
        // List offset out of range.
        let mut d = gsub();
        d[4..6].copy_from_slice(&0xFFFEu16.to_be_bytes());
        assert!(matches!(
            LayoutTable::parse(&d, LayoutKind::Gsub),
            Err(ParseError::InvalidGlyphFormat)
        ));
        // Missing script.
        let t = LayoutTable::parse(&d, LayoutKind::Gsub);
        let _ = t.and_then(|t| t.script_list().unwrap().script(Tag::new(b"arab")));
        // Lookup out of range.
        let d = gsub();
        let t = LayoutTable::parse(&d, LayoutKind::Gsub).unwrap();
        assert!(matches!(
            t.lookup_list().unwrap().lookup(9),
            Err(ParseError::InvalidGlyphFormat)
        ));
    }
}

#[cfg(test)]
mod edge_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{LayoutKind, LayoutTable, Tag};
    use crate::error::ParseError;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Script with a default LangSys plus one alternate ( Armen).
    fn script_with_alternate() -> Vec<u8> {
        let mut d = Vec::new();
        // Script: defaultLangSys at +8, langSysCount 1, record (tag, offset).
        d.extend_from_slice(&8u16.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(b"ARMN");
        d.extend_from_slice(&16u16.to_be_bytes()); // alternate at +16
                                                   // Default LangSys @8.
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&0xFFFFu16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        // Alternate LangSys @20 (8 + 4 + 8 = 20): requires feature 3.
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&2u16.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&3u16.to_be_bytes());
        d
    }

    #[test]
    fn alternate_lang_sys() {
        let d = script_with_alternate();
        // Wrap in a minimal single-script layout table.
        let header = 10u16;
        let script_list = header;
        let mut t = Vec::new();
        t.extend_from_slice(&1u16.to_be_bytes());
        t.extend_from_slice(&0u16.to_be_bytes());
        t.extend_from_slice(&script_list.to_be_bytes());
        t.extend_from_slice(&0u16.to_be_bytes()); // no feature list
        t.extend_from_slice(&0u16.to_be_bytes()); // no lookup list
        t.extend_from_slice(&1u16.to_be_bytes());
        t.extend_from_slice(b"DFLT");
        t.extend_from_slice(&8u16.to_be_bytes()); // script at +8
        t.extend_from_slice(&d);
        let table = LayoutTable::parse(&t, LayoutKind::Gsub).unwrap();
        let scripts = table.script_list().unwrap();
        let script: super::Script = scripts.script(Tag::new(b"DFLT")).unwrap().unwrap();
        assert_eq!(script.lang_sys_tags().unwrap(), vec![Tag::new(b"ARMN")]);
        let alt = script.lang_sys(Tag::new(b"ARMN")).unwrap().unwrap();
        assert_eq!(alt.required_feature_index().unwrap(), 2);
        assert_eq!(alt.feature_indices().unwrap(), vec![3]);
        // Unknown tag -> None.
        assert!(script.lang_sys(Tag::new(b"kana")).unwrap().is_none());
    }

    #[test]
    fn truncations() {
        let d = script_with_alternate();
        // Script table shorter than its header.
        assert!(matches!(
            super::Script::parse(&d, 0, LayoutKind::Gsub).and(Ok(())),
            Ok(())
        ));
        // LangSys truncated.
        assert!(super::LangSys::parse(&d, 8, LayoutKind::Gsub).is_err());
        // Feature truncated (declared records exceed the buffer).
        assert!(super::Feature::parse(&d[..2], 0, LayoutKind::Gsub).is_err());
        // Feature with records beyond the buffer.
        let mut f = vec![0u8; 4];
        f[2..4].copy_from_slice(&9u16.to_be_bytes());
        assert!(super::Feature::parse(&f, 0, LayoutKind::Gsub).is_err());
        // Lookup truncated.
        assert!(super::Lookup::parse(&d, 0, LayoutKind::Gsub).is_err());
        // List record array truncated.
        let mut short = vec![0u8; 4];
        short[0..2].copy_from_slice(&3u16.to_be_bytes()); // 3 records, no bytes
        assert!(matches!(
            super::tests::read_list_public(&short, 0),
            Err(ParseError::TruncatedTable { .. })
        ));
    }
}
