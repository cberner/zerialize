//! Procedural macro implementation for the `zerialize` crate.
//!
//! See the `zerialize` crate for documentation; this crate is an implementation
//! detail and its macros are re-exported from there.

#![forbid(unsafe_code)]

use proc_macro::{Delimiter, Group, Span, TokenStream, TokenTree};
use std::fmt::Write as _;

/// Appends a line of generated source to a `String`.
macro_rules! emit {
    ($out:expr, $($arg:tt)*) => {
        writeln!($out, $($arg)*).expect("writing to a String cannot fail")
    };
}

/// Turns a trait into a zero-copy serialization schema.
///
/// Every method must declare the slot it occupies with `#[slot(N)]`. Slots are
/// the identity of a field on the wire, so renaming or reordering methods is
/// safe, and a reader skips slots it does not know about.
///
/// The macro generates, alongside the trait:
///
/// * `{Trait}View`, the zero-copy view decoding returns, which borrows from the
///   input buffer and implements the trait itself,
/// * an implementation of the trait for `&T`, so that implementations can
///   return references to nested values,
/// * `impl Zerializable for dyn Trait`, which is how `dyn Trait` comes to name
///   the schema in `encode::<dyn Trait>()` and `decode::<dyn Trait>()`.
///
/// Methods may return a scalar, `&str`, `&[u8]`, a nested schema as
/// `impl Trait + '_`, or a sequence of them as
/// `impl List<Item = impl Trait + '_> + '_`. The last two return `impl Trait`,
/// so they must be declared `where Self: Sized` to keep `dyn Trait` usable as
/// the schema's name.
#[proc_macro_attribute]
pub fn zerializable(args: TokenStream, item: TokenStream) -> TokenStream {
    // `#[slot(N)]` is consumed here, so it must be stripped from the trait even
    // when the rest of the expansion fails, or the reported error would be a
    // confusing "cannot find attribute `slot`".
    let mut output = strip_slot_attributes(item.clone());
    match expand(args, item) {
        Ok(generated) => output.extend(generated),
        Err(error) => output.extend(error.into_tokens()),
    }
    output
}

fn expand(args: TokenStream, item: TokenStream) -> Result<TokenStream, Error> {
    if let Some(token) = args.into_iter().next() {
        return Err(Error::new(
            token.span(),
            "#[zerializable] does not take any arguments",
        ));
    }
    let schema = parse_schema(&item.into_iter().collect::<Vec<_>>())?;
    Ok(generate(&schema)
        .parse()
        .expect("generated code is valid Rust"))
}

// ============================================================
// Schema
// ============================================================

struct Schema {
    visibility: String,
    name: String,
    methods: Vec<Method>,
}

struct Method {
    name: String,
    slot: u32,
    /// The trait's declaration of this method, reused verbatim so that the
    /// generated implementations are guaranteed to match it.
    signature: String,
    kind: Kind,
}

enum Kind {
    Str,
    Bytes,
    /// A fixed width primitive, named by its Rust type.
    Scalar(String),
    /// A nested message, named by the path of its schema trait.
    Nested(String),
    /// A sequence of nested messages.
    Repeated(String),
}

/// Names a schema by its trait, spelling out the `'static` object lifetime that
/// `impl Zerializable for dyn Trait` is written against. Without it the default
/// object lifetime in a return type is the one elided from `&self`.
fn schema_of(path: &str) -> String {
    format!("(dyn {path} + 'static)")
}

fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

/// The view type of another schema, named through its `Zerializable` impl so
/// that nested schemas do not have to live in the same module.
fn view_of(path: &str) -> String {
    format!(
        "<{} as ::zerialize::Zerializable>::View<'buf>",
        schema_of(path)
    )
}

const SCALARS: [&str; 11] = [
    "bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64",
];

// ============================================================
// Parsing
// ============================================================

