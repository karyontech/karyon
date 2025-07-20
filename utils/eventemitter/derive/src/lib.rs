use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// Derive macro for the AsEventValue trait
///
/// This macro implements AsEventValue for structs and enums,
/// using either the type name as the event_id or by passing a custom event_id.
///
/// # Example
///
/// ```compile_fail;
///
/// #[derive(EventValue)]
/// struct MyEvent {
///     data: String,
/// }
///
/// // impl AsEventValue for MyEvent {
/// //     fn event_id() -> &'static str {
/// //         "MyEvent"
/// //     }
/// // }
///
/// #[derive(EventValue)]
/// #[event_id("custom_event_name")]
/// struct MyEvent2 {
///     data: String,
/// }
///
/// // impl AsEventValue for MyEvent2 {
/// //     fn event_id() -> &'static str {
/// //         "custom_event_name"
/// //     }
/// // }
///
/// ```
#[proc_macro_derive(EventValue, attributes(event_id))]
pub fn derive_event_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;

    let event_id = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("event_id"))
        .and_then(|attr| attr.parse_args::<syn::LitStr>().ok().map(|lit| lit.value()))
        .unwrap_or_else(|| name.to_string());

    let expanded = quote! {
        impl karyon_eventemitter::AsEventValue for #name {
            fn event_id() -> &'static str {
                #event_id
            }
        }
    };

    TokenStream::from(expanded)
}
