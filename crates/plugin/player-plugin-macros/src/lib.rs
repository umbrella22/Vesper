//! Procedural macros for the safe Vesper plugin author SDK.

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input, spanned::Spanned};

/// Exports a zero-argument plugin factory through the stable native entry.
#[proc_macro_attribute]
pub fn export(args: TokenStream, item: TokenStream) -> TokenStream {
    if !args.is_empty() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "`export` takes no arguments",
        )
        .to_compile_error()
        .into();
    }

    let factory = parse_macro_input!(item as ItemFn);
    let signature = &factory.sig;
    if signature.asyncness.is_some() {
        return syn::Error::new(signature.span(), "plugin factories cannot be async")
            .to_compile_error()
            .into();
    }
    if !signature.generics.params.is_empty() || signature.generics.where_clause.is_some() {
        return syn::Error::new(
            signature.generics.span(),
            "plugin factories cannot be generic",
        )
        .to_compile_error()
        .into();
    }
    if !signature.inputs.is_empty() {
        return syn::Error::new(
            signature.inputs.span(),
            "plugin factories must take no arguments",
        )
        .to_compile_error()
        .into();
    }

    let factory_name = &signature.ident;
    quote! {
        #factory

        #[doc(hidden)]
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn vesper_plugin_entry()
            -> *const ::player_plugin::__private::VesperPluginRoot
        {
            ::player_plugin::__private::export_plugin(#factory_name)
        }
    }
    .into()
}
