use crate::{
    parse::{Delimited, KeyValue, Paren},
    route::EqStr,
    util::ItemImplExt,
};
use pmutil::{q, ToTokensExt};
use proc_macro2::{Ident, TokenStream};
use syn::{
    parse2,
    punctuated::{Pair, Punctuated},
    Attribute, Block, Data, DeriveInput, Expr, Field, FieldValue, Fields, GenericParam, ItemImpl,
    Lit, Meta, Stmt, Token, TraitBound, TraitBoundModifier, TypeParamBound,
};

/// Search for `#[serde(rename = '')]`
fn get_rename(attrs: &[Attribute]) -> Option<String> {
    let attr = attrs.iter().find(|attr| {
        //
        if !attr.path.is_ident("serde") {
            return false;
        }

        parse2(attr.tokens.clone())
    })?;
}

fn field_name(type_attrs: &[Attribute], field: &Field) {}

fn extract_example(attrs: &mut Vec<Attribute>) -> Option<TokenStream> {
    let mut v = None;

    attrs.iter().find(|attr| {
        if attr.path.is_ident("schema") {
            for config in parse2::<Paren<Delimited<Meta>>>(attr.tokens.clone())
                .expect("invalid schema config found while extracting example")
                .inner
                .inner
            {
                match config {
                    Meta::Path(v) => unimplemented!("#[schema]: Meta::Path({})", v.dump()),
                    Meta::List(v) => unimplemented!("#[schema]: Meta::List({})", v.dump()),
                    Meta::NameValue(n) => {
                        if n.path.is_ident("example") {
                            assert!(
                                v.is_none(),
                                "duplicate #[schema(example = \"foo\")] detected"
                            );

                            v = Some(match n.lit {
                                Lit::Str(s) => s
                                    .value()
                                    .parse::<TokenStream>()
                                    .expect("expected example to be path"),
                                l => panic!(
                                    "#[schema(example = \"foo\")]: value of example should be a \
                                     string literal, but got {}",
                                    l.dump()
                                ),
                            });
                        }
                    }
                }
            }
        }

        true
    });

    let v = v?;
    match syn::parse2::<Lit>(v.clone()) {
        Ok(v) => {
            let v = match v {
                Lit::Str(v) => q!(Vars { v }, { String(v.into()) }),
                Lit::ByteStr(_) => panic!("byte string is not a valid example"),
                Lit::Byte(_) => panic!("byte is not a valid example"),
                Lit::Char(v) => q!(Vars { v }, { String(v.into()) }),
                Lit::Int(v) => q!(Vars { v }, { Number(v.into()) }),
                Lit::Float(v) => q!(Vars { v }, { Number(v.into()) }),
                Lit::Bool(v) => q!(Vars { v }, { Bool(v) }),
                Lit::Verbatim(_) => unimplemented!("Verbatim?"),
            };

            Some(q!(Vars { v }, (rweb::rt::serde_json::Value::v)).into())
        }
        Err(..) => Some(v),
    }
}

fn extract_doc(attrs: &mut Vec<Attribute>) -> String {
    let mut doc = None;
    let mut comments = String::new();

    attrs.retain(|attr| {
        if attr.path.is_ident("schema") {
            let v = match parse2::<Paren<KeyValue<Ident>>>(attr.tokens.clone()) {
                Ok(v) if v.inner.key == "description" => v.inner.value.value(),
                _ => return true,
            };
            doc = Some(v);
            return false;
        }

        if attr.path.is_ident("doc") {
            let v = match parse2::<EqStr>(attr.tokens.clone()) {
                Ok(v) => v.value,
                _ => return true,
            };

            if !comments.is_empty() {
                comments.push(' ');
            }
            comments.push_str(&v.value());
        }

        true
    });

    match doc {
        Some(v) => v,
        None => comments,
    }
}

fn handle_field(f: &mut Field) -> Stmt {
    let i = f.ident.as_ref().unwrap();

    let desc = extract_doc(&mut f.attrs);
    let example_v = extract_example(&mut f.attrs);

    q!(
        Vars {
            name: i,
            desc,
            Type: &f.ty,
            example_v: super::quote_option(example_v),
        },
        {
            map.insert(rweb::rt::Cow::Borrowed(stringify!(name)), {
                {
                    #[allow(unused_mut)]
                    let mut s = <Type as rweb::openapi::Entity>::describe();
                    let description = desc;
                    if !description.is_empty() {
                        s.description = rweb::rt::Cow::Borrowed(description);
                    }
                    let example = example_v;
                    if let Some(example) = example {
                        s.example = Some(example);
                    }
                    s
                }
            });
        }
    )
    .parse()
}

fn handle_fields(fields: &mut Fields) -> Block {
    // Properties
    let mut block: Block = q!({ {} }).parse();
    block.stmts.push(
        q!({
            #[allow(unused_mut)]
            let mut map: rweb::rt::BTreeMap<rweb::rt::Cow<'static, str>, _> =
                rweb::rt::BTreeMap::default();
        })
        .parse(),
    );

    for f in fields {
        block.stmts.push(handle_field(f));
    }

    block.stmts.push(Stmt::Expr(q!({ map }).parse()));

    block
}

