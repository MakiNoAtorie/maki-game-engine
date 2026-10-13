//! The derive macros of the engine.
//!
//! `#[derive(Fields)]` writes `maki_core::fields::Fields` for a struct or an enum:
//! the table of its fields, and its value as a tree. See that module for what a
//! tool does with them, and for the hints of `#[field(...)]`.
//!
//! How to read this file: a derive macro gets the text of a type as tokens, and
//! gives back the tokens of an `impl` block. `syn` reads the tokens into a tree
//! (`DeriveInput`), and `quote!` writes Rust code with `#name` for a part that this
//! code fills in.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Error, Expr, ExprLit, Lit, LitStr, Meta, Result, Type,
    parse_macro_input,
};

#[proc_macro_derive(Fields, attributes(fields, field))]
pub fn derive_fields(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let code = expand(&input).unwrap_or_else(Error::into_compile_error);
    code.into()
}

/// The path of the core crate in the crate that is compiled.
// An engine crate names `maki-core`. A script crate of a project names `maki-engine`
// only, and has the core crate as `maki_engine::core`. A macro cannot ask which
// names a crate has: this reads the manifest of the crate, as the macros of
// bevy_ecs do.
fn core_path() -> Tokens {
    let variable = |name: &str| std::env::var(name).unwrap_or_default();
    let manifest = std::path::Path::new(&variable("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = std::fs::read_to_string(manifest).unwrap_or_default();
    let names_core = manifest
        .lines()
        .any(|line| line.trim_start().starts_with("maki-core"));
    // The core crate has itself under its name (`extern crate self`).
    if names_core || variable("CARGO_PKG_NAME") == "maki-core" {
        quote!(::maki_core)
    } else {
        quote!(::maki_engine::core)
    }
}

/// What `#[field(...)]` says of one field or one variant.
#[derive(Default)]
struct Hints {
    skip: bool,
    label: Option<String>,
    default: Option<Expr>,
    some: Option<Expr>,
    /// Calls that change the kind of the field: `.range(0.0, 1.0)`, `.color()`.
    kind: Vec<Tokens>,
}

fn hints(attributes: &[Attribute]) -> Result<Hints> {
    let mut hints = Hints::default();
    let marked = attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("field"));
    for attribute in marked {
        attribute.parse_nested_meta(|meta| {
            let name = meta.path.get_ident().map(ToString::to_string);
            match name.unwrap_or_default().as_str() {
                "skip" => hints.skip = true,
                "color" => hints.kind.push(quote!(.color())),
                "label" => hints.label = Some(meta.value()?.parse::<LitStr>()?.value()),
                "default" => hints.default = Some(meta.value()?.parse()?),
                "some" => hints.some = Some(meta.value()?.parse()?),
                "step" => {
                    let step: Expr = meta.value()?.parse()?;
                    hints.kind.push(quote!(.step(#step)));
                }
                "file" => {
                    let group: LitStr = meta.value()?.parse()?;
                    hints.kind.push(quote!(.file(#group)));
                }
                "pick" => {
                    let list: LitStr = meta.value()?.parse()?;
                    hints.kind.push(quote!(.pick(#list)));
                }
                "range" => {
                    let range: Expr = meta.value()?.parse()?;
                    let ends = match &range {
                        Expr::Range(range) => range.start.as_ref().zip(range.end.as_ref()),
                        _ => None,
                    };
                    let Some((start, end)) = ends else {
                        return Err(meta.error("a range with two ends: `range = 0.0..=1.0`"));
                    };
                    hints.kind.push(quote!(.range(#start, #end)));
                }
                _ => {
                    return Err(meta.error(
                        "not a hint of `field`. The hints: skip, label, default, some, \
                         range, step, color, file, pick",
                    ));
                }
            }
            Ok(())
        })?;
    }
    Ok(hints)
}

/// The doc comment of a field or a variant, as one line.
fn help(attributes: &[Attribute]) -> String {
    let lines = attributes.iter().filter_map(|attribute| {
        let Meta::NameValue(pair) = &attribute.meta else {
            return None;
        };
        let Expr::Lit(ExprLit {
            lit: Lit::Str(text),
            ..
        }) = &pair.value
        else {
            return None;
        };
        attribute
            .path()
            .is_ident("doc")
            .then(|| text.value().trim().to_owned())
    });
    lines.collect::<Vec<String>>().join(" ")
}

/// The code of one `Field` of a table. `None`: the field is skipped.
/// `label`: the label with no hint. `of_default`: code that gives this field of the
/// default of its type.
fn field_code(
    core: &Tokens,
    name: &str,
    label: String,
    ty: &Type,
    attributes: &[Attribute],
    of_default: Option<Tokens>,
) -> Result<Option<Tokens>> {
    let hints = hints(attributes)?;
    if hints.skip {
        return Ok(None);
    }
    let label = hints.label.unwrap_or(label);
    let help = help(attributes);
    let changes = &hints.kind;
    // The value of the hint has the type of the field: `30.0` for an `Option<f32>`.
    let some = hints.some.map(|value| {
        quote!(.some({
            let value: #ty = ::core::option::Option::Some(#value);
            #core::fields::Fields::tree(&value)
        }))
    });
    let default = match (hints.default, of_default) {
        (Some(value), _) => quote!(::core::option::Option::Some({
            let value: #ty = #value;
            #core::fields::Fields::tree(&value)
        })),
        (None, Some(value)) => {
            quote!(::core::option::Option::Some(#core::fields::Fields::tree(&#value)))
        }
        (None, None) => quote!(::core::option::Option::None),
    };
    Ok(Some(quote! {
        #core::fields::Field {
            name: #name,
            label: #label,
            help: #help,
            kind: <#ty as #core::fields::Fields>::kind() #(#changes)* #some,
            default: #default,
        }
    }))
}

fn expand(input: &DeriveInput) -> Result<Tokens> {
    let core = core_path();
    let (kind, tree) = match &input.data {
        Data::Struct(data) => expand_struct(&core, input, data)?,
        Data::Enum(data) => expand_enum(&core, data)?,
        Data::Union(_) => {
            return Err(Error::new_spanned(
                input,
                "`Fields` is for a struct or an enum",
            ));
        }
    };
    let name = &input.ident;
    let (generics, of_type, bounds) = input.generics.split_for_impl();
    Ok(quote! {
        impl #generics #core::fields::Fields for #name #of_type #bounds {
            fn kind() -> #core::fields::Kind {
                #kind
            }

            fn tree(&self) -> #core::fields::Value {
                #tree
            }
        }
    })
}

/// True: the type has `#[fields(default)]`.
fn has_default(input: &DeriveInput) -> Result<bool> {
    let mut found = false;
    let marked = input
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("fields"));
    for attribute in marked {
        attribute.parse_nested_meta(|meta| {
            if !meta.path.is_ident("default") {
                return Err(meta.error("not a hint of `fields`. The hint: default"));
            }
            found = true;
            Ok(())
        })?;
    }
    Ok(found)
}

/// The bodies of `kind` and `tree` for a struct.
fn expand_struct(
    core: &Tokens,
    input: &DeriveInput,
    data: &syn::DataStruct,
) -> Result<(Tokens, Tokens)> {
    let syn::Fields::Named(named) = &data.fields else {
        let reason = "`Fields` is for a struct with named fields";
        return Err(Error::new_spanned(&data.fields, reason));
    };
    let has_default = has_default(input)?;
    let (mut rows, mut entries) = (Vec::new(), Vec::new());
    for field in &named.named {
        let ident = field.ident.as_ref().expect("a named field");
        let name = ident.to_string();
        let of_default = has_default.then(|| quote!(default.#ident));
        let label = name.replace('_', " ");
        let row = field_code(core, &name, label, &field.ty, &field.attrs, of_default)?;
        let Some(row) = row else {
            continue;
        };
        rows.push(row);
        entries.push(quote! {
            map.insert(#name.to_owned(), #core::fields::Fields::tree(&self.#ident));
        });
    }
    let default =
        has_default.then(|| quote!(let default = <Self as ::core::default::Default>::default();));
    let kind = quote! {
        #default
        #core::fields::Kind::Group(::std::vec![#(#rows),*])
    };
    let tree = quote! {
        let mut map = #core::fields::Map::new();
        #(#entries)*
        #core::fields::Value::Object(map)
    };
    Ok((kind, tree))
}

/// The bodies of `kind` and `tree` for an enum.
fn expand_enum(core: &Tokens, data: &syn::DataEnum) -> Result<(Tokens, Tokens)> {
    let (mut variants, mut arms) = (Vec::new(), Vec::new());
    for variant in &data.variants {
        let ident = &variant.ident;
        let name = ident.to_string();
        let hints = hints(&variant.attrs)?;
        let label = hints.label.unwrap_or_else(|| name.clone());
        let help = help(&variant.attrs);
        let default = match hints.default {
            Some(value) => quote!(::core::option::Option::Some({
                let value: Self = #value;
                #core::fields::Fields::tree(&value)
            })),
            None => quote!(::core::option::Option::None),
        };

        // The fields of the variant, the names that an arm of `match` gives them,
        // and the tree of each.
        let (mut rows, mut names, mut trees) = (Vec::new(), Vec::new(), Vec::new());
        for (index, field) in variant.fields.iter().enumerate() {
            // A field with no name has no label of its own.
            let (name, label, binding) = match &field.ident {
                Some(ident) => (
                    ident.to_string(),
                    ident.to_string().replace('_', " "),
                    ident.clone(),
                ),
                None => (
                    index.to_string(),
                    String::new(),
                    format_ident!("field_{index}"),
                ),
            };
            let row = field_code(core, &name, label, &field.ty, &field.attrs, None)?;
            let Some(row) = row else {
                let reason = "a field of a variant cannot be skipped";
                return Err(Error::new_spanned(field, reason));
            };
            rows.push(row);
            trees.push(quote!(#core::fields::Fields::tree(#binding)));
            names.push((name, binding));
        }
        let bindings = names.iter().map(|(_, binding)| binding);
        // The forms of serde for an enum: `"Name"`, `{"Name": value}`,
        // `{"Name": [values]}`, `{"Name": {field: value}}`.
        let (form, pattern, content) = match &variant.fields {
            syn::Fields::Unit => (quote!(Unit), quote!(), None),
            syn::Fields::Unnamed(_) if trees.len() == 1 => (
                quote!(Tuple),
                quote!((#(#bindings),*)),
                Some(quote!(#(#trees)*)),
            ),
            syn::Fields::Unnamed(_) => (
                quote!(Tuple),
                quote!((#(#bindings),*)),
                Some(quote!(#core::fields::Value::Array(::std::vec![#(#trees),*]))),
            ),
            syn::Fields::Named(_) => {
                let keys = names.iter().map(|(name, _)| name);
                let content = quote!({
                    let mut map = #core::fields::Map::new();
                    #(map.insert(#keys.to_owned(), #trees);)*
                    #core::fields::Value::Object(map)
                });
                (quote!(Struct), quote!({ #(#bindings),* }), Some(content))
            }
        };
        arms.push(match content {
            Some(content) => {
                quote!(Self::#ident #pattern => #core::fields::variant_tree(#name, #content),)
            }
            None => quote!(Self::#ident => #core::fields::Value::String(#name.to_owned()),),
        });
        variants.push(quote! {
            #core::fields::Variant {
                name: #name,
                label: #label,
                help: #help,
                form: #core::fields::Form::#form,
                fields: ::std::vec![#(#rows),*],
                default: #default,
            }
        });
    }
    let kind = quote!(#core::fields::Kind::Choice(::std::vec![#(#variants),*]));
    let tree = quote!(match self { #(#arms)* });
    Ok((kind, tree))
}
