use syn::{
    parenthesized,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    LitStr, Token,
};

pub(crate) struct KeyValue<K = LitStr, V = LitStr> {
    pub key: K,
    _eq: Token![=],
    pub value: V,
}

impl<K, V> Parse for KeyValue<K, V>
where
    K: Parse,
    V: Parse,
{
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        Ok(KeyValue {
            key: input.parse()?,
            _eq: input.parse()?,
            value: input.parse()?,
        })
    }
}

/// A node wrapped with paren.
pub(crate) struct Paren<T> {
    pub inner: T,
}

impl<T> Parse for Paren<T>
where
    T: Parse,
{
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        let content;
        parenthesized!(content in input);
        Ok(Paren {
            inner: content.parse()?,
        })
    }
}

pub(crate) struct Delimited<T> {
    pub inner: Punctuated<T, Token![,]>,
}

impl<T> Parse for Delimited<T>
where
    T: Parse,
{
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        Ok(Delimited {
            inner: Punctuated::parse_separated_nonempty(input)?,
        })
    }
}
