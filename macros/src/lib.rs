//! Procedural macro implementation for the `zerialize` crate.
//!
//! See the `zerialize` crate for documentation; this crate is an implementation
//! detail and its macros are re-exported from there.

#![forbid(unsafe_code)]

use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::{
    Attribute, Error, FnArg, GenericArgument, Ident, Item, ItemTrait, LitInt, Path, PathArguments,
    PathSegment, ReceiverKind, ReturnType, Safety, Signature, TraitBound, TraitItem, TraitItemFn,
    Type, TypeImplTrait, TypeParamBound, TypePath, Visibility, WherePredicate,
};

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
pub fn zerializable(
    args: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut item = match parse_trait(item) {
        Ok(item) => item,
        Err(error) => return error.to_compile_error().into(),
    };
    let expansion = expand(args.into(), &item);
    // `#[slot(N)]` is consumed here, so it must be stripped from the trait even
    // when the rest of the expansion fails, or the reported error would be a
    // confusing "cannot find attribute `slot`".
    strip_slot_attributes(&mut item);
    let mut output = item.into_token_stream();
    output.extend(match expansion {
        Ok(generated) => generated,
        Err(error) => error.to_compile_error(),
    });
    output.into()
}

fn parse_trait(item: proc_macro::TokenStream) -> Result<ItemTrait, Error> {
    match syn::parse(item)? {
        Item::Trait(item) => Ok(item),
        other => Err(Error::new_spanned(
            other,
            "#[zerializable] may only be applied to a trait",
        )),
    }
}

fn expand(args: TokenStream, item: &ItemTrait) -> Result<TokenStream, Error> {
    if !args.is_empty() {
        return Err(Error::new_spanned(
            args,
            "#[zerializable] does not take any arguments",
        ));
    }
    Ok(generate(&parse_schema(item)?))
}

fn strip_slot_attributes(item: &mut ItemTrait) {
    for trait_item in &mut item.items {
        let attributes = match trait_item {
            TraitItem::Const(item) => &mut item.attrs,
            TraitItem::Fn(item) => &mut item.attrs,
            TraitItem::Type(item) => &mut item.attrs,
            TraitItem::Macro(item) => &mut item.attrs,
            _ => continue,
        };
        attributes.retain(|attribute| !attribute.path().is_ident("slot"));
    }
}

// ============================================================
// Schema
// ============================================================

struct Schema<'a> {
    visibility: &'a Visibility,
    name: &'a Ident,
    methods: Vec<Method<'a>>,
}

struct Method<'a> {
    name: &'a Ident,
    slot: u32,
    /// The trait's declaration of this method, reused verbatim so that the
    /// generated implementations are guaranteed to match it.
    signature: &'a Signature,
    kind: Kind<'a>,
}

