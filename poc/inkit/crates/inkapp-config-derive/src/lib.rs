//! `#[derive(Config)]` for inkapp-config. Generates an `impl Default` honoring
//! per-field `#[config(default = <expr>)]`, the `Config` trait impl (KIND +
//! NAMESPACE), and an `inventory::submit!` registering the section's schema.
//!
//! Authors pair this with `#[derive(serde::Deserialize)]` + `#[serde(default)]`
//! on the same struct; serde then fills absent fields from the generated Default.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Expr, Fields, LitStr, Type};

#[proc_macro_derive(Config, attributes(config))]
pub fn derive_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    // ---- struct-level: #[config(kind = "...", namespace = "...")] ----
    let mut kind: Option<String> = None;
    let mut namespace = "framework".to_string();
    for attr in &input.attrs {
        if attr.path().is_ident("config") {
            attr.parse_nested_meta(|m| {
                if m.path.is_ident("kind") {
                    kind = Some(m.value()?.parse::<LitStr>()?.value());
                } else if m.path.is_ident("namespace") {
                    namespace = m.value()?.parse::<LitStr>()?.value();
                } else {
                    return Err(m.error("unknown #[config(...)] key (expected kind/namespace)"));
                }
                Ok(())
            })
            .expect("parse struct-level #[config(...)]");
        }
    }
    let kind = kind.expect("#[config(kind = \"...\")] is required on the struct");
    let ns_variant = match namespace.as_str() {
        "connector" => quote!(::inkapp_config::Namespace::Connector),
        "app" => quote!(::inkapp_config::Namespace::App),
        "framework" => quote!(::inkapp_config::Namespace::Framework),
        other => panic!("unknown namespace {other:?} (expected connector/app/framework)"),
    };

    // ---- fields ----
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(n) => &n.named,
            _ => panic!("Config derive requires a struct with named fields"),
        },
        _ => panic!("Config derive requires a struct"),
    };

    let mut default_inits = Vec::new();
    let mut schema_entries = Vec::new();

    for f in fields {
        let fname = f.ident.clone().unwrap();
        let fname_str = fname.to_string();

        let mut default_expr: Option<Expr> = None;
        let mut doc = String::new();
        for attr in &f.attrs {
            if attr.path().is_ident("config") {
                attr.parse_nested_meta(|m| {
                    if m.path.is_ident("default") {
                        default_expr = Some(m.value()?.parse::<Expr>()?);
                    } else {
                        return Err(
                            m.error("unknown #[config(...)] key on field (expected: default)")
                        );
                    }
                    Ok(())
                })
                .expect("parse field-level #[config(...)]");
            } else if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(nv) = &attr.meta {
                    if let Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = &nv.value
                    {
                        if !doc.is_empty() {
                            doc.push(' ');
                        }
                        doc.push_str(s.value().trim());
                    }
                }
            }
        }

        let fty = &f.ty;
        let ty_str = quote!(#fty).to_string().replace(' ', "");
        let field_kind = match last_segment(fty).as_deref() {
            Some("SecretRef") => quote!(::inkapp_config::FieldKind::Secret),
            Some("ConnectorRef") => quote!(::inkapp_config::FieldKind::Connector),
            _ => quote!(::inkapp_config::FieldKind::Plain),
        };

        let (default_str, default_init) = if let Some(expr) = &default_expr {
            (quote!(#expr).to_string(), quote! { #fname: #expr })
        } else {
            (
                String::new(),
                quote! { #fname: ::core::default::Default::default() },
            )
        };
        default_inits.push(default_init);

        let name_lit = LitStr::new(&fname_str, fname.span());
        let ty_lit = LitStr::new(&ty_str, fname.span());
        let def_lit = LitStr::new(&default_str, fname.span());
        let doc_lit = LitStr::new(&doc, fname.span());
        schema_entries.push(quote! {
            ::inkapp_config::FieldSchema {
                name: #name_lit,
                ty: #ty_lit,
                default: #def_lit,
                doc: #doc_lit,
                kind: #field_kind,
            }
        });
    }

    let kind_lit = LitStr::new(&kind, name.span());

    quote! {
        impl ::core::default::Default for #name {
            fn default() -> Self {
                Self { #(#default_inits),* }
            }
        }

        impl ::inkapp_config::Config for #name {
            const KIND: &'static str = #kind_lit;
            const NAMESPACE: ::inkapp_config::Namespace = #ns_variant;
        }

        ::inkapp_config::inventory::submit! {
            ::inkapp_config::ConfigSchema {
                kind: #kind_lit,
                namespace: #ns_variant,
                fields: &[ #(#schema_entries),* ],
            }
        }
    }
    .into()
}

// Matches the last path segment only — assumes the names SecretRef/ConnectorRef
// refer to inkapp_config's types. An aliased or shadowed type would be
// misclassified, but that does not occur in practice in this codebase.
fn last_segment(ty: &Type) -> Option<String> {
    if let Type::Path(p) = ty {
        p.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}
