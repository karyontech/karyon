use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, spanned::Spanned, ImplItem, ItemImpl, Type};

macro_rules! err {
    ($($tt:tt)*) => {
        return syn::Error::new($($tt)*).to_compile_error().into()
    };
}

#[proc_macro_attribute]
pub fn rpc_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut methods: Vec<Ident> = vec![];

    let item2 = item.clone();
    let parsed_input = parse_macro_input!(item2 as ItemImpl);

    let self_ty = match *parsed_input.self_ty {
        Type::Path(p) => p,
        _ => err!(
            parsed_input.span(),
            "implementing the trait `RPCService` on this type is unsupported"
        ),
    };

    if parsed_input.items.is_empty() {
        err!(self_ty.span(), "At least one method should be implemented");
    }

    for item in parsed_input.items {
        match item {
            ImplItem::Method(method) => {
                methods.push(method.sig.ident);
            }
            _ => err!(item.span(), "unexpected item"),
        }
    }

    let item2: TokenStream2 = item.into();
    let quoted = quote! {
            karyon_jsonrpc_internal::impl_rpc_service!(#self_ty, #(#methods),*);
            #item2
    };

    quoted.into()
}
