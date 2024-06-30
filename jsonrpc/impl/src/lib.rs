use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse_macro_input, spanned::Spanned, FnArg, ImplItem, ItemImpl, ReturnType, Signature, Type,
    TypePath,
};

macro_rules! err {
    ($($tt:tt)*) => {
        return syn::Error::new($($tt)*).to_compile_error().into()
    };
}

#[proc_macro_attribute]
pub fn rpc_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item2 = item.clone();
    let parsed_input = parse_macro_input!(item2 as ItemImpl);

    let self_ty = match *parsed_input.self_ty {
        Type::Path(p) => p,
        _ => err!(
            parsed_input.span(),
            "Implementing the trait `RPCService` on this type is unsupported"
        ),
    };

    let methods = match parse_struct_methods(&self_ty, parsed_input.items) {
        Ok(res) => res,
        Err(err) => return err.to_compile_error().into(),
    };

    let mut method_idents = vec![];
    for method in methods.iter() {
        method_idents.push(method.ident.clone());
        if method.inputs.len() != 2 {
            err!(
                method.span(),
                "requires `&self` and a parameter of type `serde_json::Value`"
            );
        }

        if let Err(err) = validate_method(method) {
            return err.to_compile_error().into();
        }
    }

    let impl_methods: Vec<TokenStream2> = method_idents.iter().map(
        |m| quote! {
            stringify!(#m) => Some(Box::new(move |params: serde_json::Value| Box::pin(self.#m(params)))),
        },
    ).collect();

    let item: TokenStream2 = item.into();
    quote! {
        impl karyon_jsonrpc::RPCService for #self_ty {
            fn get_method<'a>(
                &'a self,
                name: &'a str
            ) -> Option<karyon_jsonrpc::RPCMethod> {
                match name {
                #(#impl_methods)*
                    _ => None,
                }
            }
            fn name(&self) -> String{
                stringify!(#self_ty).to_string()
            }
        }
        #item
    }
    .into()
}

#[proc_macro_attribute]
pub fn rpc_pubsub_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item2 = item.clone();
    let parsed_input = parse_macro_input!(item2 as ItemImpl);

    let self_ty = match *parsed_input.self_ty {
        Type::Path(p) => p,
        _ => err!(
            parsed_input.span(),
            "Implementing the trait `PubSubRPCService` on this type is unsupported"
        ),
    };

    let methods = match parse_struct_methods(&self_ty, parsed_input.items) {
        Ok(res) => res,
        Err(err) => return err.to_compile_error().into(),
    };

    let mut method_idents = vec![];
    for method in methods.iter() {
        method_idents.push(method.ident.clone());
        if method.inputs.len() != 4 {
            err!(method.span(), "requires `&self` and three parameters: `Arc<Channel>`, method: `String`, and `serde_json::Value`");
        }
        if let Err(err) = validate_method(method) {
            return err.to_compile_error().into();
        }
    }

    let impl_methods: Vec<TokenStream2> = method_idents.iter().map(
        |m| quote! {
            stringify!(#m) => {
                Some(Box::new(
                    move |chan: std::sync::Arc<karyon_jsonrpc::Channel>, method: String, params: serde_json::Value| {
                    Box::pin(self.#m(chan, method, params))
                }))
            },
        },
    ).collect();

    let item: TokenStream2 = item.into();
    quote! {
        impl karyon_jsonrpc::PubSubRPCService for #self_ty {
            fn get_pubsub_method<'a>(
                &'a self,
                name: &'a str
            ) -> Option<karyon_jsonrpc::PubSubRPCMethod> {
                match name {
                #(#impl_methods)*
                    _ => None,
                }
            }

            fn name(&self) -> String{
                stringify!(#self_ty).to_string()
            }
        }
        #item
    }
    .into()
}

fn parse_struct_methods(
    self_ty: &TypePath,
    items: Vec<ImplItem>,
) -> Result<Vec<Signature>, syn::Error> {
    let mut methods: Vec<Signature> = vec![];

    if items.is_empty() {
        return Err(syn::Error::new(
            self_ty.span(),
            "At least one method should be implemented",
        ));
    }

    for item in items {
        match item {
            ImplItem::Fn(method) => {
                methods.push(method.sig);
            }
            _ => return Err(syn::Error::new(item.span(), "Unexpected item!")),
        }
    }

    Ok(methods)
}

fn validate_method(method: &Signature) -> Result<(), syn::Error> {
    if let FnArg::Typed(_) = method.inputs[0] {
        return Err(syn::Error::new(method.span(), "requires `&self` parameter"));
    }

    if let ReturnType::Default = method.output {
        return Err(syn::Error::new(
            method.span(),
            "requires `Result<serde_json::Value, RPCError>` as return type",
        ));
    }
    Ok(())
}
