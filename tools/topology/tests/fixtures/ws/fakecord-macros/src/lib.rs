use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn command(_a: TokenStream, item: TokenStream) -> TokenStream {
    item
}
