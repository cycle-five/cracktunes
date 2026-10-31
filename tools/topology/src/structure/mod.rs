//! Structure from `syn`: module trees, units, regions, hashes and orphans.
//!
//! Positions are `Pos { line: 1-based, col: 0-based chars }`, exactly what
//! `proc-macro2` reports, so Task 4 can place SCIP occurrences by position.

mod hash;
mod items;
mod tree;

use crate::model::{CrateInfo, Orphan, Unit, UnitId, Warning};
use std::collections::BTreeMap;

pub use tree::read;

/// A source position. Ordering is by line, then column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    /// 1-based.
    pub line: u32,
    /// 0-based, in chars.
    pub col: u32,
}

/// A source range; `end` is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

impl Span {
    pub fn contains(&self, p: Pos) -> bool {
        self.start <= p && p < self.end
    }
}

/// A unit plus the positions Task 4 needs to place occurrences into it.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitDraft {
    pub unit: Unit,
    /// Where the unit's name is written (`None` for impls, macro calls and file modules).
    pub ident: Option<Pos>,
    /// The whole item, outer attributes included.
    pub span: Span,
    /// A fn's block, a const/static's expression, a macro call's tokens.
    pub body: Option<Span>,
}

/// A named field of a struct or union.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub owner: UnitId,
    pub name: String,
    pub file: String,
    pub ident: Pos,
}

/// One per module-level `use` item.
#[derive(Debug, Clone, PartialEq)]
pub struct UseSpan {
    pub module: UnitId,
    pub file: String,
    pub span: Span,
    pub public: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub krate: String,
    pub module: UnitId,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Structure {
    pub crates: Vec<CrateInfo>,
    pub units: Vec<UnitDraft>,
    pub fields: Vec<FieldDef>,
    pub uses: Vec<UseSpan>,
    pub files: BTreeMap<String, FileInfo>,
    pub orphans: Vec<Orphan>,
    pub warnings: Vec<Warning>,
}