enum Kind<'a> {
    Str,
    Bytes,
    /// A fixed width primitive, named by its Rust type.
    Scalar(&'a Ident),
    /// A nested message, named by the path of its schema trait.
    Nested(&'a Path),
    /// A sequence of nested messages.
    Repeated(&'a Path),
}

/// Names a schema by its trait, spelling out the `'static` object lifetime that
/// `impl Zerializable for dyn Trait` is written against. Without it the default
/// object lifetime in a return type is the one elided from `&self`.
fn schema_of(path: &Path) -> TokenStream {
    quote!((dyn #path + 'static))
}

/// The view type of another schema, named through its `Zerializable` impl so
/// that nested schemas do not have to live in the same module.
fn view_of(path: &Path) -> TokenStream {
    let schema = schema_of(path);
    quote!(<#schema as ::zerialize::Zerializable>::View<'buf>)
}

const SCALARS: [&str; 11] = [
    "bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64",
];

// ============================================================
// Parsing
// ============================================================

fn parse_schema(item: &ItemTrait) -> Result<Schema<'_>, Error> {
    item.modifiers.require_empty()?;
    if let Some(unsafety) = &item.unsafety {
        return Err(Error::new_spanned(
            unsafety,
            "#[zerializable] traits may not be `unsafe`",
        ));
    }
    if !item.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &item.generics,
            "#[zerializable] traits may not have generic parameters",
        ));
    }
    if let Some(where_clause) = &item.generics.where_clause {
        return Err(Error::new_spanned(
            where_clause,
            "#[zerializable] traits may not have a where clause",
        ));
    }
    if !item.supertraits.is_empty() {
        return Err(Error::new_spanned(
            &item.supertraits,
            "#[zerializable] traits may not have supertraits",
        ));
    }

    let mut methods = Vec::new();
    for trait_item in &item.items {
        let TraitItem::Fn(function) = trait_item else {
            return Err(Error::new_spanned(
                trait_item,
                "#[zerializable] traits may only contain methods",
            ));
        };
        methods.push(parse_method(function, &methods)?);
    }

    Ok(Schema {
        visibility: &item.vis,
        name: &item.ident,
        methods,
    })
}

/// Parses one method, given the methods of the same trait already parsed, which
/// is what a slot is checked for uniqueness against.
fn parse_method<'a>(function: &'a TraitItemFn, parsed: &[Method<'a>]) -> Result<Method<'a>, Error> {
    let mut declared = None;
    for attribute in &function.attrs {
        if attribute.path().is_ident("slot") {
            declared = Some((parse_slot(attribute)?, attribute));
        }
    }

    function.modifiers.require_empty()?;
    if let Some(default) = &function.default {
        return Err(Error::new_spanned(
            default,
            "#[zerializable] methods may not have a default body",
        ));
    }

    let signature = &function.sig;
    if !signature.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &signature.generics,
            "#[zerializable] methods may not have generic parameters",
        ));
    }
    if signature.constness.is_some()
        || signature.asyncness.is_some()
        || signature.abi.is_some()
        || !matches!(signature.safety, Safety::Default)
    {
        return Err(Error::new_spanned(
            signature,
            "#[zerializable] methods may not be `const`, `async`, `unsafe`, or `extern`",
        ));
    }
    if !takes_shared_self(signature) {
        return Err(Error::new(
            signature.paren_token.span.join(),
            "#[zerializable] methods must take `&self` and no other arguments",
        ));
    }

    let ReturnType::Type(_, return_type) = &signature.output else {
        return Err(Error::new_spanned(
            signature,
            "#[zerializable] methods must return a value",
        ));
    };
    let kind = parse_return_type(return_type)?;
    if matches!(kind, Kind::Nested(_) | Kind::Repeated(_)) && !requires_sized(signature) {
        return Err(Error::new_spanned(
            return_type,
            "methods returning `impl Trait` must be declared `where Self: Sized`, \
             so that `dyn Trait` stays dyn compatible",
        ));
    }

    let Some((slot, attribute)) = declared else {
        return Err(Error::new_spanned(
            signature,
            "every #[zerializable] method requires a #[slot(N)] attribute",
        ));
    };
    if let Some(previous) = parsed.iter().find(|method| method.slot == slot) {
        return Err(Error::new_spanned(
            attribute,
            format!("slot {slot} is already used by `{}`", previous.name),
        ));
    }

    Ok(Method {
        name: &signature.ident,
        slot,
        signature,
        kind,
    })
}

/// Returns the slot number a `#[slot(N)]` attribute declares.
fn parse_slot(attribute: &Attribute) -> Result<u32, Error> {
    let invalid = || Error::new_spanned(attribute, "expected `#[slot(N)]`, where N is a u32");
    let slot: LitInt = attribute.parse_args().map_err(|_| invalid())?;
    slot.base10_parse().map_err(|_| invalid())
}

fn takes_shared_self(signature: &Signature) -> bool {
    signature.variadic.is_none()
        && signature.inputs.len() == 1
        && matches!(signature.inputs.first(), Some(FnArg::Receiver(receiver))
            if matches!(receiver.kind, ReceiverKind::Reference(_, _, None)))
}

