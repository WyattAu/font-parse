//! Typed table parsers — zero-copy views over raw table bytes.
//!
//! Each view borrows the table's bytes (`&'a [u8]`) and exposes accessors
//! that read fields on demand: table access allocates nothing. The views die
//! with the borrow and can never outlive the buffer.

pub mod cmap;
pub mod dsig;
pub mod fvar;
pub mod glyf;
pub mod gvar;
pub mod head;
pub mod hhea;
pub mod hmtx;
pub mod layout;
pub mod loca;
pub mod maxp;
pub mod name;
pub mod os2;
pub mod post;
pub mod stat;

pub use cmap::{Cmap, CmapSubtable, EncodingRecord};
pub use dsig::DsigTable;
pub use fvar::{AvarTable, FvarTable, VarInstance, VariationAxis};
pub use glyf::{
    Component, CompositeGlyph, Contour, GlyphOutline, GlyphRef, OutlinePoint, SimpleGlyph,
};
pub use gvar::{GvarTable, TupleVariation};
pub use head::HeadTable;
pub use hhea::HheaTable;
pub use hmtx::HmtxTable;
pub use layout::{
    Feature, FeatureList, LangSys, LayoutKind, LayoutTable, Lookup, LookupList, Script, ScriptList,
};
pub use loca::LocaTable;
pub use maxp::MaxpTable;
pub use name::{NameRecord, NameTable};
pub use os2::Os2Table;
pub use post::PostTable;
pub use stat::{AxisValue, StatAxis, StatTable};
