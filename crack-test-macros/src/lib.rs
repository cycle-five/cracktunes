//! `#[db_test(migrator = "MIGRATOR")]`: crack-core's replacement for
//! `#[sqlx::test(migrator = "MIGRATOR")]`.
//!
//! 🔑 `#[sqlx::test]` runs every migration into a fresh database for every test.
//! This attribute hands the test to `crate::test_db::run`, which copies a
//! template that was migrated once. It takes the same argument and the same
//! function shapes, so moving a test over changes only the attribute's path.
//!
//! 🪤 **Only crack-core can use it.** The expansion calls `crate::test_db`,
//! which resolves in the crate that uses the attribute, and only crack-core has
//! that module.
//!
//! ```ignore
//! #[crack_test_macros::db_test(migrator = "MIGRATOR")]
//! #[cfg_attr(not(feature = "db-tests"), ignore = "…")]
//! async fn finds_the_user(pool: PgPool) -> sqlx::Result<()> { … }
//! ```
//!
//! expands to
//!
//! ```ignore
//! #[test]
//! #[cfg_attr(not(feature = "db-tests"), ignore = "…")]
//! fn finds_the_user() -> sqlx::Result<()> {
//!     async fn finds_the_user(pool: PgPool) -> sqlx::Result<()> { … }
//!     crate::test_db::run(&MIGRATOR, concat!(module_path!(), "::", stringify!(finds_the_user)), finds_the_user)
//! }
//! ```
//!
//! Every other attribute moves to the outer function unchanged. Lint levels
//! reach nested items, so an `#[expect(..)]` there still covers the body.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, ItemFn, LitStr, Path, Token};

/// Runs an `async fn(pool: PgPool)` test in its own copy of the migrator's
/// template database. See the crate documentation.
#[proc_macro_attribute]
pub fn db_test(args: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(args as Args);
    let item = syn::parse_macro_input!(item as ItemFn);
    expand(&args, item)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `migrator = "PATH"`, the one argument, spelled as `#[sqlx::test]` spells it.
struct Args {
    migrator: Path,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key: Ident = input.parse()?;
        if key != "migrator" {
            return Err(syn::Error::new(
                key.span(),
                r#"expected `migrator = "PATH"`"#,
            ));
        }
        input.parse::<Token![=]>()?;
        let migrator = input.parse::<LitStr>()?.parse::<Path>()?;
        if !input.is_empty() {
            return Err(input.error(r#"expected only `migrator = "PATH"`"#));
        }
        Ok(Args { migrator })
    }
}

fn expand(args: &Args, item: ItemFn) -> syn::Result<TokenStream2> {
    let ItemFn {
        attrs,
        vis,
        sig,
        block,
    } = item;

    if sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            sig.fn_token,
            "#[db_test] needs an `async fn`",
        ));
    }
    if sig.inputs.len() != 1 {
        return Err(syn::Error::new_spanned(
            &sig.ident,
            "#[db_test] needs exactly one argument, the `PgPool`",
        ));
    }

    let name = &sig.ident;
    let output = &sig.output;
    let migrator = &args.migrator;
    Ok(quote! {
        #[::core::prelude::v1::test]
        #(#attrs)*
        #vis fn #name() #output {
            #sig #block
            crate::test_db::run(
                &#migrator,
                ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#name)),
                #name,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Args {
        syn::parse_quote!(migrator = "MIGRATOR")
    }

    fn rejection(item: ItemFn) -> String {
        expand(&args(), item)
            .expect_err("the attribute should reject this")
            .to_string()
    }

    #[test]
    fn rejects_a_fn_that_is_not_async() {
        let error = rejection(syn::parse_quote!(
            fn plain(pool: PgPool) {}
        ));
        assert!(error.contains("async fn"), "{error}");
    }

    #[test]
    fn rejects_a_fn_without_exactly_one_argument() {
        for item in [
            syn::parse_quote!(
                async fn none() {}
            ),
            syn::parse_quote!(
                async fn two(pool: PgPool, other: u8) {}
            ),
        ] {
            let error = rejection(item);
            assert!(error.contains("exactly one argument"), "{error}");
        }
    }

    #[test]
    fn rejects_an_argument_other_than_migrator() {
        let error = syn::parse_str::<Args>(r#"fixtures = "users""#)
            .err()
            .expect("only `migrator` is accepted")
            .to_string();
        assert!(error.contains("migrator"), "{error}");
    }
}
