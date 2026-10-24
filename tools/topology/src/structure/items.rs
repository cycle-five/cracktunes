//! Turns parsed files into units: ids, spans, regions, flags, hashes.

use super::hash::hash_tokens;
use super::{FieldDef, FileInfo, Pos, Span, Structure, UnitDraft, UseSpan};
use crate::model::{Loc, Unit, UnitId, UnitKind, Warning, WarningKind};
use proc_macro2::{LineColumn, TokenStream, TokenTree};
use quote::ToTokens;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use syn::ext::IdentExt;
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Fields, ImplItem, Item, ItemImpl, ItemMod, ItemTrait, MacroDelimiter, Meta, Token,
    TraitItem, Type, Visibility,
};

pub(super) fn pos(lc: LineColumn) -> Pos {
    Pos {
        line: lc.line as u32,
        col: lc.column as u32,
    }
}

fn span_pm(s: proc_macro2::Span) -> Span {
    Span {
        start: pos(s.start()),
        end: pos(s.end()),
    }
}

/// First token's start to last token's end, so outer attributes are inside.
fn span_of(t: &impl ToTokens) -> Span {
    let toks: Vec<TokenTree> = t.to_token_stream().into_iter().collect();
    match (toks.first(), toks.last()) {
        (Some(a), Some(b)) => Span {
            start: pos(a.span().start()),
            end: pos(b.span().end()),
        },
        _ => Span {
            start: Pos { line: 1, col: 0 },
            end: Pos { line: 1, col: 0 },
        },
    }
}

fn ident_pos(i: &syn::Ident) -> Pos {
    pos(i.span().start())
}

/// The last line an exclusive span end touches.
fn last_line(s: Span) -> u32 {
    if s.end.col == 0 && s.end.line > s.start.line {
        s.end.line - 1
    } else {
        s.end.line
    }
}

/// `test` and `features` as inherited down the item tree.
#[derive(Debug, Clone, Default)]
pub(super) struct Inherit {
    pub test: bool,
    pub features: BTreeSet<String>,
}

impl Inherit {
    fn with(&self, attrs: &[Attribute]) -> Inherit {
        let mut next = self.clone();
        for a in attrs {
            let last = a.path().segments.last().map(|s| s.ident.to_string());
            if last.as_deref() == Some("test") {
                next.test = true;
            }
            if a.path().is_ident("cfg") {
                if let Ok(m) = a.parse_args::<Meta>()
                    && requires_test(&m)
                {
                    next.test = true;
                }
                scan_features(a.meta.to_token_stream(), &mut next.features);
            } else if a.path().is_ident("cfg_attr") {
                scan_features(a.meta.to_token_stream(), &mut next.features);
            }
        }
        next
    }
}

/// Does this `cfg` predicate hold only when `test` is set?
fn requires_test(m: &Meta) -> bool {
    match m {
        Meta::Path(p) => p.is_ident("test"),
        Meta::List(l) => {
            let args = l
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .ok();
            let Some(args) = args else { return false };
            if l.path.is_ident("all") {
                args.iter().any(requires_test)
            } else if l.path.is_ident("any") {
                !args.is_empty() && args.iter().all(requires_test)
            } else {
                false
            }
        },
        Meta::NameValue(_) => false,
    }
}

/// Every `feature = "…"` anywhere in the tokens, `not(…)` included.
fn scan_features(ts: TokenStream, out: &mut BTreeSet<String>) {
    let toks: Vec<TokenTree> = ts.into_iter().collect();
    for (i, t) in toks.iter().enumerate() {
        match t {
            TokenTree::Group(g) => scan_features(g.stream(), out),
            TokenTree::Ident(id) if id == "feature" => {
                if let (Some(TokenTree::Punct(eq)), Some(TokenTree::Literal(l))) =
                    (toks.get(i + 1), toks.get(i + 2))
                    && eq.as_char() == '='
                    && let Ok(s) = syn::parse2::<syn::LitStr>(TokenTree::Literal(l.clone()).into())
                {
                    out.insert(s.value());
                }
            },
            _ => {},
        }
    }
}

/// Context for the items of one module.
pub(super) struct ModCtx {
    pub id: UnitId,
    pub file: String,
    /// Directory in which `mod x;` declared here looks for `x.rs` / `x/mod.rs`.
    pub dir: String,
    /// Inside an inline `mod a { … }` in the file.
    pub in_inline: bool,
    pub inh: Inherit,
}