fn parse_schema(tokens: &[TokenTree]) -> Result<Schema, Error> {
    let mut index = 0;
    while is_punct(tokens.get(index), '#') {
        index += 2;
    }

    let mut visibility = String::new();
    if is_ident(tokens.get(index), "pub") {
        visibility.push_str("pub");
        index += 1;
        if let Some(TokenTree::Group(group)) = tokens.get(index)
            && group.delimiter() == Delimiter::Parenthesis
        {
            visibility.push_str(&group.to_string());
            index += 1;
        }
    }

    if !is_ident(tokens.get(index), "trait") {
        return Err(Error::new(
            span_of(tokens.get(index)),
            "#[zerializable] may only be applied to a trait",
        ));
    }
    index += 1;

    let name = match tokens.get(index) {
        Some(TokenTree::Ident(ident)) => ident.to_string(),
        other => return Err(Error::new(span_of(other), "expected a trait name")),
    };
    index += 1;

    let body = match tokens.get(index) {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => group,
        other => {
            return Err(Error::new(
                span_of(other),
                "#[zerializable] traits may not have generic parameters, supertraits, \
                 or a where clause",
            ));
        }
    };

    let methods = parse_methods(body)?;
    Ok(Schema {
        visibility,
        name,
        methods,
    })
}

fn parse_methods(body: &Group) -> Result<Vec<Method>, Error> {
    let tokens: Vec<TokenTree> = body.stream().into_iter().collect();
    let mut methods: Vec<Method> = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        let mut slot: Option<(u32, Span)> = None;
        while is_punct(tokens.get(index), '#') {
            let attribute = match tokens.get(index + 1) {
                Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket => group,
                other => return Err(Error::new(span_of(other), "expected an attribute")),
            };
            if let Some(value) = parse_slot(attribute)? {
                slot = Some((value, attribute.span()));
            }
            index += 2;
        }

        let start = index;
        if !is_ident(tokens.get(index), "fn") {
            return Err(Error::new(
                span_of(tokens.get(index)),
                "#[zerializable] traits may only contain methods",
            ));
        }
        index += 1;

        let name = match tokens.get(index) {
            Some(TokenTree::Ident(ident)) => ident.to_string(),
            other => return Err(Error::new(span_of(other), "expected a method name")),
        };
        index += 1;

        match tokens.get(index) {
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
                let arguments: Vec<TokenTree> = group.stream().into_iter().collect();
                if !(arguments.len() == 2
                    && is_punct(arguments.first(), '&')
                    && is_ident(arguments.get(1), "self"))
                {
                    return Err(Error::new(
                        group.span(),
                        "#[zerializable] methods must take `&self` and no other arguments",
                    ));
                }
            }
            other => return Err(Error::new(span_of(other), "expected an argument list")),
        }
        index += 1;

        if !(is_punct(tokens.get(index), '-') && is_punct(tokens.get(index + 1), '>')) {
            return Err(Error::new(
                span_of(tokens.get(index)),
                "#[zerializable] methods must return a value",
            ));
        }
        index += 2;

        let return_start = index;
        while index < tokens.len() && !is_signature_end(tokens.get(index)) {
            index += 1;
        }
        let return_type = &tokens[return_start..index];
        let return_span = span_of(tokens.get(return_start));

        let where_start = index;
        if is_ident(tokens.get(index), "where") {
            index += 1;
            while index < tokens.len() && !is_signature_end(tokens.get(index)) {
                index += 1;
            }
        }
        let where_clause = &tokens[where_start..index];

        match tokens.get(index) {
            Some(TokenTree::Punct(punct)) if punct.as_char() == ';' => index += 1,
            other => {
                return Err(Error::new(
                    span_of(other),
                    "#[zerializable] methods may not have a default body",
                ));
            }
        }

        let kind = parse_return_type(return_type, return_span)?;
        if matches!(kind, Kind::Nested(_) | Kind::Repeated(_))
            && !where_clause
                .iter()
                .any(|token| is_ident(Some(token), "Sized"))
        {
            return Err(Error::new(
                return_span,
                "methods returning `impl Trait` must be declared `where Self: Sized`, \
                 so that `dyn Trait` stays dyn compatible",
            ));
        }

        let Some((slot, slot_span)) = slot else {
            return Err(Error::new(
                span_of(tokens.get(start)),
                "every #[zerializable] method requires a #[slot(N)] attribute",
            ));
        };
        if let Some(previous) = methods.iter().find(|method| method.slot == slot) {
            return Err(Error::new(
                slot_span,
                &format!("slot {slot} is already used by `{}`", previous.name),
            ));
        }

        methods.push(Method {
            signature: format!(
                "fn {name}(&self) -> {} {}",
                to_source(return_type),
                to_source(where_clause)
            ),
            name,
            slot,
            kind,
        });
    }

    Ok(methods)
}