/// Whether a method is declared `where Self: Sized`, which is what keeps a
/// trait dyn compatible despite returning `impl Trait`.
fn requires_sized(signature: &Signature) -> bool {
    let Some(where_clause) = &signature.generics.where_clause else {
        return false;
    };
    where_clause.predicates.iter().any(|predicate| {
        matches!(predicate, WherePredicate::Type(predicate)
            if is_self(&predicate.bounded_ty)
                && predicate.bounds.iter().any(|bound| matches!(bound, TypeParamBound::Trait(bound)
                    if bound.maybe.is_none() && bound.path.is_ident("Sized"))))
    })
}

fn is_self(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if named(path).is_some_and(|name| name == "Self"))
}

/// The single identifier a type path names, if it is neither qualified nor
/// generic.
fn named(path: &TypePath) -> Option<&Ident> {
    if path.qself.is_some() {
        None
    } else {
        path.path.get_ident()
    }
}

fn parse_return_type(ty: &Type) -> Result<Kind<'_>, Error> {
    let unsupported = || {
        Error::new_spanned(
            ty,
            "unsupported return type: expected a scalar, `&str`, `&[u8]`, \
             `impl Trait + '_`, or `impl List<Item = impl Trait + '_> + '_`",
        )
    };

    match ty {
        Type::Reference(reference) if reference.mutability.is_none() => match &*reference.elem {
            Type::Path(path) if named(path).is_some_and(|name| name == "str") => Ok(Kind::Str),
            Type::Slice(slice) => match &*slice.elem {
                Type::Path(path) if named(path).is_some_and(|name| name == "u8") => Ok(Kind::Bytes),
                _ => Err(unsupported()),
            },
            _ => Err(unsupported()),
        },
        Type::Path(path) => named(path)
            .filter(|name| SCALARS.iter().any(|scalar| *name == scalar))
            .map(Kind::Scalar)
            .ok_or_else(unsupported),
        Type::ImplTrait(impl_trait) => {
            let bound = trait_bound(impl_trait)?;
            let segment = bound
                .path
                .segments
                .last()
                .expect("a path has at least one segment");
            if segment.ident == "List" {
                Ok(Kind::Repeated(parse_list_item(segment)?))
            } else {
                Ok(Kind::Nested(&bound.path))
            }
        }
        _ => Err(unsupported()),
    }
}

/// Extracts the trait an `impl Trait + '_` names. Its lifetime bound is left
/// behind: the generated code does not need it, because a view borrows from the
/// buffer, not the source.
fn trait_bound(impl_trait: &TypeImplTrait) -> Result<&TraitBound, Error> {
    match impl_trait.bounds.first() {
        Some(TypeParamBound::Trait(bound)) => Ok(bound),
        _ => Err(Error::new_spanned(
            impl_trait,
            "expected a trait path after `impl`",
        )),
    }
}

/// Extracts the element schema out of the `<Item = impl Path + '_>` that
/// follows `impl List`.
fn parse_list_item(list: &PathSegment) -> Result<&Path, Error> {
    let expected = || {
        Error::new_spanned(
            list,
            "expected `impl List<Item = impl Trait + '_> + '_`, naming the schema \
             the list holds",
        )
    };
    let PathArguments::AngleBracketed(arguments) = &list.arguments else {
        return Err(expected());
    };
    let item = arguments
        .args
        .iter()
        .find_map(|argument| match argument {
            GenericArgument::AssocType(item) if item.ident == "Item" => Some(&item.ty),
            _ => None,
        })
        .ok_or_else(expected)?;
    let Type::ImplTrait(item) = item else {
        return Err(expected());
    };
    Ok(&trait_bound(item)?.path)
}

// ============================================================
// Code generation
// ============================================================