struct Draft {
    base: String,
    kind: UnitKind,
    module: Option<UnitId>,
    parent: Option<UnitId>,
    file: String,
    span: Span,
    ident: Option<Pos>,
    body: Option<Span>,
    inh: Inherit,
    tokens: TokenStream,
}

pub(super) struct Collector<'a> {
    pub(super) root: Option<&'a Path>,
    pub(super) out: Structure,
    pub(super) visited: BTreeSet<String>,
    pub(super) krate: String,
    crate_test: bool,
    counts: HashMap<String, u32>,
}

fn type_name(ty: &Type) -> String {
    match ty {
        Type::Path(p) if p.qself.is_none() => p
            .path
            .segments
            .last()
            .map(|s| s.ident.unraw().to_string())
            .unwrap_or_default(),
        Type::Paren(p) => type_name(&p.elem),
        Type::Group(g) => type_name(&g.elem),
        other => other.to_token_stream().to_string().replace(' ', ""),
    }
}

fn macro_delim_span(d: &MacroDelimiter) -> Span {
    let ds = match d {
        MacroDelimiter::Paren(p) => p.span,
        MacroDelimiter::Brace(b) => b.span,
        MacroDelimiter::Bracket(b) => b.span,
    };
    span_pm(ds.join())
}

impl<'a> Collector<'a> {
    pub(super) fn new(root: Option<&'a Path>) -> Self {
        Collector {
            root,
            out: Structure::default(),
            visited: BTreeSet::new(),
            krate: String::new(),
            crate_test: false,
            counts: HashMap::new(),
        }
    }

    /// Starts a crate: ids are made unique within it.
    pub(super) fn begin_crate(&mut self, id: &str, test: bool) {
        self.krate = id.to_string();
        self.crate_test = test;
        self.counts.clear();
    }

    pub(super) fn warn(&mut self, kind: WarningKind, message: String, loc: Option<Loc>) {
        self.out.warnings.push(Warning { kind, message, loc });
    }

    fn add(&mut self, d: Draft) -> UnitId {
        let n = self.counts.entry(d.base.clone()).or_insert(0);
        *n += 1;
        let id = if *n == 1 {
            d.base
        } else {
            format!("{}#{}", d.base, n)
        };
        let id = UnitId(id);
        let lines = last_line(d.span) - d.span.start.line + 1;
        self.out.units.push(UnitDraft {
            unit: Unit {
                id: id.clone(),
                kind: d.kind,
                krate: self.krate.clone(),
                module: d.module,
                parent: d.parent,
                file: d.file,
                start_line: d.span.start.line,
                end_line: last_line(d.span),
                lines,
                own_lines: lines,
                test: d.inh.test,
                features: d.inh.features,
                hash: hash_tokens(d.tokens),
            },
            ident: d.ident,
            span: d.span,
            body: d.body,
        });
        id
    }

    /// Adds a file's module unit and everything in it. `parent` is the module
    /// that declares it (`None` for a crate root); `name` is its name there.
    pub(super) fn add_file_module(
        &mut self,
        file: &str,
        text: String,
        parent: Option<(&ModCtx, &str, &[Attribute])>,
        dir: String,
    ) {
        let ast = match syn::parse_file(&text) {
            Ok(a) => a,
            Err(e) => {
                let line = e.span().start().line as u32;
                self.warn(
                    WarningKind::UnparsedFile,
                    format!("{file}: {e}"),
                    Some(Loc {
                        file: file.to_string(),
                        line,
                    }),
                );
                return;
            },
        };
        let (id, module, mut inh) = match parent {
            None => (
                self.krate.clone(),
                None,
                Inherit {
                    test: self.crate_test,
                    features: BTreeSet::new(),
                },
            ),
            Some((p, name, decl)) => (
                format!("{}::{}", p.id.0, name),
                Some(p.id.clone()),
                p.inh.with(decl),
            ),
        };
        inh = inh.with(&ast.attrs);
        let end = end_of(&text);
        let span = Span {
            start: Pos { line: 1, col: 0 },
            end,
        };
        let uid = self.add(Draft {
            base: id,
            kind: UnitKind::Module,
            module,
            parent: None,
            file: file.to_string(),
            span,
            ident: None,
            body: None,
            inh: inh.clone(),
            tokens: ast.to_token_stream(),
        });
        self.out.files.insert(
            file.to_string(),
            FileInfo {
                krate: self.krate.clone(),
                module: uid.clone(),
                text,
            },
        );
        let ctx = ModCtx {
            id: uid,
            file: file.to_string(),
            dir,
            in_inline: false,
            inh,
        };
        self.items(&ctx, &ast.items);
    }