/// Returns the slot number of a `#[slot(N)]` attribute, or `None` for any other
/// attribute, which is left on the method untouched.
fn parse_slot(attribute: &Group) -> Result<Option<u32>, Error> {
    let tokens: Vec<TokenTree> = attribute.stream().into_iter().collect();
    if !is_ident(tokens.first(), "slot") {
        return Ok(None);
    }
    let invalid = || Error::new(attribute.span(), "expected `#[slot(N)]`, where N is a u32");
    if tokens.len() != 2 {
        return Err(invalid());
    }
    match &tokens[1] {
        TokenTree::Group(group) if group.delimiter() == Delimiter::Parenthesis => group
            .stream()
            .to_string()
            .parse()
            .map(Some)
            .map_err(|_| invalid()),
        _ => Err(invalid()),
    }
}

fn parse_return_type(tokens: &[TokenTree], span: Span) -> Result<Kind, Error> {
    let unsupported = || {
        Error::new(
            span,
            "unsupported return type: expected a scalar, `&str`, `&[u8]`, \
             `impl Trait + '_`, or `impl List<Item = impl Trait + '_> + '_`",
        )
    };

    match tokens.first() {
        Some(TokenTree::Punct(punct)) if punct.as_char() == '&' => {
            let rest = skip_lifetime(&tokens[1..]);
            if rest.len() != 1 {
                return Err(unsupported());
            }
            match &rest[0] {
                TokenTree::Ident(ident) if ident.to_string() == "str" => Ok(Kind::Str),
                TokenTree::Group(group) if group.delimiter() == Delimiter::Bracket => {
                    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                    if inner.len() == 1 && is_ident(inner.first(), "u8") {
                        Ok(Kind::Bytes)
                    } else {
                        Err(unsupported())
                    }
                }
                _ => Err(unsupported()),
            }
        }
        Some(TokenTree::Ident(ident)) if ident.to_string() == "impl" => {
            let (path, rest) = parse_impl_trait(tokens, span)?;
            if last_segment(&path) == "List" {
                Ok(Kind::Repeated(parse_list_item(rest, span)?))
            } else {
                Ok(Kind::Nested(path))
            }
        }
        Some(TokenTree::Ident(ident))
            if tokens.len() == 1 && SCALARS.contains(&ident.to_string().as_str()) =>
        {
            Ok(Kind::Scalar(ident.to_string()))
        }
        _ => Err(unsupported()),
    }
}

/// Extracts the trait path out of `impl Path ...`, returning it along with
/// whatever follows it. Any lifetime bound is left behind: the generated code
/// does not need it, because a view borrows from the buffer, not the source.
fn parse_impl_trait(tokens: &[TokenTree], span: Span) -> Result<(String, &[TokenTree]), Error> {
    let mut path = String::new();
    let mut index = 1;
    while let Some(token) = tokens.get(index) {
        match token {
            TokenTree::Ident(ident) => path.push_str(&ident.to_string()),
            TokenTree::Punct(punct) if punct.as_char() == ':' => path.push(':'),
            _ => break,
        }
        index += 1;
    }
    if path.is_empty() {
        return Err(Error::new(span, "expected a trait path after `impl`"));
    }
    Ok((path, &tokens[index..]))
}

/// Extracts the element schema out of the `<Item = impl Path + '_>` that
/// follows `impl List`.
fn parse_list_item(tokens: &[TokenTree], span: Span) -> Result<String, Error> {
    let expected = || {
        Error::new(
            span,
            "expected `impl List<Item = impl Trait + '_> + '_`, naming the schema \
             the list holds",
        )
    };
    if !(is_punct(tokens.first(), '<')
        && is_ident(tokens.get(1), "Item")
        && is_punct(tokens.get(2), '='))
    {
        return Err(expected());
    }
    let item = tokens.get(3..).ok_or_else(expected)?;
    if !is_ident(item.first(), "impl") {
        return Err(expected());
    }
    Ok(parse_impl_trait(item, span)?.0)
}