pub fn derive_schema(mut input: DeriveInput) -> TokenStream {
    let desc = extract_doc(&mut input.attrs);

    let mut component = None;
    let example = extract_example(&mut input.attrs);

    input.attrs.retain(|attr| {
        if attr.path.is_ident("schema") {
            for config in parse2::<Paren<Delimited<Meta>>>(attr.tokens.clone())
                .expect("schema config of type is invalid")
                .inner
                .inner
            {
                match config {
                    Meta::Path(..) => unimplemented!("Meta::Path in #[schema]"),
                    Meta::NameValue(n) => {
                        //

                        if n.path.is_ident("component") {
                            assert!(
                                component.is_none(),
                                "duplicate #[schema(component = \"foo\")] detected"
                            );
                            component = Some(match n.lit {
                                Lit::Str(s) => s.value(),
                                l => panic!(
                                    "#[schema]: value of component should be a string literal, \
                                     but got {}",
                                    l.dump()
                                ),
                            })
                        } else {
                            panic!("#[schema]: Unknown option {}", n.path.dump())
                        }
                    }
                    Meta::List(l) => unimplemented!("Meta::List in #[schema]: {}", l.dump()),
                }
            }
        }

        true
    });

    let mut fields: Punctuated<FieldValue, Token![,]> = Default::default();
    if let Some(tts) = example {
        fields.push(q!(Vars { tts }, ({ example: Some(tts) })).parse());
    }

    match input.data {
        Data::Struct(ref mut data) => {
            match data.fields {
                Fields::Named(_) => {
                    let block = handle_fields(&mut data.fields);
                    fields.push(q!(Vars { block }, { properties: block }).parse());
                }
                Fields::Unnamed(ref n) if n.unnamed.len() == 1 => {}
                _ => {}
            }

            fields.push(q!({ schema_type: Some(rweb::openapi::Type::Object) }).parse());
        }
        Data::Enum(ref mut data) => {
            if data
                .variants
                .iter()
                .all(|variant| variant.fields.len() == 0)
            {
                // c-like enums

                let exprs: Punctuated<Expr, Token![,]> = data
                    .variants
                    .iter()
                    .map(|variant| {
                        //

                        let name = &variant.ident.to_string();
                        Pair::Punctuated(
                            q!(Vars { name }, { rweb::rt::Cow::Borrowed(name) }).parse(),
                            Default::default(),
                        )
                    })
                    .collect();

                fields.push(q!(Vars { exprs }, { enum_values: vec![exprs] }).parse());
            } else {
                let exprs: Punctuated<Expr, Token![,]> = data
                    .variants
                    .iter_mut()
                    .filter_map(|v| {
                        let desc = extract_doc(&mut v.attrs);

                        match v.fields {
                            Fields::Named(..) => Some(Pair::Punctuated(
                                {
                                    let fields = handle_fields(&mut v.fields);
                                    q!(
                                        Vars { fields, desc },
                                        ({
                                            #[allow(unused_mut)]
                                            let mut s = rweb::openapi::Schema {
                                                properties: fields,
                                                ..rweb::rt::Default::default()
                                            };
                                            let description = desc;
                                            if !description.is_empty() {
                                                s.description =
                                                    rweb::rt::Cow::Borrowed(description);
                                            }

                                            rweb::openapi::ObjectOrReference::Object(s)
                                        })
                                    )
                                    .parse()
                                },
                                Default::default(),
                            )),
                            Fields::Unnamed(ref f) => {
                                //
                                assert!(f.unnamed.len() <= 1);
                                if f.unnamed.len() == 0 {
                                    return None;
                                }

                                Some(Pair::Punctuated(
                                    q!(
                                        Vars {
                                            Type: &f.unnamed.first().unwrap().ty,
                                            desc
                                        },
                                        ({
                                            #[allow(unused_mut)]
                                            let mut s = <Type as rweb::openapi::Entity>::describe();
                                            let description = desc;
                                            if !description.is_empty() {
                                                s.description =
                                                    rweb::rt::Cow::Borrowed(description);
                                            }

                                            rweb::openapi::ObjectOrReference::Object(s)
                                        })
                                    )
                                    .parse(),
                                    Default::default(),
                                ))
                            }
                            Fields::Unit => None,
                        }
                    })
                    .collect();

                fields.push(q!(Vars { exprs }, { one_of: vec![exprs] }).parse());
            }
        }
        Data::Union(_) => unimplemented!("#[derive(Schema)] for union"),
    }

    let mut item = if let Some(comp) = component {
        let path_to_schema = format!("#/components/schemas/{}", comp);

        q!(
            Vars {
                Type: &input.ident,
                desc,
                path_to_schema,
                fields,
                comp,
            },
            {
                impl rweb::openapi::Entity for Type {
                    fn describe() -> rweb::openapi::Schema {
                        rweb::openapi::Schema {
                            ref_path: rweb::rt::Cow::Borrowed(path_to_schema),
                            ..rweb::rt::Default::default()
                        }
                    }

                    fn describe_components() -> rweb::openapi::Components {
                        vec![(
                            rweb::rt::Cow::Borrowed(comp),
                            rweb::openapi::Schema {
                                fields,
                                description: rweb::rt::Cow::Borrowed(desc),
                                ..rweb::rt::Default::default()
                            },
                        )]
                    }
                }
            }
        )
    } else {
        q!(
            Vars {
                Type: &input.ident,
                desc,
                fields
            },
            {
                impl rweb::openapi::Entity for Type {
                    fn describe() -> rweb::openapi::Schema {
                        rweb::openapi::Schema {
                            fields,
                            description: rweb::rt::Cow::Borrowed(desc),
                            ..rweb::rt::Default::default()
                        }
                    }
                }
            }
        )
    }
    .parse::<ItemImpl>()
    .with_generics(input.generics.clone());

    for param in item.generics.params.iter_mut() {
        match param {
            GenericParam::Type(ref mut ty) => ty.bounds.push(TypeParamBound::Trait(TraitBound {
                paren_token: None,
                modifier: TraitBoundModifier::None,
                lifetimes: None,
                path: q!({ rweb::openapi::Entity }).parse(),
            })),
            _ => continue,
        }
    }

    item.dump()
}