    pub(super) fn items(&mut self, ctx: &ModCtx, items: &[Item]) {
        for item in items {
            match item {
                Item::Fn(f) => {
                    let inh = ctx.inh.with(&f.attrs);
                    self.leaf(
                        ctx,
                        UnitKind::Function,
                        &f.sig.ident,
                        item,
                        Some(span_of(&f.block)),
                        inh,
                    );
                },
                Item::Struct(s) => {
                    let inh = ctx.inh.with(&s.attrs);
                    let id = self.leaf(ctx, UnitKind::Struct, &s.ident, item, None, inh);
                    self.fields(&id, ctx, &s.fields);
                },
                Item::Union(u) => {
                    let inh = ctx.inh.with(&u.attrs);
                    let id = self.leaf(ctx, UnitKind::Union, &u.ident, item, None, inh);
                    let named = Fields::Named(u.fields.clone());
                    self.fields(&id, ctx, &named);
                },
                Item::Enum(e) => {
                    let inh = ctx.inh.with(&e.attrs);
                    self.leaf(ctx, UnitKind::Enum, &e.ident, item, None, inh);
                },
                Item::Trait(t) => self.trait_item(ctx, item, t),
                Item::Type(t) => {
                    let inh = ctx.inh.with(&t.attrs);
                    self.leaf(ctx, UnitKind::TypeAlias, &t.ident, item, None, inh);
                },
                Item::Const(c) => {
                    let inh = ctx.inh.with(&c.attrs);
                    let body = Some(span_of(&c.expr));
                    self.leaf(ctx, UnitKind::Const, &c.ident, item, body, inh);
                },
                Item::Static(s) => {
                    let inh = ctx.inh.with(&s.attrs);
                    let body = Some(span_of(&s.expr));
                    self.leaf(ctx, UnitKind::Static, &s.ident, item, body, inh);
                },
                Item::Macro(m) => {
                    let inh = ctx.inh.with(&m.attrs);
                    let body = Some(macro_delim_span(&m.mac.delimiter));
                    match &m.ident {
                        Some(name) => {
                            self.leaf(ctx, UnitKind::MacroRules, name, item, body, inh);
                        },
                        None => {
                            let last = m
                                .mac
                                .path
                                .segments
                                .last()
                                .map(|s| s.ident.unraw().to_string())
                                .unwrap_or_default();
                            self.add(Draft {
                                base: format!("{}::{}!", ctx.id.0, last),
                                kind: UnitKind::MacroCall,
                                module: Some(ctx.id.clone()),
                                parent: None,
                                file: ctx.file.clone(),
                                span: span_of(item),
                                ident: None,
                                body,
                                inh,
                                tokens: item.to_token_stream(),
                            });
                        },
                    }
                },
                Item::Impl(i) => self.impl_item(ctx, item, i),
                Item::Mod(m) => self.mod_item(ctx, item, m),
                Item::Use(u) => self.out.uses.push(UseSpan {
                    module: ctx.id.clone(),
                    file: ctx.file.clone(),
                    span: span_of(u),
                    public: !matches!(u.vis, Visibility::Inherited),
                }),
                _ => {},
            }
        }
    }

    /// A unit named `ident` directly in the module.
    fn leaf(
        &mut self,
        ctx: &ModCtx,
        kind: UnitKind,
        ident: &syn::Ident,
        item: &Item,
        body: Option<Span>,
        inh: Inherit,
    ) -> UnitId {
        self.add(Draft {
            base: format!("{}::{}", ctx.id.0, ident.unraw()),
            kind,
            module: Some(ctx.id.clone()),
            parent: None,
            file: ctx.file.clone(),
            span: span_of(item),
            ident: Some(ident_pos(ident)),
            body,
            inh,
            tokens: item.to_token_stream(),
        })
    }