// ============================================================
// Code generation
// ============================================================

fn generate(schema: &Schema) -> String {
    const VALIDATED: &str = "the message was validated when it was decoded";
    let Schema {
        visibility,
        name,
        methods,
    } = schema;
    let view = format!("{name}View");
    let source = format!("__{name}Source");
    // The offset table is indexed by slot, so it is as long as the highest one.
    let slots = methods
        .iter()
        .map(|method| method.slot as usize + 1)
        .max()
        .unwrap_or(0);
    let mut out = String::new();

    // The object safe adapter that gives `encode::<dyn Trait>(&value)` a single
    // dynamic call to dispatch on, rather than one per field.
    emit!(out, "#[doc(hidden)]");
    emit!(out, "{visibility} trait {source} {{");
    emit!(
        out,
        "    fn __zerialize_encode(&self, __writer: &mut ::zerialize::Writer);"
    );
    emit!(out, "}}");

    emit!(out, "impl<__S: {name}> {source} for __S {{");
    emit!(
        out,
        "    fn __zerialize_encode(&self, __writer: &mut ::zerialize::Writer) {{"
    );
    emit!(
        out,
        "        let __frame = __writer.begin_frame({slots}usize);"
    );
    for method in methods {
        emit!(
            out,
            "        __writer.begin_entry(&__frame, {}usize);",
            method.slot
        );
        let value = format!("<Self as {name}>::{}(self)", method.name);
        match &method.kind {
            Kind::Str => emit!(out, "        __writer.write_str({value});"),
            Kind::Bytes => emit!(out, "        __writer.write_bytes({value});"),
            Kind::Scalar(ty) => emit!(out, "        __writer.write_{ty}({value});"),
            Kind::Nested(path) => {
                emit!(out, "        let __value = {value};");
                emit!(
                    out,
                    "        <{} as ::zerialize::Zerializable>::encode_source(&__value, __writer);",
                    schema_of(path)
                );
            }
            Kind::Repeated(path) => {
                emit!(out, "        let __items = {value};");
                emit!(
                    out,
                    "        let __length = ::zerialize::List::len(&__items);"
                );
                emit!(out, "        let __list = __writer.begin_frame(__length);");
                emit!(out, "        for __index in 0..__length {{");
                emit!(
                    out,
                    "            let __item = ::zerialize::List::get(&__items, __index)\
                     .expect(\"List::get returned None below List::len\");"
                );
                emit!(out, "            __writer.begin_entry(&__list, __index);");
                emit!(
                    out,
                    "            <{} as ::zerialize::Zerializable>::encode_source(&__item, __writer);",
                    schema_of(path)
                );
                emit!(out, "        }}");
                emit!(out, "        __writer.end_frame(__list);");
            }
        }
    }
    emit!(out, "        __writer.end_frame(__frame);");
    emit!(out, "    }}");
    emit!(out, "}}");

    // So that an implementation can return `&self.nested` as `impl Trait + '_`.
    emit!(out, "impl<__S: {name}> {name} for &__S {{");
    for method in methods {
        emit!(out, "    {} {{", method.signature);
        emit!(out, "        <__S as {name}>::{}(*self)", method.name);
        emit!(out, "    }}");
    }
    emit!(out, "}}");

    // The view is the bytes of the message and nothing else. Fields are read
    // out of them on access, by indexing the frame's offset table with the
    // slot number, so a view costs the same whatever its schema holds.
    emit!(out, "/// Zero-copy view of a `{name}` message.");
    emit!(out, "///");
    emit!(
        out,
        "/// Returned by `decode::<dyn {name}>`, borrowing from the buffer that"
    );
    emit!(out, "/// was decoded rather than owning its contents.");
    emit!(out, "#[derive(::core::clone::Clone, ::core::marker::Copy)]");
    emit!(out, "#[allow(dead_code)]");
    emit!(out, "{visibility} struct {view}<'buf> {{");
    emit!(out, "    bytes: &'buf [u8],");
    emit!(out, "}}");

    // Inherent accessors shadow the trait's, and return concrete view types
    // rather than the trait's opaque `impl Trait`, which lets callers keep
    // using nested views as views.
    emit!(out, "#[allow(dead_code)]");
    emit!(out, "impl<'buf> {view}<'buf> {{");
    for method in methods {
        let slot = method.slot;
        let (return_type, body) = match &method.kind {
            Kind::Str => (
                "&'buf str".to_string(),
                format!("__message.read_str({slot}u32).expect({VALIDATED:?})"),
            ),
            Kind::Bytes => (
                "&'buf [u8]".to_string(),
                format!("__message.read_bytes({slot}u32).expect({VALIDATED:?})"),
            ),
            Kind::Scalar(ty) => (
                ty.clone(),
                format!("__message.read_{ty}({slot}u32).expect({VALIDATED:?})"),
            ),
            Kind::Nested(path) => (
                view_of(path),
                format!(
                    "<{schema} as ::zerialize::Zerializable>::decode_view(\
                     __message.read_message({slot}u32).expect({VALIDATED:?})).expect({VALIDATED:?})",
                    schema = schema_of(path)
                ),
            ),
            Kind::Repeated(path) => (
                format!("::zerialize::ListView<'buf, {}>", schema_of(path)),
                format!("__message.read_list({slot}u32).expect({VALIDATED:?})"),
            ),
        };
        emit!(
            out,
            "    {visibility} fn {}(&self) -> {return_type} {{",
            method.name
        );
        emit!(
            out,
            "        let __message = ::zerialize::Message::trusted(self.bytes);"
        );
        emit!(out, "        {body}");
        emit!(out, "    }}");
    }
    emit!(out, "}}");

    emit!(out, "impl<'buf> {name} for {view}<'buf> {{");
    for method in methods {
        emit!(out, "    {} {{", method.signature);
        emit!(out, "        {view}::{}(self)", method.name);
        emit!(out, "    }}");
    }
    emit!(out, "}}");

    // A view compares equal to any implementation holding the same data, which
    // is what makes round trips assertable.
    emit!(
        out,
        "impl<__S: {name}> ::core::cmp::PartialEq<__S> for {view}<'_> {{"
    );
    emit!(out, "    fn eq(&self, __other: &__S) -> bool {{");
    for method in methods {
        let mine = format!("{view}::{}(self)", method.name);
        let theirs = format!("<__S as {name}>::{}(__other)", method.name);
        match &method.kind {
            Kind::Repeated(_) => {
                emit!(out, "        let __mine = {mine};");
                emit!(out, "        let __theirs = {theirs};");
                emit!(
                    out,
                    "        if ::zerialize::List::len(&__mine) \
                     != ::zerialize::List::len(&__theirs) {{"
                );
                emit!(out, "            return false;");
                emit!(out, "        }}");
                emit!(
                    out,
                    "        for (__left, __right) in ::zerialize::List::iter(&__mine)\
                     .zip(::zerialize::List::iter(&__theirs)) {{"
                );
                emit!(out, "            if __left != __right {{");
                emit!(out, "                return false;");
                emit!(out, "            }}");
                emit!(out, "        }}");
            }
            _ => {
                emit!(out, "        if {mine} != {theirs} {{");
                emit!(out, "            return false;");
                emit!(out, "        }}");
            }
        }
    }
    emit!(out, "        true");
    emit!(out, "    }}");
    emit!(out, "}}");

    // Debug reads the fields rather than the bytes, so a view prints as the
    // message it stands for.
    emit!(out, "impl ::core::fmt::Debug for {view}<'_> {{");
    emit!(
        out,
        "    fn fmt(&self, __formatter: &mut ::core::fmt::Formatter<'_>) \
         -> ::core::fmt::Result {{"
    );
    emit!(out, "        __formatter.debug_struct({view:?})");
    for method in methods {
        emit!(
            out,
            "            .field({:?}, &{view}::{}(self))",
            method.name,
            method.name
        );
    }
    emit!(out, "            .finish()");
    emit!(out, "    }}");
    emit!(out, "}}");

    emit!(out, "impl ::zerialize::Zerializable for dyn {name} {{");
    emit!(out, "    type Source<'src> = dyn {source} + 'src;");
    emit!(out, "    type View<'buf> = {view}<'buf>;");
    emit!(
        out,
        "    fn encode_source<'src>(\
         __source: &'src Self::Source<'src>, __writer: &mut ::zerialize::Writer) {{"
    );
    emit!(
        out,
        "        {source}::__zerialize_encode(__source, __writer)"
    );
    emit!(out, "    }}");
    emit!(
        out,
        "    fn decode_view<'buf>(__message: ::zerialize::Message<'buf>) \
         -> ::core::result::Result<Self::View<'buf>, ::zerialize::Error> {{"
    );
    if !methods.is_empty() {
        // Reading every field is what validation is: it leaves the accessors
        // above nothing that can fail.
        emit!(out, "        if __message.validates() {{");
        for method in methods {
            let slot = method.slot;
            match &method.kind {
                Kind::Str => emit!(out, "            __message.read_str({slot}u32)?;"),
                Kind::Bytes => emit!(out, "            __message.read_bytes({slot}u32)?;"),
                Kind::Scalar(ty) => emit!(out, "            __message.read_{ty}({slot}u32)?;"),
                Kind::Nested(path) => emit!(
                    out,
                    "            <{} as ::zerialize::Zerializable>::decode_view(\
                     __message.read_message({slot}u32)?)?;",
                    schema_of(path)
                ),
                Kind::Repeated(path) => emit!(
                    out,
                    "            __message.read_list::<{}>({slot}u32)?;",
                    schema_of(path)
                ),
            }
        }
        emit!(out, "        }}");
    }
    emit!(
        out,
        "        ::core::result::Result::Ok({view} {{ bytes: __message.bytes() }})"
    );
    emit!(out, "    }}");
    emit!(out, "}}");

    out
}

