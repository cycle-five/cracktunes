//! The typed topology model. Everything written to disk goes through these types.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// `crate id + module path + name`, joined with `::`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnitId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Lib,
    Bin,
    ProcMacro,
    Test,
    Example,
    Bench,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Module,
    Function,
    Method,
    Struct,
    Enum,
    Union,
    Trait,
    TypeAlias,
    Const,
    Static,
    MacroRules,
    MacroCall,
    Impl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    Signature,
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Clean,
    Signature,
    Body,
    Inherited,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Loc {
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Generator {
    pub tool_hash: String,
    pub rust_analyzer: String,
    pub config_hash: String,
    pub ra_error_lines: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrateInfo {
    pub id: String,
    pub package: String,
    pub kind: TargetKind,
    pub root_file: String,
    pub test_only: bool,
    pub lines: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub kind: UnitKind,
    #[serde(rename = "crate")]
    pub krate: String,
    pub module: Option<UnitId>,
    pub parent: Option<UnitId>,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub lines: u32,
    pub own_lines: u32,
    pub test: bool,
    pub features: BTreeSet<String>,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: UnitId,
    pub to: UnitId,
    pub position: Position,
    pub count: u32,
    pub first: Loc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalRef {
    pub from: UnitId,
    pub package: String,
    pub symbol: String,
    pub position: Position,
    pub count: u32,
    pub first: Loc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reexport {
    pub module: UnitId,
    pub target: String,
    pub first: Loc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub package: String,
    pub symbol: String,
    pub position: Position,
    pub count: u32,
    pub first: Loc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitTaint {
    pub state: State,
    pub cuts: u32,
    pub direct: Vec<Evidence>,
    pub via: Vec<UnitId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateLines {
    pub clean: u32,
    pub signature: u32,
    pub body: u32,
    pub inherited: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleEdge {
    pub from: UnitId,
    pub to: UnitId,
    pub count: u32,
    pub first: Loc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cycle {
    #[serde(rename = "crate")]
    pub krate: String,
    pub modules: Vec<UnitId>,
    pub edges: Vec<ModuleEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cycles {
    pub by_module: Vec<Cycle>,
    pub by_top_module: Vec<Cycle>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WideStruct {
    pub id: UnitId,
    pub fields: Vec<String>,
    /// field -> module -> units
    pub uses: BTreeMap<String, BTreeMap<UnitId, BTreeSet<UnitId>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Orphan {
    pub package: String,
    pub file: String,
    pub lines: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    UnindexedFile,
    UnparsedFile,
    OccurrenceOutsideTree,
    UnmappedDefinition,
    UnmappedReference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Warning {
    pub kind: WarningKind,
    pub message: String,
    pub loc: Option<Loc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topology {
    pub generator: Generator,
    pub name: String,
    pub groups: Vec<String>,
    pub crates: Vec<CrateInfo>,
    pub units: Vec<Unit>,
    pub edges: Vec<Edge>,
    pub external: Vec<ExternalRef>,
    pub reexports: Vec<Reexport>,
    pub imports: BTreeMap<UnitId, BTreeSet<String>>,
    pub taint: BTreeMap<String, BTreeMap<UnitId, UnitTaint>>,
    pub module_lines: BTreeMap<String, BTreeMap<UnitId, StateLines>>,
    pub cycles: Cycles,
    pub wide_structs: Vec<WideStruct>,
    pub module_groups: BTreeMap<UnitId, BTreeMap<String, u32>>,
    pub orphans: Vec<Orphan>,
    pub warnings: Vec<Warning>,
}