    fn fields(&mut self, owner: &UnitId, ctx: &ModCtx, fields: &Fields) {
        let Fields::Named(named) = fields else { return };
        for f in &named.named {
            if let Some(i) = &f.ident {
                self.out.fields.push(FieldDef {
                    owner: owner.clone(),
                    name: i.unraw().to_string(),
                    file: ctx.file.clone(),
                    ident: ident_pos(i),
                });
            }
        }
    }

    fn trait_item(&mut self, ctx: &ModCtx, item: &Item, t: &ItemTrait) {
        let inh = ctx.inh.with(&t.attrs);
        let tid = self.leaf(ctx, UnitKind::Trait, &t.ident, item, None, inh.clone());
        let prefix = format!("{}::{}", ctx.id.0, t.ident.unraw());
        for ti in &t.items {
            let (kind, ident, attrs, body, span, tokens) = match ti {
                TraitItem::Fn(f) => (
                    UnitKind::Method,
                    &f.sig.ident,
                    &f.attrs,
                    f.default.as_ref().map(span_of),
                    span_of(f),
                    f.to_token_stream(),
                ),
                TraitItem::Const(c) => (
                    UnitKind::Const,
                    &c.ident,
                    &c.attrs,
                    c.default.as_ref().map(|(_, e)| span_of(e)),
                    span_of(c),
                    c.to_token_stream(),
                ),
                TraitItem::Type(a) => (
                    UnitKind::TypeAlias,
                    &a.ident,
                    &a.attrs,
                    None,
                    span_of(a),
                    a.to_token_stream(),
                ),
                _ => continue,
            };
            self.add(Draft {
                base: format!("{prefix}::{}", ident.unraw()),
                kind,
                module: Some(ctx.id.clone()),
                parent: Some(tid.clone()),
                file: ctx.file.clone(),
                span,
                ident: Some(ident_pos(ident)),
                body,
                inh: inh.with(attrs),
                tokens,
            });
        }
    }

    fn impl_item(&mut self, ctx: &ModCtx, item: &Item, i: &ItemImpl) {
        let inh = ctx.inh.with(&i.attrs);
        let self_name = type_name(&i.self_ty);
        let trait_name = i.trait_.as_ref().map(|(_, path, _)| {
            path.segments
                .last()
                .map(|s| s.ident.unraw().to_string())
                .unwrap_or_default()
        });
        let label = match &trait_name {
            Some(t) => format!("<impl {t} for {self_name}>"),
            None => format!("<impl {self_name}>"),
        };
        let iid = self.add(Draft {
            base: format!("{}::{label}", ctx.id.0),
            kind: UnitKind::Impl,
            module: Some(ctx.id.clone()),
            parent: None,
            file: ctx.file.clone(),
            span: span_of(item),
            ident: None,
            body: None,
            inh: inh.clone(),
            tokens: item.to_token_stream(),
        });
        let prefix = match &trait_name {
            Some(t) => format!("{}::<{self_name} as {t}>", ctx.id.0),
            None => format!("{}::{self_name}", ctx.id.0),
        };
        for ii in &i.items {
            let (kind, ident, attrs, body, span, tokens) = match ii {
                ImplItem::Fn(f) => (
                    UnitKind::Method,
                    &f.sig.ident,
                    &f.attrs,
                    Some(span_of(&f.block)),
                    span_of(f),
                    f.to_token_stream(),
                ),
                ImplItem::Const(c) => (
                    UnitKind::Const,
                    &c.ident,
                    &c.attrs,
                    Some(span_of(&c.expr)),
                    span_of(c),
                    c.to_token_stream(),
                ),
                ImplItem::Type(a) => (
                    UnitKind::TypeAlias,
                    &a.ident,
                    &a.attrs,
                    None,
                    span_of(a),
                    a.to_token_stream(),
                ),
                _ => continue,
            };
            self.add(Draft {
                base: format!("{prefix}::{}", ident.unraw()),
                kind,
                module: Some(ctx.id.clone()),
                parent: Some(iid.clone()),
                file: ctx.file.clone(),
                span,
                ident: Some(ident_pos(ident)),
                body,
                inh: inh.with(attrs),
                tokens,
            });
        }
    }

