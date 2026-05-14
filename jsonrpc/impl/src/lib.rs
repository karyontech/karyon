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
    expand_service_impl(attr, item, ServiceKind::Rpc)
}

#[proc_macro_attribute]
pub fn rpc_pubsub_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_service_impl(attr, item, ServiceKind::PubSub)
}

#[derive(Clone, Copy)]
enum ServiceKind {
    Rpc,
    PubSub,
}

impl ServiceKind {
    fn arity(self) -> usize {
        match self {
            // &self + serde_json::Value
            ServiceKind::Rpc => 2,
            // &self + Arc<Channel>, String, serde_json::Value
            ServiceKind::PubSub => 4,
        }
    }

    fn arity_error(self) -> &'static str {
        match self {
            ServiceKind::Rpc => "requires `&self` and a parameter of type `serde_json::Value`",
            ServiceKind::PubSub => {
                "requires `&self` and three parameters: \
                `Arc<Channel>`, method: `String`, and `serde_json::Value`"
            }
        }
    }

    fn unsupported_self_type_error(self) -> &'static str {
        match self {
            ServiceKind::Rpc => "Implementing the trait `RPCService` on this type is unsupported",
            ServiceKind::PubSub => {
                "Implementing the trait `PubSubRPCService` on this type is unsupported"
            }
        }
    }

    fn dispatch_arm(self, name: &str, ident: &syn::Ident) -> TokenStream2 {
        match self {
            ServiceKind::Rpc => quote! {
                #name => Some(Box::new(
                    move |params: serde_json::Value| Box::pin(self.#ident(params))
                )),
            },
            ServiceKind::PubSub => quote! {
                #name => Some(Box::new(
                    move |chan: std::sync::Arc<karyon_jsonrpc::server::channel::Channel>,
                          method: String,
                          params: serde_json::Value| {
                        Box::pin(self.#ident(chan, method, params))
                    }
                )),
            },
        }
    }

    fn impl_block(
        self,
        self_ty: &TypePath,
        service_name: &str,
        arms: &[TokenStream2],
    ) -> TokenStream2 {
        match self {
            ServiceKind::Rpc => quote! {
                impl karyon_jsonrpc::server::RPCService for #self_ty {
                    fn get_method(
                        &self,
                        name: &str,
                    ) -> Option<karyon_jsonrpc::server::RPCMethod> {
                        match name {
                            #(#arms)*
                            _ => None,
                        }
                    }
                    fn name(&self) -> String {
                        #service_name.to_string()
                    }
                }
            },
            ServiceKind::PubSub => quote! {
                impl karyon_jsonrpc::server::PubSubRPCService for #self_ty {
                    fn get_pubsub_method(
                        &self,
                        name: &str,
                    ) -> Option<karyon_jsonrpc::server::PubSubRPCMethod> {
                        match name {
                            #(#arms)*
                            _ => None,
                        }
                    }
                    fn name(&self) -> String {
                        #service_name.to_string()
                    }
                }
            },
        }
    }
}

fn expand_service_impl(attr: TokenStream, item: TokenStream, kind: ServiceKind) -> TokenStream {
    let item2 = item.clone();
    let parsed_input = parse_macro_input!(item2 as ItemImpl);

    let self_ty = match *parsed_input.self_ty {
        Type::Path(p) => p,
        _ => err!(parsed_input.span(), kind.unsupported_self_type_error()),
    };

    let service_name = match resolve_service_name(attr, &self_ty) {
        Ok(name) => name,
        Err(err) => return err.to_compile_error().into(),
    };

    let methods = match parse_struct_methods(&self_ty, parsed_input.items) {
        Ok(res) => res,
        Err(err) => return err.to_compile_error().into(),
    };

    let mut arms = Vec::with_capacity(methods.len());
    for (rename, sig) in methods.iter() {
        if sig.inputs.len() != kind.arity() {
            err!(sig.span(), kind.arity_error());
        }
        let name = rename.clone().unwrap_or(sig.ident.to_string());
        arms.push(kind.dispatch_arm(&name, &sig.ident));
    }

    let impl_block = kind.impl_block(&self_ty, &service_name, &arms);
    let original: TokenStream2 = item.into();
    quote! {
        #impl_block
        #original
    }
    .into()
}

fn resolve_service_name(attr: TokenStream, self_ty: &TypePath) -> Result<String, syn::Error> {
    if !attr.is_empty() {
        let parsed_attr: syn::Meta = syn::parse(attr)?;
        if let Some(name) = parse_service_name(parsed_attr)? {
            return Ok(name);
        }
    }
    Ok(self_ty.path.require_ident()?.to_string())
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
