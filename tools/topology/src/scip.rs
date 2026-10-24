//! Reads a SCIP index with the `scip` crate's generated protobuf types.

use anyhow::{Context, Result, bail};
use protobuf::Message;
use scip::types::{Index, PositionEncoding};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub symbol: String,
    pub definition: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScipDocument {
    pub path: String,
    pub occurrences: Vec<Occurrence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScipIndex {
    pub documents: Vec<ScipDocument>,
}

pub fn read(path: &Path) -> Result<ScipIndex> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    parse(&bytes).with_context(|| format!("decoding {}", path.display()))
}

pub fn parse(bytes: &[u8]) -> Result<ScipIndex> {
    let index = Index::parse_from_bytes(bytes).context("not a SCIP index")?;
    let mut documents = Vec::with_capacity(index.documents.len());
    for doc in index.documents {
        match doc.position_encoding.enum_value() {
            Ok(PositionEncoding::UTF8CodeUnitOffsetFromLineStart) => {},
            Ok(other) => bail!(
                "{}: unsupported SCIP position encoding {other:?} (need UTF8CodeUnitOffsetFromLineStart)",
                doc.relative_path
            ),
            Err(n) => bail!("{}: unknown SCIP position encoding {n}", doc.relative_path),
        }
        let mut occurrences = Vec::with_capacity(doc.occurrences.len());
        for occ in &doc.occurrences {
            let r = occ
                .range
                .iter()
                .map(|&n| u32::try_from(n))
                .collect::<Result<Vec<u32>, _>>()
                .with_context(|| {
                    format!("{}: negative range {:?}", doc.relative_path, occ.range)
                })?;
            let (start_line, start_col, end_line, end_col) = match r[..] {
                [l, s, e] => (l, s, l, e),
                [sl, sc, el, ec] => (sl, sc, el, ec),
                _ => bail!("{}: malformed range {:?}", doc.relative_path, occ.range),
            };
            occurrences.push(Occurrence {
                start_line,
                start_col,
                end_line,
                end_col,
                symbol: occ.symbol.clone(),
                definition: occ.symbol_roles & 1 != 0,
            });
        }
        documents.push(ScipDocument {
            path: doc.relative_path,
            occurrences,
        });
    }
    documents.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ScipIndex { documents })
}

#[cfg(test)]
mod tests {
    use super::*;
    use protobuf::{EnumOrUnknown, Message};
    use scip::types::{Document, Index, Occurrence as Occ, PositionEncoding};

    fn occ(range: &[i32], symbol: &str, roles: i32) -> Occ {
        Occ {
            range: range.to_vec(),
            symbol: symbol.into(),
            symbol_roles: roles,
            ..Default::default()
        }
    }

    fn doc(path: &str, enc: PositionEncoding, occs: Vec<Occ>) -> Document {
        Document {
            relative_path: path.into(),
            position_encoding: EnumOrUnknown::new(enc),
            occurrences: occs,
            ..Default::default()
        }
    }

    fn bytes(docs: Vec<Document>) -> Vec<u8> {
        Index {
            documents: docs,
            ..Default::default()
        }
        .write_to_bytes()
        .unwrap()
    }

    const UTF8: PositionEncoding = PositionEncoding::UTF8CodeUnitOffsetFromLineStart;

    #[test]
    fn documents_sorted_and_ranges_decoded() {
        let b = bytes(vec![
            doc("b.rs", UTF8, vec![occ(&[3, 4, 9], "s b", 0)]),
            doc(
                "a.rs",
                UTF8,
                vec![occ(&[1, 2, 5, 6], "s a", 1), occ(&[7, 0, 3], "s c", 8)],
            ),
        ]);
        let idx = parse(&b).unwrap();
        let paths: Vec<_> = idx.documents.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["a.rs", "b.rs"]);
        assert_eq!(
            idx.documents[0].occurrences[0],
            Occurrence {
                start_line: 1,
                start_col: 2,
                end_line: 5,
                end_col: 6,
                symbol: "s a".into(),
                definition: true
            }
        );
        // roles = 8 has other bits set but not the definition bit
        assert_eq!(
            idx.documents[0].occurrences[1],
            Occurrence {
                start_line: 7,
                start_col: 0,
                end_line: 7,
                end_col: 3,
                symbol: "s c".into(),
                definition: false
            }
        );
        assert_eq!(idx.documents[1].occurrences[0].end_line, 3);
        assert_eq!(idx.documents[1].occurrences[0].end_col, 9);
    }

    #[test]
    fn definition_is_bit_zero_only() {
        let b = bytes(vec![doc(
            "a.rs",
            UTF8,
            vec![occ(&[0, 0, 1], "s", 3), occ(&[0, 0, 1], "s", 2)],
        )]);
        let o = &parse(&b).unwrap().documents[0].occurrences;
        assert!(o[0].definition);
        assert!(!o[1].definition);
    }

    #[test]
    fn other_encodings_are_rejected_by_name() {
        let b = bytes(vec![doc(
            "a.rs",
            PositionEncoding::UTF16CodeUnitOffsetFromLineStart,
            vec![],
        )]);
        let e = parse(&b).unwrap_err().to_string();
        assert!(e.contains("a.rs") && e.contains("UTF16"), "{e}");
        let b = bytes(vec![doc(
            "a.rs",
            PositionEncoding::UnspecifiedPositionEncoding,
            vec![],
        )]);
        assert!(parse(&b).is_err());
    }

    #[test]
    fn malformed_ranges_are_errors() {
        for r in [&[1, 2][..], &[1, 2, 3, 4, 5], &[-1, 0, 1]] {
            let b = bytes(vec![doc("a.rs", UTF8, vec![occ(r, "s", 0)])]);
            assert!(parse(&b).is_err(), "{r:?}");
        }
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(parse(&[0xff, 0xff, 0xff]).is_err());
    }
}