    fn mod_item(&mut self, ctx: &ModCtx, item: &Item, m: &ItemMod) {
        let name = m.ident.unraw().to_string();
        let Some((_, content)) = &m.content else {
            self.file_mod(ctx, &name, m);
            return;
        };
        let inh = ctx.inh.with(&m.attrs);
        let id = self.add(Draft {
            base: format!("{}::{name}", ctx.id.0),
            kind: UnitKind::Module,
            module: Some(ctx.id.clone()),
            parent: None,
            file: ctx.file.clone(),
            span: span_of(item),
            ident: Some(ident_pos(&m.ident)),
            body: None,
            inh: inh.clone(),
            tokens: item.to_token_stream(),
        });
        let inner = ModCtx {
            id,
            file: ctx.file.clone(),
            dir: if ctx.dir.is_empty() {
                name
            } else {
                format!("{}/{name}", ctx.dir)
            },
            in_inline: true,
            inh,
        };
        self.items(&inner, content);
    }

    /// Fills in `own_lines`, sorts, and returns what was collected.
    pub(super) fn finish(mut self) -> Structure {
        let mut kids: std::collections::BTreeMap<UnitId, Vec<(String, u32, u32)>> =
            Default::default();
        for d in &self.out.units {
            let u = &d.unit;
            if let Some(c) = u.parent.as_ref().or(u.module.as_ref()) {
                kids.entry(c.clone())
                    .or_default()
                    .push((u.file.clone(), u.start_line, u.end_line));
            }
        }
        for d in &mut self.out.units {
            let u = &mut d.unit;
            let mut covered = BTreeSet::new();
            for (file, a, b) in kids.get(&u.id).into_iter().flatten() {
                if *file == u.file {
                    covered.extend((*a).max(u.start_line)..=(*b).min(u.end_line));
                }
            }
            u.own_lines = u.lines - covered.len() as u32;
        }
        self.out.units.sort_by(|a, b| a.unit.id.cmp(&b.unit.id));
        self.out
            .fields
            .sort_by(|a, b| (&a.file, a.ident).cmp(&(&b.file, b.ident)));
        self.out
            .uses
            .sort_by(|a, b| (&a.file, a.span.start).cmp(&(&b.file, b.span.start)));
        self.out
    }
}

