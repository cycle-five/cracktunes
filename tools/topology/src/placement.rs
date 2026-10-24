//! Places SCIP occurrences into the units `syn` found.
//!
//! Definitions map a symbol to the unit (or field, or module) that defines it;
//! references are then attributed to the innermost unit containing them.

use crate::model::{Loc, Position, Reexport, UnitId, UnitKind, Warning, WarningKind};
use crate::scip::{Occurrence, ScipIndex};
use crate::structure::{FieldDef, Pos, Structure, UnitDraft, UseSpan};
use crate::symbol::Symbol;
use crate::workspace::Workspace;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// What a reference points at.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefTarget {
    Unit(UnitId),
    Module(UnitId),
    Field { owner: UnitId, field: String },
    External { package: String, symbol: String },
}

/// One reference occurrence placed inside a unit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawRef {
    pub from: UnitId,
    pub target: RefTarget,
    pub position: Position,
    pub loc: Loc,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlacementStats {
    pub definitions_exact: u32,
    pub definitions_contained: u32,
    pub definitions_unmapped: u32,
    pub references: u32,
    pub references_unmapped: u32,
    /// A symbol defined twice in one crate with different mappings.
    pub definitions_ambiguous: u32,
    /// Non-local symbols `Symbol::parse` rejected.
    pub symbols_unparsed: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Placed {
    pub refs: Vec<RawRef>,
    /// A `pub use` target is the unit id of a workspace symbol, or
    /// `"<package> <descriptors>"` for an external one.
    pub reexports: Vec<Reexport>,
    pub imports: BTreeMap<UnitId, BTreeSet<String>>,
    pub warnings: Vec<Warning>,
    pub stats: PlacementStats,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Mapped {
    Unit(UnitId),
    Module(UnitId),
    Field { owner: UnitId, name: String },
}

/// One place a symbol is defined. A lib and a bin of one package share symbols,
/// so a symbol can have one `Def` per crate.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Def {
    krate: String,
    mapped: Mapped,
}

/// Places every occurrence; only packages that are not workspace members are external.
pub fn place(structure: &Structure, index: &ScipIndex, ws: &Workspace) -> Placed {
    place_with(structure, index, ws, &BTreeSet::new())
}

/// Like [`place`], but the packages in `external` are also treated as external
/// even when they are workspace members (a configured dependency group).
pub fn place_with(
    structure: &Structure,
    index: &ScipIndex,
    ws: &Workspace,
    external: &BTreeSet<String>,
) -> Placed {
    Placer::new(structure, ws, external).run(index)
}

struct Placer<'a> {
    structure: &'a Structure,
    ws: &'a Workspace,
    external: &'a BTreeSet<String>,
    units_by_file: HashMap<&'a str, Vec<&'a UnitDraft>>,
    kinds: BTreeMap<&'a UnitId, UnitKind>,
    unit_at: HashMap<(&'a str, Pos), &'a UnitDraft>,
    field_at: HashMap<(&'a str, Pos), &'a FieldDef>,
    uses_by_file: HashMap<&'a str, Vec<&'a UseSpan>>,
    lib_of: HashMap<&'a str, &'a str>,
    defs: HashMap<String, Vec<Def>>,
    out: Placed,
}

/// The text of one file, split into lines.
struct Lines<'a>(Vec<&'a str>);

impl<'a> Lines<'a> {
    fn line(&self, line0: u32) -> &'a str {
        self.0.get(line0 as usize).copied().unwrap_or("")
    }

    /// 0-based line and UTF-8 byte column to a `Pos` (1-based line, char column).
    fn pos(&self, line0: u32, byte_col: u32) -> Pos {
        let bytes = self.line(line0).as_bytes();
        let prefix = &bytes[..(byte_col as usize).min(bytes.len())];
        Pos {
            line: line0 + 1,
            col: String::from_utf8_lossy(prefix).chars().count() as u32,
        }
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The path segment written just before `col`, when it is joined by `::`.
fn segment_before(line: &str, col: usize) -> Option<&str> {
    let head = line.get(..col)?.strip_suffix("::")?;
    let start = head
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_ident_char(c))
        .last()
        .map_or(head.len(), |(i, _)| i);
    Some(&head[start..])
}

/// Tuple-struct fields (`Pair#0.`) have no definition occurrence of their own;
/// returns the length of the owner's symbol (`Pair#`).
fn tuple_field_owner(symbol: &str) -> Option<usize> {
    let body = symbol.strip_suffix('.')?;
    let digits = body.len() - body.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    (digits > 0 && body[..body.len() - digits].ends_with('#')).then_some(body.len() - digits)
}

fn module_id(krate: &str, path: &[String]) -> UnitId {
    let mut id = krate.to_string();
    for part in path {
        id.push_str("::");
        id.push_str(part);
    }
    UnitId(id)
}

impl<'a> Placer<'a> {
    fn new(structure: &'a Structure, ws: &'a Workspace, external: &'a BTreeSet<String>) -> Self {
        let mut units_by_file: HashMap<&str, Vec<&UnitDraft>> = HashMap::new();
        let mut kinds = BTreeMap::new();
        let mut unit_at = HashMap::new();
        for u in &structure.units {
            units_by_file.entry(&u.unit.file).or_default().push(u);
            kinds.insert(&u.unit.id, u.unit.kind);
            if let Some(p) = u.ident {
                unit_at.insert((u.unit.file.as_str(), p), u);
            }
        }
        let field_at = structure
            .fields
            .iter()
            .map(|f| ((f.file.as_str(), f.ident), f))
            .collect();
        let mut uses_by_file: HashMap<&str, Vec<&UseSpan>> = HashMap::new();
        for u in &structure.uses {
            uses_by_file.entry(&u.file).or_default().push(u);
        }
        let mut lib_of = HashMap::new();
        for p in &ws.packages {
            use crate::model::TargetKind::{Lib, ProcMacro};
            if let Some(t) = p.targets.iter().find(|t| matches!(t.kind, Lib | ProcMacro)) {
                lib_of.insert(p.name.as_str(), t.crate_id.as_str());
            }
        }
        Placer {
            structure,
            ws,
            external,
            units_by_file,
            kinds,
            unit_at,
            field_at,
            uses_by_file,
            lib_of,
            defs: HashMap::new(),
            out: Placed::default(),
        }
    }

    fn is_external(&self, package: &str) -> bool {
        !self.ws.is_member(package) || self.external.contains(package)
    }

    fn run(mut self, index: &ScipIndex) -> Placed {
        let indexed: BTreeSet<&str> = index.documents.iter().map(|d| d.path.as_str()).collect();
        for path in self.structure.files.keys() {
            if !indexed.contains(path.as_str()) {
                self.out.warnings.push(Warning {
                    kind: WarningKind::UnindexedFile,
                    message: format!("{path} has no SCIP document"),
                    loc: Some(Loc {
                        file: path.clone(),
                        line: 1,
                    }),
                });
            }
        }
        for doc in &index.documents {
            let Some(info) = self.structure.files.get(&doc.path) else {
                let n = doc
                    .occurrences
                    .iter()
                    .filter(|o| Symbol::parse(&o.symbol).is_some())
                    .count();
                if n > 0 {
                    self.out.warnings.push(Warning {
                        kind: WarningKind::OccurrenceOutsideTree,
                        message: format!(
                            "{} occurrences in {}, which is not in the module tree",
                            n, doc.path
                        ),
                        loc: Some(Loc {
                            file: doc.path.clone(),
                            line: 1,
                        }),
                    });
                }
                continue;
            };
            let lines = Lines(info.text.lines().collect());
            for occ in doc.occurrences.iter().filter(|o| o.definition) {
                self.define(&doc.path, &info.krate, &lines, occ);
            }
        }
        for doc in &index.documents {
            let Some(info) = self.structure.files.get(&doc.path) else {
                continue;
            };
            let lines = Lines(info.text.lines().collect());
            for occ in doc.occurrences.iter().filter(|o| !o.definition) {
                self.reference(&doc.path, &info.krate, &lines, occ);
            }
        }
        let out = &mut self.out;
        out.refs.sort();
        out.reexports.sort_by(|a, b| {
            (&a.module, &a.target, &a.first).cmp(&(&b.module, &b.target, &b.first))
        });
        out.warnings
            .sort_by(|a, b| (&a.loc, a.kind, &a.message).cmp(&(&b.loc, b.kind, &b.message)));
        self.out
    }

    /// Records a non-local symbol that did not parse; locals are skipped silently.
    fn unparsed(&mut self, file: &str, line0: u32, symbol: &str) {
        if symbol.starts_with("local ") {
            return;
        }
        self.out.stats.symbols_unparsed += 1;
        self.out.warnings.push(Warning {
            kind: WarningKind::UnparsedSymbol,
            message: format!("cannot parse symbol {symbol:?}"),
            loc: Some(Loc {
                file: file.into(),
                line: line0 + 1,
            }),
        });
    }

    /// The innermost unit whose span contains `pos`.
    fn innermost(&self, file: &str, pos: Pos) -> Option<&'a UnitDraft> {
        self.units_by_file
            .get(file)?
            .iter()
            .filter(|u| u.span.contains(pos))
            .max_by_key(|u| (u.span.start, Reverse(u.span.end)))
            .copied()
    }

    fn mapped_unit(&self, u: &UnitDraft) -> Mapped {
        if u.unit.kind == UnitKind::Module {
            Mapped::Module(u.unit.id.clone())
        } else {
            Mapped::Unit(u.unit.id.clone())
        }
    }

    fn define(&mut self, file: &str, krate: &str, lines: &Lines, occ: &Occurrence) {
        let Some(sym) = Symbol::parse(&occ.symbol) else {
            self.unparsed(file, occ.start_line, &occ.symbol);
            return;
        };
        if !self.ws.is_member(&sym.package) {
            return;
        }
        let pos = lines.pos(occ.start_line, occ.start_col);
        let module = sym
            .module_path()
            .map(|p| module_id(krate, &p))
            .filter(|id| self.kinds.contains_key(id));
        let mapped = if let Some(id) = module {
            self.out.stats.definitions_exact += 1;
            Some(Mapped::Module(id))
        } else if let Some(u) = self.unit_at.get(&(file, pos)) {
            self.out.stats.definitions_exact += 1;
            Some(self.mapped_unit(u))
        } else if let Some(f) = self.field_at.get(&(file, pos)) {
            self.out.stats.definitions_exact += 1;
            Some(Mapped::Field {
                owner: f.owner.clone(),
                name: f.name.clone(),
            })
        } else if let Some(u) = self.innermost(file, pos) {
            self.out.stats.definitions_contained += 1;
            Some(self.mapped_unit(u))
        } else {
            None
        };
        let Some(mapped) = mapped else {
            self.out.stats.definitions_unmapped += 1;
            self.out.warnings.push(Warning {
                kind: WarningKind::UnmappedDefinition,
                message: format!("no unit contains the definition of {}", occ.symbol),
                loc: Some(Loc {
                    file: file.into(),
                    line: pos.line,
                }),
            });
            return;
        };
        let def = Def {
            krate: krate.into(),
            mapped,
        };
        let slot = self.defs.entry(occ.symbol.clone()).or_default();
        if slot.contains(&def) {
            return;
        }
        let clash = slot.iter().any(|d| d.krate == def.krate);
        slot.push(def);
        if clash {
            self.out.stats.definitions_ambiguous += 1;
            self.out.warnings.push(Warning {
                kind: WarningKind::AmbiguousDefinition,
                message: format!(
                    "{} is defined more than once in {krate}; the smallest unit id wins",
                    occ.symbol
                ),
                loc: Some(Loc {
                    file: file.into(),
                    line: pos.line,
                }),
            });
        }
    }

    /// Picks the candidate a reference means; see the module rules in the plan.
    fn resolve(
        &self,
        sym: &Symbol,
        occ: &Occurrence,
        line: &str,
        from_krate: &str,
    ) -> Option<&Def> {
        let cands = self.defs.get(&occ.symbol).or_else(|| {
            self.defs
                .get(&occ.symbol[..tuple_field_owner(&occ.symbol)?])
        })?;
        let lib = self.lib_of.get(sym.package.as_str()).copied();
        let by = |k: &str| {
            cands
                .iter()
                .filter(|d| d.krate == k)
                .min_by(|a, b| a.mapped.cmp(&b.mapped))
        };
        let start = occ.start_col as usize;
        let names_lib = lib.is_some_and(|l| {
            if sym.module_path().is_some_and(|p| p.is_empty()) {
                occ.start_line == occ.end_line && line.get(start..occ.end_col as usize) == Some(l)
            } else {
                segment_before(line, start) == Some(l)
            }
        });
        if names_lib && let Some(d) = lib.and_then(by) {
            return Some(d);
        }
        by(from_krate).or_else(|| lib.and_then(by))
    }

    fn reference(&mut self, file: &str, file_krate: &str, lines: &Lines, occ: &Occurrence) {
        let Some(sym) = Symbol::parse(&occ.symbol) else {
            self.unparsed(file, occ.start_line, &occ.symbol);
            return;
        };
        let pos = lines.pos(occ.start_line, occ.start_col);
        let line = lines.line(occ.start_line);
        let loc = Loc {
            file: file.into(),
            line: pos.line,
        };
        self.out.stats.references += 1;

        let use_span = self
            .uses_by_file
            .get(file)
            .and_then(|v| v.iter().find(|u| u.span.contains(pos)))
            .copied();
        if let Some(u) = use_span {
            let external = self.is_external(&sym.package);
            if !u.public {
                if external {
                    self.out
                        .imports
                        .entry(u.module.clone())
                        .or_default()
                        .insert(sym.package.clone());
                }
                return;
            }
            // `a` in `pub use a::B;` is only a path prefix, not what is re-exported;
            // in `pub use a::*;` it is the only thing the glob names.
            let rest = line.get(occ.end_col as usize..).unwrap_or("");
            if let Some(after) = rest.strip_prefix("::")
                && !after.trim_start().starts_with('*')
            {
                return;
            }
            let target = if external {
                Some(format!("{} {}", sym.package, sym.descriptors))
            } else {
                self.resolve(&sym, occ, line, file_krate)
                    .map(|d| match &d.mapped {
                        Mapped::Unit(id) | Mapped::Module(id) => id.0.clone(),
                        Mapped::Field { owner, name } => format!("{}.{name}", owner.0),
                    })
            };
            match target {
                Some(target) => self.out.reexports.push(Reexport {
                    module: u.module.clone(),
                    target,
                    first: loc,
                }),
                None => self.unmapped_reference(occ, loc),
            }
            return;
        }

        let Some(from) = self.innermost(file, pos) else {
            self.unmapped_reference(occ, loc);
            return;
        };
        let position = if from.body.is_some_and(|b| b.contains(pos)) {
            Position::Body
        } else {
            Position::Signature
        };
        let target = if self.is_external(&sym.package) {
            Some(RefTarget::External {
                package: sym.package.clone(),
                symbol: sym.descriptors.clone(),
            })
        } else {
            self.resolve(&sym, occ, line, &from.unit.krate)
                .map(|d| match &d.mapped {
                    Mapped::Unit(id) => RefTarget::Unit(id.clone()),
                    Mapped::Module(id) => RefTarget::Module(id.clone()),
                    Mapped::Field { owner, name } => RefTarget::Field {
                        owner: owner.clone(),
                        field: name.clone(),
                    },
                })
        };
        match target {
            Some(target) => self.out.refs.push(RawRef {
                from: from.unit.id.clone(),
                target,
                position,
                loc,
            }),
            None => self.unmapped_reference(occ, loc),
        }
    }

    fn unmapped_reference(&mut self, occ: &Occurrence, loc: Loc) {
        self.out.stats.references_unmapped += 1;
        self.out.warnings.push(Warning {
            kind: WarningKind::UnmappedReference,
            message: format!("no unit defines {}", occ.symbol),
            loc: Some(loc),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_columns_become_char_columns() {
        let l = Lines(vec!["", "a🎵b"]);
        assert_eq!(l.pos(1, 0), Pos { line: 2, col: 0 });
        assert_eq!(l.pos(1, 5), Pos { line: 2, col: 2 });
        assert_eq!(l.pos(1, 6), Pos { line: 2, col: 3 });
        assert_eq!(l.pos(1, 99), Pos { line: 2, col: 3 });
        assert_eq!(l.pos(7, 3), Pos { line: 8, col: 0 });
    }

    #[test]
    fn segment_before_reads_the_path_prefix() {
        let line = "let _ = run() + appbin::run();";
        assert_eq!(segment_before(line, 24), Some("appbin"));
        assert_eq!(segment_before(line, 8), None);
        assert_eq!(segment_before("a::b::c", 6), Some("b"));
        assert_eq!(segment_before("::c", 2), Some(""));
        assert_eq!(segment_before("x::é", 2), None);
    }

    #[test]
    fn tuple_field_owner_cases() {
        assert_eq!(tuple_field_owner("p 1 a/Pair#0."), Some(11));
        assert_eq!(tuple_field_owner("p 1 a/Pair#12."), Some(11));
        assert_eq!(tuple_field_owner("p 1 a/Pair#f1."), None);
        assert_eq!(tuple_field_owner("p 1 a/Pair#"), None);
        assert_eq!(tuple_field_owner("p 1 a/Pair#."), None);
        assert_eq!(tuple_field_owner("p 1 a/f0."), None);
        assert_eq!(tuple_field_owner("p 1 a/Pair#0"), None);
    }

    #[test]
    fn module_ids_join_with_double_colons() {
        assert_eq!(module_id("c", &[]), UnitId("c".into()));
        assert_eq!(
            module_id("c", &["a".into(), "b".into()]),
            UnitId("c::a::b".into())
        );
    }
}
