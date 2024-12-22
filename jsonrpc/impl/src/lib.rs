use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse_macro_input, spanned::Spanned, FnArg, ImplItem, ItemFn, ItemImpl, LitStr, ReturnType,
    Signature, Type, TypePath,
};

#[proc_macro_attribute]
pub fn rpc_method(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_fn = parse_macro_input!(item as ItemFn);
    TokenStream::from(quote! {
        #item_fn
    })
}

macro_rules! err {
    ($($tt:tt)*) => {
        return syn::Error::new($($tt)*).to_compile_error().into()
    };
}

#[proc_macro_attribute]
pub fn rpc_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item2 = item.clone();
    let parsed_input = parse_macro_input!(item2 as ItemImpl);

    let self_ty = match *parsed_input.self_ty {
        Type::Path(p) => p,
        _ => err!(
            parsed_input.span(),
            "Implementing the trait `RPCService` on this type is unsupported"
        ),
    };

    let mut service_name = None;
    if !attr.is_empty() {
        let parsed_attr = syn::parse_macro_input!(attr as syn::Meta);
        service_name = match parse_service_name(parsed_attr) {
            Ok(res) => res,
            Err(err) => return err.to_compile_error().into(),
        };
    }

    let default_sn = match self_ty.path.require_ident() {
        Ok(res) => res.to_string(),
        Err(err) => return err.to_compile_error().into(),
    };
    let service_name = service_name.unwrap_or(default_sn);

    let methods = match parse_struct_methods(&self_ty, parsed_input.items) {
        Ok(res) => res,
        Err(err) => return err.to_compile_error().into(),
    };

    let mut method_idents = vec![];
    for (mn, method) in methods.iter() {
        if method.inputs.len() != 2 {
            err!(
                method.span(),
                "requires `&self` and a parameter of type `serde_json::Value`"
            );
        }

        method_idents.push((
            mn.clone().unwrap_or(method.ident.to_string()),
            method.ident.clone(),
        ));
    }

    let impl_methods: Vec<TokenStream2> = method_idents
        .iter()
        .map(|(n, m)| {
            quote! {
                #n => Some(Box::new(move |params: serde_json::Value| Box::pin(self.#m(params)))),
            }
        })
        .collect();

    let item: TokenStream2 = item.into();
    quote! {
        impl karyon_jsonrpc::server::RPCService for #self_ty {
            fn get_method(
                &self,
                name: &str
            ) -> Option<karyon_jsonrpc::server::RPCMethod> {
                match name {
                #(#impl_methods)*
                    _ => None,
                }
            }
            fn name(&self) -> String{
                #service_name.to_string()
            }
        }
        #item
    }
    .into()
}

#[proc_macro_attribute]
pub fn rpc_pubsub_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item2 = item.clone();
    let parsed_input = parse_macro_input!(item2 as ItemImpl);

    let self_ty = match *parsed_input.self_ty {
        Type::Path(p) => p,
        _ => err!(
            parsed_input.span(),
            "Implementing the trait `PubSubRPCService` on this type is unsupported"
        ),
    };

    let mut service_name = None;
    if !attr.is_empty() {
        let parsed_attr = syn::parse_macro_input!(attr as syn::Meta);
        service_name = match parse_service_name(parsed_attr) {
            Ok(res) => res,
            Err(err) => return err.to_compile_error().into(),
        };
    }

    let default_sn = match self_ty.path.require_ident() {
        Ok(res) => res.to_string(),
        Err(err) => return err.to_compile_error().into(),
    };
    let service_name = service_name.unwrap_or(default_sn);

    let methods = match parse_struct_methods(&self_ty, parsed_input.items) {
        Ok(res) => res,
        Err(err) => return err.to_compile_error().into(),
    };

    let mut method_idents = vec![];
    for (mn, method) in methods.iter() {
        if method.inputs.len() != 4 {
            err!(method.span(), "requires `&self` and three parameters: `Arc<Channel>`, method: `String`, and `serde_json::Value`");
        }

        method_idents.push((
            mn.clone().unwrap_or(method.ident.to_string()),
            method.ident.clone(),
        ));
    }

    let impl_methods: Vec<TokenStream2> = method_idents.iter().map(
        |(n, m)| quote! {
            #n => {
                Some(Box::new(
                    move |chan: std::sync::Arc<karyon_jsonrpc::server::channel::Channel>, method: String, params: serde_json::Value| {
                    Box::pin(self.#m(chan, method, params))
                }))
            },
        },
    ).collect();

    let item: TokenStream2 = item.into();
    quote! {
        impl karyon_jsonrpc::server::PubSubRPCService for #self_ty {
            fn get_pubsub_method(
                &self,
                name: &str
            ) -> Option<karyon_jsonrpc::server::PubSubRPCMethod> {
                match name {
                #(#impl_methods)*
                    _ => None,
                }
            }

            fn name(&self) -> String{
                #service_name.to_string()
            }
        }
        #item
    }
    .into()
}

fn parse_struct_methods(
    self_ty: &TypePath,
    items: Vec<ImplItem>,
) -> Result<Vec<(Option<String>, Signature)>, syn::Error> {
    let mut methods: Vec<(Option<String>, Signature)> = vec![];

    if items.is_empty() {
        return Err(syn::Error::new(
            self_ty.span(),
            "At least one method should be implemented",
        ));
    }

    for item in items {
        match item {
            ImplItem::Fn(method) => {
                let mut rpc_method_name = None;
                validate_method(&method.sig)?;

                for attr in method.attrs {
                    if attr.path().is_ident("rpc_method") {
                        attr.parse_nested_meta(|meta| {
                            if meta.path.is_ident("name") {
                                let value = meta.value()?;
                                let s: LitStr = value.parse()?;
                                if s.value().is_empty() {
                                    return Err(syn::Error::new(attr.span(), "Empty string"));
                                }
                                rpc_method_name = Some(s.value().clone());
                                Ok(())
                            } else {
                                Err(syn::Error::new(attr.span(), "Unexpected attribute"))
                            }
                        })?;
                        break;
                    }
                }

                methods.push((rpc_method_name, method.sig));
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

fn parse_service_name(attr: syn::Meta) -> Result<Option<String>, syn::Error> {
    if let syn::Meta::NameValue(ref n) = attr {
        if n.path.is_ident("name") {
            if let syn::Expr::Lit(lit) = &n.value {
                if let syn::Lit::Str(lit_str) = &lit.lit {
                    if lit_str.value().is_empty() {
                        return Err(syn::Error::new(attr.span(), "Empty string"));
                    }
                    return Ok(Some(lit_str.value().to_string()));
                }
            }
        } else {
            return Err(syn::Error::new(attr.span(), "Unexpected attribute"));
        }
    }
    Ok(None)
}
