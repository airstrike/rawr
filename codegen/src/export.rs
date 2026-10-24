//! `#[rawr::export]`: an app's struct, impl block or free function, made
//! available to typed scripts through `rawr::Exports`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::spanned::Spanned;

pub fn expand(item: syn::Item) -> TokenStream {
    let expanded = match &item {
        syn::Item::Struct(definition) => structure(definition),
        syn::Item::Impl(block) => methods(block),
        syn::Item::Fn(function) => free_function(function),
        other => Err(syn::Error::new(
            other.span(),
            "`#[rawr::export]` exports a struct, its impl block or a free function",
        )),
    };
    match expanded {
        Ok(expanded) => quote! { #item #expanded },
        Err(error) => {
            let error = error.to_compile_error();
            quote! { #item #error }
        }
    }
}

fn no_generics(generics: &syn::Generics, what: &str) -> syn::Result<()> {
    if generics.params.is_empty() && generics.where_clause.is_none() {
        return Ok(());
    }
    Err(syn::Error::new(
        generics.span(),
        format!("`#[rawr::export]` can't export a generic {what} yet"),
    ))
}

fn structure(definition: &syn::ItemStruct) -> syn::Result<TokenStream> {
    no_generics(&definition.generics, "struct")?;
    let ident = &definition.ident;
    let name = ident.unraw().to_string();
    let fields = match &definition.fields {
        syn::Fields::Named(fields) => fields
            .named
            .iter()
            .filter(|field| matches!(field.vis, syn::Visibility::Public(_)))
            .collect(),
        _ => Vec::new(),
    };
    let fields = fields.into_iter().map(|field| {
        let member = field.ident.as_ref().expect("a named field has a name");
        let path = format!("{name}::{}", member.unraw());
        let ty = &field.ty;
        quote! {
            exports.field(
                #path,
                <#ty as ::rawr::host::export::Value>::ty(),
                <Self as ::rawr::host::export::Value>::ty(),
                |args| {
                    if args.len() == 1 {
                        return ::rawr::host::export::__private::shared::<#ident, _>(&*args[0], |this| {
                            ::rawr::host::export::Value::into_dynamic(::core::clone::Clone::clone(&this.#member))
                        });
                    }
                    let value: #ty = ::rawr::host::export::Value::from_dynamic(&*args[1])?;
                    let mut this = ::rawr::host::export::__private::owned::<#ident>(&mut *args[0])?;
                    this.#member = value;
                    ::core::result::Result::Ok(::rawr::host::export::Value::into_dynamic(this))
                },
            );
        }
    });
    Ok(quote! {
        impl ::rawr::host::export::Value for #ident {
            fn ty() -> ::core::result::Result<::rawr::host::type_ref::Type, ::rawr::host::Error> {
                ::rawr::host::type_ref::Type::registered(#name)
            }
            fn into_dynamic(self) -> ::rawr::Dynamic {
                ::rawr::Dynamic::from(self)
            }
            fn from_dynamic(
                value: &::rawr::Dynamic,
            ) -> ::core::result::Result<Self, ::std::boxed::Box<::rawr::EvalAltResult>> {
                ::rawr::host::export::__private::shared::<Self, Self>(value, ::core::clone::Clone::clone)
            }
        }

        impl ::rawr::Export for #ident {
            fn export(exports: &mut ::rawr::Exports) {
                exports.type_::<Self>(#name);
                #(#fields)*
                {
                    use ::rawr::host::export::__private::{NoMethods as _, ViaMethods as _};
                    (&::rawr::host::export::__private::Probe::<Self>::new()).methods(exports);
                }
            }
        }
    })
}

/// A function's declared signature and the callback that calls it.
struct Callable {
    inputs: Vec<TokenStream>,
    output: TokenStream,
    reads: Vec<TokenStream>,
    passes: Vec<TokenStream>,
}

fn callable(signature: &syn::Signature, first: usize) -> syn::Result<Callable> {
    no_generics(&signature.generics, "function")?;
    if let Some(asyncness) = signature.asyncness {
        return Err(syn::Error::new(
            asyncness.span(),
            "`#[rawr::export]` can't export an async function yet",
        ));
    }
    let mut callable = Callable {
        inputs: Vec::new(),
        output: match &signature.output {
            syn::ReturnType::Default => quote! { <() as ::rawr::host::export::Value>::ty() },
            syn::ReturnType::Type(_, ty) => {
                if let syn::Type::Reference(reference) = ty.as_ref() {
                    return Err(syn::Error::new(
                        reference.span(),
                        "`#[rawr::export]` can't export a function that returns a reference; return an owned value",
                    ));
                }
                quote! { <#ty as ::rawr::host::export::Value>::ty() }
            }
        },
        reads: Vec::new(),
        passes: Vec::new(),
    };
    let typed = signature.inputs.iter().filter_map(|input| match input {
        syn::FnArg::Typed(typed) => Some(typed),
        syn::FnArg::Receiver(_) => None,
    });
    for (index, typed) in typed.enumerate() {
        let slot = first + index;
        let local = format_ident!("__rawr_argument_{index}");
        match typed.ty.as_ref() {
            syn::Type::Reference(reference) if reference.mutability.is_some() => {
                return Err(syn::Error::new(
                    reference.span(),
                    "`#[rawr::export]` can't export a `&mut` parameter yet",
                ));
            }
            syn::Type::Reference(reference) => {
                let target = &reference.elem;
                let owned = quote! { <#target as ::rawr::host::export::Param>::Owned };
                callable
                    .inputs
                    .push(quote! { <#owned as ::rawr::host::export::Value>::ty() });
                callable.reads.push(quote! {
                    let #local: #owned = ::rawr::host::export::Value::from_dynamic(&*args[#slot])?;
                });
                callable.passes.push(quote! { &#local });
            }
            syn::Type::ImplTrait(ty) => {
                return Err(syn::Error::new(
                    ty.span(),
                    "`#[rawr::export]` can't export an `impl Trait` parameter yet",
                ));
            }
            ty => {
                callable
                    .inputs
                    .push(quote! { <#ty as ::rawr::host::export::Value>::ty() });
                callable.reads.push(quote! {
                    let #local: #ty = ::rawr::host::export::Value::from_dynamic(&*args[#slot])?;
                });
                callable.passes.push(quote! { #local });
            }
        }
    }
    Ok(callable)
}

fn methods(block: &syn::ItemImpl) -> syn::Result<TokenStream> {
    no_generics(&block.generics, "impl block")?;
    if let Some((_, path, _)) = &block.trait_ {
        return Err(syn::Error::new(
            path.span(),
            "`#[rawr::export]` exports an inherent impl block, not a trait impl",
        ));
    }
    let owner = &block.self_ty;
    let syn::Type::Path(path) = owner.as_ref() else {
        return Err(syn::Error::new(
            owner.span(),
            "`#[rawr::export]` exports the impl block of a named struct",
        ));
    };
    let name = path
        .path
        .segments
        .last()
        .map(|segment| segment.ident.unraw().to_string())
        .unwrap_or_default();
    let mut declarations = Vec::new();
    for item in &block.items {
        let syn::ImplItem::Fn(function) = item else {
            continue;
        };
        if !matches!(function.vis, syn::Visibility::Public(_)) {
            continue;
        }
        let signature = &function.sig;
        let ident = &signature.ident;
        let item_path = format!("{name}::{}", ident.unraw());
        let receiver = signature.receiver();
        if let Some(receiver) = receiver
            && receiver.colon_token.is_some()
        {
            return Err(syn::Error::new(
                receiver.span(),
                "`#[rawr::export]` exports `self`, `&self` and `&mut self` receivers",
            ));
        }
        let Callable {
            inputs,
            output,
            reads,
            passes,
        } = callable(signature, usize::from(receiver.is_some()))?;
        let call = |this: TokenStream| quote! { <#owner>::#ident(#this #(#passes),*) };
        let declaration = match receiver {
            None => {
                let call = call(quote! {});
                quote! {
                    exports.associated_function(#item_path, ::std::vec![#(#inputs),*], #output, |args| {
                        #(#reads)*
                        ::core::result::Result::Ok(::rawr::host::export::Value::into_dynamic(#call))
                    });
                }
            }
            Some(receiver) if receiver.reference.is_none() => {
                let call = call(quote! { this, });
                quote! {
                    exports.method(#item_path, ::rawr::receiver::Mode::Owned, ::std::vec![#(#inputs),*], #output, |args| {
                        #(#reads)*
                        let this = ::rawr::host::export::__private::owned::<#owner>(&mut *args[0])?;
                        ::core::result::Result::Ok(::rawr::host::export::Value::into_dynamic(#call))
                    });
                }
            }
            Some(receiver) => {
                let call = call(quote! { this, });
                let (mode, access, argument) = if receiver.mutability.is_some() {
                    (
                        quote! { Mutable },
                        quote! { mutable },
                        quote! { &mut *args[0] },
                    )
                } else {
                    (quote! { Shared }, quote! { shared }, quote! { &*args[0] })
                };
                quote! {
                    exports.method(#item_path, ::rawr::receiver::Mode::#mode, ::std::vec![#(#inputs),*], #output, |args| {
                        #(#reads)*
                        ::rawr::host::export::__private::#access::<#owner, _>(#argument, |this| #call)
                            .map(::rawr::host::export::Value::into_dynamic)
                    });
                }
            }
        };
        declarations.push(declaration);
    }
    Ok(quote! {
        impl ::rawr::host::export::__private::Methods for #owner {
            fn methods(exports: &mut ::rawr::Exports) {
                #(#declarations)*
            }
        }
    })
}

fn free_function(function: &syn::ItemFn) -> syn::Result<TokenStream> {
    let signature = &function.sig;
    let ident = &signature.ident;
    let name = ident.unraw().to_string();
    let visibility = &function.vis;
    let Callable {
        inputs,
        output,
        reads,
        passes,
    } = callable(signature, 0)?;
    // The function's name in the type namespace, which `Exports::add`
    // names: a braced struct takes no value-namespace name.
    let marker = ident;
    Ok(quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code)]
        #visibility struct #marker {}

        impl ::rawr::Export for #marker {
            fn export(exports: &mut ::rawr::Exports) {
                exports.function(#name, ::std::vec![#(#inputs),*], #output, |args| {
                    #(#reads)*
                    ::core::result::Result::Ok(::rawr::host::export::Value::into_dynamic(#ident(#(#passes),*)))
                });
            }
        }
    })
}