/// Position just past the last char of the text.
fn end_of(text: &str) -> Pos {
    let line = text.matches('\n').count() as u32 + 1;
    let col = text
        .rsplit('\n')
        .next()
        .map(|l| l.chars().count())
        .unwrap_or(0) as u32;
    Pos { line, col }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub struct Parsed(pub Structure);

    impl Parsed {
        pub fn has(&self, id: &str) -> bool {
            self.0.units.iter().any(|u| u.unit.id.0 == id)
        }
        pub fn get(&self, id: &str) -> &UnitDraft {
            self.0
                .units
                .iter()
                .find(|u| u.unit.id.0 == id)
                .unwrap_or_else(|| panic!("no unit {id}"))
        }
    }

    pub fn parse(src: &str) -> Parsed {
        parse_in(src, false)
    }

    fn parse_in(src: &str, test: bool) -> Parsed {
        let mut c = Collector::new(None);
        c.begin_crate("c", test);
        c.add_file_module("c.rs", src.to_string(), None, String::new());
        Parsed(c.finish())
    }

    fn hash_of(src: &str) -> String {
        parse(src).get("c::f").unit.hash.clone()
    }

    #[test]
    fn ids_for_impls_and_methods() {
        let s = parse(
            "struct Bot; trait T { fn f(&self); fn d(&self) {} } impl Bot { fn ping(&self) {} } impl T for Bot { fn f(&self) {} } impl Bot { fn pong(&self) {} }",
        );
        for id in [
            "c::Bot",
            "c::T",
            "c::T::f",
            "c::T::d",
            "c::<impl Bot>",
            "c::Bot::ping",
            "c::<impl T for Bot>",
            "c::<Bot as T>::f",
            "c::<impl Bot>#2",
            "c::Bot::pong",
        ] {
            assert!(s.has(id), "{id}");
        }
        assert_eq!(
            s.get("c::Bot::ping").unit.parent.as_ref().unwrap().0,
            "c::<impl Bot>"
        );
        assert_eq!(
            s.get("c::Bot::pong").unit.parent.as_ref().unwrap().0,
            "c::<impl Bot>#2"
        );
        assert_eq!(s.get("c::T::d").unit.parent.as_ref().unwrap().0, "c::T");
        assert_eq!(s.get("c::Bot::ping").unit.kind, UnitKind::Method);
        assert_eq!(s.get("c::<impl Bot>").unit.kind, UnitKind::Impl);
        assert_eq!(s.get("c::T").unit.kind, UnitKind::Trait);
        // a required trait method has no body, a provided one has
        assert!(s.get("c::T::f").body.is_none());
        assert!(s.get("c::T::d").body.is_some());
        // methods live in the module, not in the impl
        assert_eq!(s.get("c::Bot::ping").unit.module.as_ref().unwrap().0, "c");
    }

    #[test]
    fn impl_labels_drop_generics_and_keep_odd_types() {
        let s = parse(
            "struct W<T>(T); impl<T> From<T> for W<T> { fn from(t: T) -> Self { W(t) } } impl std::fmt::Display for W<u8> { fn fmt(&self) {} } impl Foo for &str {} impl [u8] {} impl (A) {} impl<T> W<T> { const N: u32 = 1; type X = u8; }",
        );
        for id in [
            "c::<impl From for W>",
            "c::<W as From>::from",
            "c::<impl Display for W>",
            "c::<W as Display>::fmt",
            "c::<impl Foo for &str>",
            "c::<impl [u8]>",
            "c::<impl A>",
            "c::W::N",
            "c::W::X",
        ] {
            assert!(s.has(id), "{id}");
        }
        assert_eq!(s.get("c::W::N").unit.kind, UnitKind::Const);
        assert!(s.get("c::W::N").body.is_some());
        assert_eq!(s.get("c::W::X").unit.kind, UnitKind::TypeAlias);
        assert!(s.get("c::W::X").body.is_none());
    }

    #[test]
    fn only_feature_keys_are_features() {
        let s = parse(
            "#[cfg(target_os = \"linux\")] fn f() {} #[cfg(all(unix, target_os = \"mac\", feature = \"x\"))] fn g() {}",
        );
        assert!(s.get("c::f").unit.features.is_empty());
        let g: Vec<_> = s.get("c::g").unit.features.iter().cloned().collect();
        assert_eq!(g, ["x"]);
    }

    #[test]
    fn trait_consts_and_types_are_units() {
        let s = parse("trait T { const N: u32; const D: u32 = 1; type A; }");
        assert_eq!(s.get("c::T::N").unit.kind, UnitKind::Const);
        assert!(s.get("c::T::N").body.is_none());
        assert!(s.get("c::T::D").body.is_some());
        assert_eq!(s.get("c::T::A").unit.kind, UnitKind::TypeAlias);
        assert_eq!(s.get("c::T::A").unit.parent.as_ref().unwrap().0, "c::T");
    }

    #[test]
    fn qualified_self_types_use_their_tokens() {
        let s = parse("impl Foo for <A as B>::C {}");
        assert!(s.has("c::<impl Foo for <AasB>::C>"));
    }

    #[test]
    fn cfg_alternatives_get_suffixes() {
        let s = parse(
            "#[cfg(feature = \"a\")] fn f() {} #[cfg(not(feature = \"a\"))] fn f() {} fn f() {}",
        );
        assert!(s.has("c::f") && s.has("c::f#2") && s.has("c::f#3"));
        assert!(s.get("c::f").unit.features.contains("a"));
        assert!(s.get("c::f#2").unit.features.contains("a"));
        assert!(s.get("c::f#3").unit.features.is_empty());
        // order follows the source
        assert!(s.get("c::f").span.start < s.get("c::f#2").span.start);
    }

    #[test]
    fn fn_regions_split_signature_and_body() {
        let s = parse("fn f(x: u32) -> u32 {\n    x\n}");
        let d = s.get("c::f");
        let body = d.body.unwrap();
        assert_eq!(body.start, Pos { line: 1, col: 20 });
        assert_eq!(body.end, Pos { line: 3, col: 1 });
        assert!(d.span.start <= body.start);
        assert_eq!(d.span.start, Pos { line: 1, col: 0 });
        assert_eq!(d.span.end, body.end);
        assert_eq!(d.ident, Some(Pos { line: 1, col: 3 }));
        assert_eq!(
            (d.unit.start_line, d.unit.end_line, d.unit.lines),
            (1, 3, 3)
        );
        // the signature is outside the body
        assert!(!body.contains(Pos { line: 1, col: 19 }));
        assert!(body.contains(Pos { line: 1, col: 20 }));
        assert!(body.contains(Pos { line: 3, col: 0 }));
        assert!(!body.contains(Pos { line: 3, col: 1 }));
    }

    #[test]
    fn attributes_and_docs_are_inside_the_span() {
        let s = parse("/// doc\n#[inline]\npub fn f() {}\n");
        let d = s.get("c::f");
        assert_eq!(d.span.start, Pos { line: 1, col: 0 });
        assert_eq!(d.unit.start_line, 1);
        assert_eq!(d.ident, Some(Pos { line: 3, col: 7 }));
    }

    #[test]
    fn hash_ignores_docs_and_whitespace() {
        assert_eq!(hash_of("/// a\nfn f() { 1 }"), hash_of("fn f()   {\n 1 }"));
        assert_ne!(hash_of("fn f() { 1 }"), hash_of("fn f() { 2 }"));
    }

    #[test]
    fn nested_fn_is_part_of_body() {
        let s = parse("fn outer() { fn inner() {} struct S; impl S { fn m() {} } }");
        assert!(!s.has("c::inner") && !s.has("c::outer::inner"));
        assert!(!s.has("c::S") && !s.has("c::S::m"));
        assert_eq!(s.0.units.len(), 2); // the file module and `outer`
    }

    #[test]
    fn raw_identifiers_are_unraw() {
        let s = parse("fn r#type() {} struct r#struct; impl r#struct { fn r#fn(&self) {} }");
        assert!(s.has("c::type") && s.has("c::struct") && s.has("c::struct::fn"));
        assert!(s.has("c::<impl struct>"));
    }

    #[test]
    fn item_kinds() {
        let s = parse(
            "struct A; enum B {} union U { a: u32 } type T = u8; const C: u8 = 1; static S: u8 = 2; macro_rules! m { () => {} } lazy! { x } mod i { fn g() {} } use a::b; pub use c::d;",
        );
        for (id, k) in [
            ("c::A", UnitKind::Struct),
            ("c::B", UnitKind::Enum),
            ("c::U", UnitKind::Union),
            ("c::T", UnitKind::TypeAlias),
            ("c::C", UnitKind::Const),
            ("c::S", UnitKind::Static),
            ("c::m", UnitKind::MacroRules),
            ("c::lazy!", UnitKind::MacroCall),
            ("c::i", UnitKind::Module),
            ("c::i::g", UnitKind::Function),
        ] {
            assert_eq!(s.get(id).unit.kind, k, "{id}");
        }
        assert!(s.get("c::A").body.is_none() && s.get("c::T").body.is_none());
        assert!(s.get("c::C").body.is_some() && s.get("c::S").body.is_some());
        assert!(s.get("c::m").body.is_some());
        let call = s.get("c::lazy!");
        assert!(call.ident.is_none());
        assert_eq!(&s.0.fields.len(), &1);
        assert_eq!(s.0.fields[0].owner.0, "c::U");
        // inline module: id, module field, span covers its braces
        let m = s.get("c::i");
        assert!(m.ident.is_some() && m.body.is_none());
        assert_eq!(s.get("c::i::g").unit.module.as_ref().unwrap().0, "c::i");
        assert_eq!(m.unit.module.as_ref().unwrap().0, "c");
        assert_eq!(s.0.uses.len(), 2);
        assert!(!s.0.uses[0].public && s.0.uses[1].public);
        assert_eq!(s.0.uses[0].module.0, "c");
    }

    #[test]
    fn macro_call_body_is_its_delimited_tokens() {
        let s = parse("lazy! {\n  x\n}\nm!(a);");
        let b = s.get("c::lazy!").body.unwrap();
        assert_eq!(b.start, Pos { line: 1, col: 6 });
        assert_eq!(b.end, Pos { line: 3, col: 1 });
        assert!(s.get("c::lazy!").span.end >= b.end);
        let m = s.get("c::m!").body.unwrap();
        assert_eq!(m.start, Pos { line: 4, col: 2 });
        assert_eq!(m.end, Pos { line: 4, col: 5 });
    }

    #[test]
    fn fields_record_named_fields_only() {
        let s = parse("struct A { pub x: u32, y: u8 } struct B(u8); struct C;");
        let names: Vec<_> =
            s.0.fields
                .iter()
                .map(|f| (f.owner.0.as_str(), f.name.as_str(), f.ident))
                .collect();
        assert_eq!(
            names,
            [
                ("c::A", "x", Pos { line: 1, col: 15 }),
                ("c::A", "y", Pos { line: 1, col: 23 })
            ]
        );
    }

    #[test]
    fn cfg_test_and_test_attrs_mark_tests() {
        let s = parse(
            "fn a() {} #[test] fn b() {} #[tokio::test] fn c() {} #[cfg(test)] mod t { fn d() {} impl X { fn e() {} } } #[cfg(all(test, unix))] fn f2() {} #[cfg(any(test, unix))] fn g() {} #[cfg(not(test))] fn h() {} #[cfg(unix)] fn i() {} #[cfg_attr(test, allow(x))] fn j() {} #[cfg(feature = \"test\")] fn k() {} #[cfg(any())] fn z() {}",
        );
        let t = |id: &str| s.get(id).unit.test;
        assert!(!t("c::a") && !t("c"));
        for id in [
            "c::b",
            "c::c",
            "c::t",
            "c::t::d",
            "c::t::X::e",
            "c::t::<impl X>",
            "c::f2",
        ] {
            assert!(t(id), "{id}");
        }
        for id in ["c::g", "c::h", "c::i", "c::j", "c::k", "c::z"] {
            assert!(!t(id), "{id}");
        }
        assert!(parse_in("fn a() {}", true).get("c::a").unit.test);
    }

    #[test]
    fn features_come_from_cfg_cfg_attr_and_ancestors() {
        let s = parse(
            "#[cfg(feature = \"m\")] mod g { #[cfg(all(feature = \"a\", not(feature = \"b\")))] fn f() {} #[cfg_attr(feature = \"c\", inline)] fn h() {} impl X { #[cfg(feature = \"d\")] fn i() {} } } #[derive(Debug)] #[cfg(feature = \"nested\")] struct S;",
        );
        let f = |id: &str| -> Vec<String> { s.get(id).unit.features.iter().cloned().collect() };
        assert_eq!(f("c::g"), ["m"]);
        assert_eq!(f("c::g::f"), ["a", "b", "m"]);
        assert_eq!(f("c::g::h"), ["c", "m"]);
        assert_eq!(f("c::g::X::i"), ["d", "m"]);
        assert_eq!(f("c::S"), ["nested"]);
        assert!(f("c").is_empty());
    }

    #[test]
    fn own_lines_exclude_children_but_not_other_files() {
        let s = parse(
            "// top\nfn a() {}\nfn b() {} fn c() {}\n\nmod m {\n    fn d() {}\n}\nimpl X {\n    fn e() {\n        1\n    }\n}\n",
        );
        let own = |id: &str| (s.get(id).unit.lines, s.get(id).unit.own_lines);
        assert_eq!(own("c::m"), (3, 2));
        assert_eq!(own("c::<impl X>"), (5, 2));
        assert_eq!(own("c::a"), (1, 1));
        // lines 1 (comment), 4 (blank) are the file module's own; a, b/c, m and impl cover the rest
        assert_eq!(own("c"), (12, 2));
        assert_eq!(s.get("c").span.end, Pos { line: 13, col: 0 });
        assert_eq!(s.get("c").unit.end_line, 12);
    }

    #[test]
    fn end_of_counts_chars_not_bytes() {
        assert_eq!(end_of(""), Pos { line: 1, col: 0 });
        assert_eq!(end_of("a\n"), Pos { line: 2, col: 0 });
        assert_eq!(end_of("a\n🎵🎵"), Pos { line: 2, col: 2 });
    }

    #[test]
    fn columns_are_chars() {
        let s = parse("const B: &str = \"🎵🎵\"; fn after() {}");
        assert_eq!(s.get("c::after").ident, Some(Pos { line: 1, col: 25 }));
        assert_eq!(s.get("c::after").span.start, Pos { line: 1, col: 22 });
    }

    #[test]
    fn a_broken_file_warns() {
        let mut c = Collector::new(None);
        c.begin_crate("c", false);
        c.add_file_module("bad.rs", "fn (".to_string(), None, String::new());
        let s = c.finish();
        assert!(s.units.is_empty());
        assert_eq!(s.warnings.len(), 1);
        assert_eq!(s.warnings[0].kind, WarningKind::UnparsedFile);
        assert_eq!(s.warnings[0].loc.as_ref().unwrap().file, "bad.rs");
    }
}
