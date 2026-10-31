//! SCIP symbol strings: `<scheme> <manager> <package> <version> <descriptors>`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub package: String,
    pub version: String,
    pub descriptors: String,
}

impl Symbol {
    /// `None` for locals and anything that is not a five-part global symbol.
    pub fn parse(s: &str) -> Option<Symbol> {
        if s.starts_with("local ") {
            return None;
        }
        let mut parts = s.splitn(5, ' ');
        let (_scheme, _manager) = (parts.next()?, parts.next()?);
        let package = parts.next()?;
        let version = parts.next()?;
        let descriptors = parts.next().filter(|d| !d.is_empty())?;
        Some(Symbol {
            package: package.into(),
            version: version.into(),
            descriptors: descriptors.into(),
        })
    }

    /// `Some` only when the descriptors are a pure namespace chain ending in `/`.
    pub fn module_path(&self) -> Option<Vec<String>> {
        if self.descriptors == "crate/" {
            return Some(Vec::new());
        }
        let chain = self.descriptors.strip_suffix('/')?;
        chain
            .split('/')
            .map(|part| {
                let ident =
                    !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_');
                ident.then(|| part.to_string())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_external_method() {
        let s = Symbol::parse(
            "rust-analyzer cargo serenity 0.12.5 gateway/client/impl#[Client]data().",
        )
        .unwrap();
        assert_eq!(
            (
                s.package.as_str(),
                s.version.as_str(),
                s.descriptors.as_str()
            ),
            ("serenity", "0.12.5", "gateway/client/impl#[Client]data().")
        );
        assert_eq!(s.module_path(), None);
    }

    #[test]
    fn local_is_none() {
        assert!(Symbol::parse("local 12").is_none());
    }

    #[test]
    fn module_paths() {
        assert_eq!(
            Symbol::parse("rust-analyzer cargo crack-core 0.36.2 crate/")
                .unwrap()
                .module_path(),
            Some(vec![])
        );
        assert_eq!(
            Symbol::parse("rust-analyzer cargo crack-core 0.36.2 music/queue/")
                .unwrap()
                .module_path(),
            Some(vec!["music".into(), "queue".into()])
        );
    }

    #[test]
    fn short_or_empty_symbols_are_none() {
        assert!(Symbol::parse("").is_none());
        assert!(Symbol::parse("rust-analyzer cargo pkg 1.0").is_none());
        assert!(Symbol::parse("rust-analyzer cargo pkg 1.0 ").is_none());
    }

    #[test]
    fn descriptors_keep_spaces_from_backticks() {
        let s = Symbol::parse("rust-analyzer cargo pkg 1.0 a/`b c`#").unwrap();
        assert_eq!(s.descriptors, "a/`b c`#");
    }

    #[test]
    fn non_namespace_descriptors_have_no_module_path() {
        let p = |d: &str| {
            Symbol::parse(&format!("rust-analyzer cargo pkg 1.0 {d}"))
                .unwrap()
                .module_path()
        };
        assert_eq!(p("music/Queue#"), None);
        assert_eq!(p("music/run()."), None);
        assert_eq!(p("music/`odd name`/"), None);
        assert_eq!(p("music//"), None);
        assert_eq!(p("/"), None);
        assert_eq!(p("music"), None);
    }

    #[test]
    fn single_module_and_underscores() {
        let p = |d: &str| {
            Symbol::parse(&format!("rust-analyzer cargo pkg 1.0 {d}"))
                .unwrap()
                .module_path()
        };
        assert_eq!(p("a_b1/"), Some(vec!["a_b1".into()]));
    }
}
