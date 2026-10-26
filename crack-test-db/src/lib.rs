//! Template databases for db tests: the runner behind
//! `#[crack_test_macros::db_test(migrator = "MIGRATOR")]`, in its own crate so
//! every crate with db tests can use it.
//!
//! A crate that uses the attribute declares, because the expansion calls
//! `crate::test_db::run`:
//!
//! ```ignore
//! #[cfg(test)]
//! pub(crate) use crack_test_db::test_db;
//! ```

pub mod test_db;
pub use test_db::*;