fn generate(schema: &Schema<'_>) -> TokenStream {
    const VALIDATED: &str = "the message was validated when it was decoded";
    let Schema {
        visibility,
        name,
        methods,
    } = schema;
    let view = format_ident!("{}View", name);
    let source = format_ident!("__{}Source", name);
    // The offset table is indexed by slot, so it is as long as the highest one.
    let slots = Literal::usize_suffixed(
        methods
            .iter()
            .map(|method| method.slot as usize + 1)
            .max()
            .unwrap_or(0),
    );

    let encoded = methods.iter().map(|method| {
        let entry = Literal::usize_suffixed(method.slot as usize);
        let value = {
            let method = method.name;
            quote!(<Self as #name>::#method(self))
        };
        let write = match &method.kind {
            Kind::Str => quote!(__writer.write_str(#value);),
            Kind::Bytes => quote!(__writer.write_bytes(#value);),
            Kind::Scalar(scalar) => {
                let write = format_ident!("write_{}", scalar);
                quote!(__writer.#write(#value);)
            }
            Kind::Nested(path) => {
                let schema = schema_of(path);
                quote! {
                    let __value = #value;
                    <#schema as ::zerialize::Zerializable>::encode_source(&__value, __writer);
                }
            }
            Kind::Repeated(path) => {
                let schema = schema_of(path);
                quote! {
                    let __items = #value;
                    let __length = ::zerialize::List::len(&__items);
                    let __list = __writer.begin_frame(__length);
                    for __index in 0..__length {
                        let __item = ::zerialize::List::get(&__items, __index)
                            .expect("List::get returned None below List::len");
                        __writer.begin_entry(&__list, __index);
                        <#schema as ::zerialize::Zerializable>::encode_source(&__item, __writer);
                    }
                    __writer.end_frame(__list);
                }
            }
        };
        quote! {
            __writer.begin_entry(&__frame, #entry);
            #write
        }
    });

    // So that an implementation can return `&self.nested` as `impl Trait + '_`.
    let forwarded = methods.iter().map(|method| {
        let signature = method.signature;
        let method = method.name;
        quote! {
            #signature {
                <__S as #name>::#method(*self)
            }
        }
    });

    // Inherent accessors shadow the trait's, and return concrete view types
    // rather than the trait's opaque `impl Trait`, which lets callers keep
    // using nested views as views.
    let accessors = methods.iter().map(|method| {
        let slot = Literal::u32_suffixed(method.slot);
        let (return_type, body) = match &method.kind {
            Kind::Str => (
                quote!(&'buf str),
                quote!(__message.read_str(#slot).expect(#VALIDATED)),
            ),
            Kind::Bytes => (
                quote!(&'buf [u8]),
                quote!(__message.read_bytes(#slot).expect(#VALIDATED)),
            ),
            Kind::Scalar(scalar) => {
                let read = format_ident!("read_{}", scalar);
                (
                    quote!(#scalar),
                    quote!(__message.#read(#slot).expect(#VALIDATED)),
                )
            }
            Kind::Nested(path) => {
                let schema = schema_of(path);
                (
                    view_of(path),
                    quote! {
                        <#schema as ::zerialize::Zerializable>::decode_view(
                            __message.read_message(#slot).expect(#VALIDATED),
                        )
                        .expect(#VALIDATED)
                    },
                )
            }
            Kind::Repeated(path) => {
                let schema = schema_of(path);
                (
                    quote!(::zerialize::ListView<'buf, #schema>),
                    quote!(__message.read_list(#slot).expect(#VALIDATED)),
                )
            }
        };
        let method = method.name;
        quote! {
            #visibility fn #method(&self) -> #return_type {
                let __message = ::zerialize::Message::trusted(self.bytes);
                #body
            }
        }
    });

    let implemented = methods.iter().map(|method| {
        let signature = method.signature;
        let method = method.name;
        quote! {
            #signature {
                #view::#method(self)
            }
        }
    });

    // A view compares equal to any implementation holding the same data, which
    // is what makes round trips assertable.
    let comparisons = methods.iter().map(|method| {
        let field = method.name;
        let mine = quote!(#view::#field(self));
        let theirs = quote!(<__S as #name>::#field(__other));
        match &method.kind {
            Kind::Repeated(_) => quote! {
                let __mine = #mine;
                let __theirs = #theirs;
                if ::zerialize::List::len(&__mine) != ::zerialize::List::len(&__theirs) {
                    return false;
                }
                for (__left, __right) in
                    ::zerialize::List::iter(&__mine).zip(::zerialize::List::iter(&__theirs))
                {
                    if __left != __right {
                        return false;
                    }
                }
            },
            _ => quote! {
                if #mine != #theirs {
                    return false;
                }
            },
        }
    });

    // Debug reads the fields rather than the bytes, so a view prints as the
    // message it stands for.
    let fields = methods.iter().map(|method| {
        let method = method.name;
        let label = method.to_string();
        quote!(.field(#label, &#view::#method(self)))
    });

    // Reading every field is what validation is: it leaves the accessors above
    // nothing that can fail.
    let checks = methods.iter().map(|method| {
        let slot = Literal::u32_suffixed(method.slot);
        match &method.kind {
            Kind::Str => quote!(__message.read_str(#slot)?;),
            Kind::Bytes => quote!(__message.read_bytes(#slot)?;),
            Kind::Scalar(scalar) => {
                let read = format_ident!("read_{}", scalar);
                quote!(__message.#read(#slot)?;)
            }
            Kind::Nested(path) => {
                let schema = schema_of(path);
                quote! {
                    <#schema as ::zerialize::Zerializable>::decode_view(
                        __message.read_message(#slot)?,
                    )?;
                }
            }
            Kind::Repeated(path) => {
                let schema = schema_of(path);
                quote!(__message.read_list::<#schema>(#slot)?;)
            }
        }
    });
    let validate = if methods.is_empty() {
        TokenStream::new()
    } else {
        quote! {
            if __message.validates() {
                #(#checks)*
            }
        }
    };

    let documentation = format!(
        "Zero-copy view of a `{name}` message.\n\n\
         Returned by `decode::<dyn {name}>`, borrowing from the buffer that\n\
         was decoded rather than owning its contents."
    );
    let view_name = view.to_string();

    quote! {
        // The object safe adapter that gives `encode::<dyn Trait>(&value)` a
        // single dynamic call to dispatch on, rather than one per field.
        #[doc(hidden)]
        #visibility trait #source {
            fn __zerialize_encode(&self, __writer: &mut ::zerialize::Writer);
        }

        impl<__S: #name> #source for __S {
            fn __zerialize_encode(&self, __writer: &mut ::zerialize::Writer) {
                let __frame = __writer.begin_frame(#slots);
                #(#encoded)*
                __writer.end_frame(__frame);
            }
        }

        impl<__S: #name> #name for &__S {
            #(#forwarded)*
        }

        // The view is the bytes of the message and nothing else. Fields are
        // read out of them on access, by indexing the frame's offset table with
        // the slot number, so a view costs the same whatever its schema holds.
        #[doc = #documentation]
        #[derive(::core::clone::Clone, ::core::marker::Copy)]
        #[allow(dead_code)]
        #visibility struct #view<'buf> {
            bytes: &'buf [u8],
        }

        #[allow(dead_code)]
        impl<'buf> #view<'buf> {
            #(#accessors)*
        }

        impl<'buf> #name for #view<'buf> {
            #(#implemented)*
        }

        impl<__S: #name> ::core::cmp::PartialEq<__S> for #view<'_> {
            fn eq(&self, __other: &__S) -> bool {
                #(#comparisons)*
                true
            }
        }

        impl ::core::fmt::Debug for #view<'_> {
            fn fmt(
                &self,
                __formatter: &mut ::core::fmt::Formatter<'_>,
            ) -> ::core::fmt::Result {
                __formatter.debug_struct(#view_name)
                    #(#fields)*
                    .finish()
            }
        }

        impl ::zerialize::Zerializable for dyn #name {
            type Source<'src> = dyn #source + 'src;
            type View<'buf> = #view<'buf>;

            fn encode_source<'src>(
                __source: &'src Self::Source<'src>,
                __writer: &mut ::zerialize::Writer,
            ) {
                #source::__zerialize_encode(__source, __writer)
            }

            fn decode_view<'buf>(
                __message: ::zerialize::Message<'buf>,
            ) -> ::core::result::Result<Self::View<'buf>, ::zerialize::Error> {
                #validate
                ::core::result::Result::Ok(#view { bytes: __message.bytes() })
            }
        }
    }
}