// ============================================================
// Token helpers
// ============================================================

fn is_ident(token: Option<&TokenTree>, expected: &str) -> bool {
    matches!(token, Some(TokenTree::Ident(ident)) if ident.to_string() == expected)
}

fn is_punct(token: Option<&TokenTree>, expected: char) -> bool {
    matches!(token, Some(TokenTree::Punct(punct)) if punct.as_char() == expected)
}

/// Whether a token ends the part of a method declaration the macro reads.
fn is_signature_end(token: Option<&TokenTree>) -> bool {
    is_punct(token, ';')
        || is_ident(token, "where")
        || matches!(token, Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace)
}

fn skip_lifetime(tokens: &[TokenTree]) -> &[TokenTree] {
    if is_punct(tokens.first(), '\'') {
        &tokens[2..]
    } else {
        tokens
    }
}

fn span_of(token: Option<&TokenTree>) -> Span {
    token.map_or_else(Span::call_site, TokenTree::span)
}

fn to_source(tokens: &[TokenTree]) -> String {
    tokens.iter().cloned().collect::<TokenStream>().to_string()
}

fn strip_slot_attributes(tokens: TokenStream) -> TokenStream {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut output: Vec<TokenTree> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if is_punct(tokens.get(index), '#')
            && let Some(TokenTree::Group(group)) = tokens.get(index + 1)
            && group.delimiter() == Delimiter::Bracket
            && is_ident(group.stream().into_iter().next().as_ref(), "slot")
        {
            index += 2;
            continue;
        }
        output.push(match &tokens[index] {
            TokenTree::Group(group) => {
                let mut stripped =
                    Group::new(group.delimiter(), strip_slot_attributes(group.stream()));
                stripped.set_span(group.span());
                TokenTree::Group(stripped)
            }
            other => other.clone(),
        });
        index += 1;
    }
    output.into_iter().collect()
}

// ============================================================
// Errors
// ============================================================

struct Error {
    span: Span,
    message: String,
}

impl Error {
    fn new(span: Span, message: &str) -> Self {
        Self {
            span,
            message: message.to_string(),
        }
    }

    fn into_tokens(self) -> TokenStream {
        let source = format!("::core::compile_error!({:?});", self.message);
        let tokens: TokenStream = source.parse().expect("generated code is valid Rust");
        respan(tokens, self.span)
    }
}

fn respan(tokens: TokenStream, span: Span) -> TokenStream {
    tokens
        .into_iter()
        .map(|token| {
            let mut token = match token {
                TokenTree::Group(group) => {
                    TokenTree::Group(Group::new(group.delimiter(), respan(group.stream(), span)))
                }
                other => other,
            };
            token.set_span(span);
            token
        })
        .collect()
}
