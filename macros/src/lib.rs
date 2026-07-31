//! Procedural macro implementation for the `zerialize` crate.
//!
//! See the `zerialize` crate for documentation; this crate is an implementation
//! detail and its macros are re-exported from there.

#![forbid(unsafe_code)]

use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::{
    Attribute, Data, DataEnum, DataStruct, DeriveInput, Error, Fields, FnArg, GenericArgument,
    GenericParam, Generics, Ident, Item, ItemEnum, ItemTrait, LitInt, Meta, Path, PathArguments,
    PathSegment, ReceiverKind, ReturnType, Safety, Signature, Token, TraitBound, TraitItem,
    TraitItemFn, Type, TypeImplTrait, TypeParamBound, TypePath, Visibility, WherePredicate,
    parse_quote, punctuated::Punctuated,
};

/// Turns a trait or an enum into a zero-copy serialization schema.
///
/// # Traits
///
/// A trait is a message. Every method must declare the slot it occupies with
/// `#[n(N)]`. Slots are the identity of a field on the wire, so renaming or
/// reordering methods is safe, and a reader skips slots it does not know about.
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
/// A view may also be asked to print and compare itself, with
/// `#[zerializable(derive(Debug, PartialEq))]`. Neither is generated otherwise,
/// and both are implementations only the schema can write: a view holds bytes
/// rather than fields, and compares against *any* implementation of its schema,
/// which is what makes `assert_eq!(decode::<dyn Trait>(&bytes)?, source)` read
/// the way it does. Whatever a view prints or compares must support it too, so
/// asking a schema for either asks the same of the schemas and values it
/// carries.
///
/// Methods may return a scalar, `&str`, `&[u8]`, a value type named outright,
/// a schema declared as a trait as `impl Trait + '_`, a schema declared as an
/// enum as `Enum<impl Trait + '_>`, or a sequence of either as
/// `impl List<Item = ..> + '_`. Everything named as an `impl Trait` must be
/// declared `where Self: Sized` to keep `dyn Trait` usable as the schema's
/// name. A value type, being `Copy`, is instead returned as itself: see
/// [`macro@Zerializable`].
///
/// # Enums
///
/// An enum is a choice between messages. Every variant must declare the tag
/// that names it with `#[variant(N)]`, and every field of a variant the slot it
/// occupies with `#[n(N)]`. A field is either a scalar or one of the enum's
/// parameters, each of which stands for a nested schema and so must be bound by
/// the trait declaring it. A variant carries nothing, a tuple of fields, or
/// named fields, and is built and matched the way it was declared. Naming a
/// field changes how the enum reads rather than what it encodes as, since a
/// slot is what names a field on the wire.
///
/// The macro generates, from the enum:
///
/// * the enum itself, with every parameter rewritten so that it may be either
///   an implementation of the schema it carries or the name of that schema,
/// * `impl Zerializable for Enum<dyn Trait, ..>`, which is how the enum over
///   the names of the schemas it carries comes to name a schema itself,
/// * `as_ref`, which borrows what every variant carries, so that a message
///   hands out an enum it stores the way it hands out `&self.nested`.
///
/// One declaration is therefore both a value and the name of a schema:
/// `Worker<OwnedPerson>` holds a person, `Worker<dyn Person>` names the schema
/// it encodes as, and decoding returns the enum over views,
/// `Worker<PersonView<'_>>`, which is matched like any other enum. Because a
/// variant's payload is written in terms of its parameter, a value has to be
/// given a type where it is built:
/// `let worker: Worker<OwnedPerson> = Worker::Engineer(person);`.
///
/// The enum is otherwise an ordinary enum, so what it needs is derived as
/// usual, on either side of the attribute: `#[derive(Debug, Clone, PartialEq)]`
/// reads the same above it as below it, because a `derive` below is moved above
/// before it is expanded. Either way the implementation is bounded by the
/// parameters, as a `derive` writes it, so `Worker<OwnedPerson>` is `Clone`
/// where `OwnedPerson` is.
///
/// `#[zerializable(derive(PartialEq))]` asks for the one implementation a
/// `derive` cannot write: a comparison between *any two* instantiations, so
/// that the enum over views compares against the enum over the implementation
/// it was encoded from. It is what a view carrying an enum needs of it, and it
/// covers `Worker<OwnedPerson> == Worker<OwnedPerson>` as a special case, so
/// `#[derive(PartialEq)]` is rejected alongside it. Ask for it where a schema
/// carrying the enum is compared; derive `PartialEq` where the enum is only
/// compared against itself.
///
/// A message carries an enum by naming it the way its declaration reads,
/// `fn role(&self) -> Worker<impl Person + '_> where Self: Sized`, so a schema
/// is free to be a tree of messages and choices.
///
/// A variant's fields are its own, so two variants may use the same slots, and
/// a field added to a variant is skipped by a reader built against the older
/// schema. A variant added to the schema is not: a reader has nothing to decode
/// an unknown tag as, and reports `Error::UnknownVariant`.
#[proc_macro_attribute]
pub fn zerializable(
    args: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let item = match syn::parse::<Item>(item) {
        Ok(item) => item,
        Err(error) => return error.to_compile_error().into(),
    };
    match item {
        Item::Trait(item) => expand_trait(args.into(), item),
        Item::Enum(item) => expand_enum(args.into(), item),
        other => Error::new_spanned(
            other,
            "#[zerializable] may only be applied to a trait or an enum",
        )
        .to_compile_error(),
    }
    .into()
}

/// A trait is emitted as it was declared, with the generated code beside it.
fn expand_trait(args: TokenStream, mut item: ItemTrait) -> TokenStream {
    let expansion = parse_arguments(args, true)
        .and_then(|derived| Ok(generate_schema(&parse_schema(&item)?, derived)));
    // `#[n(N)]` is consumed here, so it must be stripped from the trait even
    // when the rest of the expansion fails, or the reported error would be a
    // confusing "cannot find attribute `n`".
    for trait_item in &mut item.items {
        match trait_item {
            TraitItem::Const(item) => strip(&mut item.attrs),
            TraitItem::Fn(item) => strip(&mut item.attrs),
            TraitItem::Type(item) => strip(&mut item.attrs),
            TraitItem::Macro(item) => strip(&mut item.attrs),
            _ => continue,
        }
    }
    let mut output = item.into_token_stream();
    output.extend(expansion.unwrap_or_else(|error| error.to_compile_error()));
    output
}

/// An enum is rewritten rather than emitted as it was declared, so it is
/// generated with the rest of the expansion. Only a failed expansion emits it
/// as it was written, which keeps the reported error to the one that matters.
fn expand_enum(args: TokenStream, mut item: ItemEnum) -> TokenStream {
    let expansion = parse_arguments(args.clone(), false).and_then(|derived| {
        match hoist_derives(&item, &args, derived)? {
            Some(hoisted) => Ok(hoisted),
            None => Ok(generate_choice(&item, &parse_choice(&item)?, derived)),
        }
    });
    match expansion {
        Ok(generated) => generated,
        Err(error) => {
            strip_variant_attributes(&mut item);
            let mut output = item.into_token_stream();
            output.extend(error.to_compile_error());
            output
        }
    }
}

/// Moves a `#[derive]` written below `#[zerializable]` above it, by re-emitting
/// the enum as it was declared with the order swapped.
///
/// Attributes expand in the order they are written, so a `derive` below the
/// attribute is handed the rewritten enum, whose payloads are projections
/// through `SchemaArg` that the bounds a `derive` writes cannot reach. Above
/// it, the same `derive` sees the declaration, whose payloads are
/// the parameters themselves, and the implementation it writes holds of the
/// rewritten enum too, because a parameter that is `Sized` resolves its own
/// projection. Swapping the two is therefore all it takes for either order to
/// mean the same thing. The expansion this returns carries the attribute again,
/// and terminates because the enum it names has no `derive` left on it.
fn hoist_derives(
    item: &ItemEnum,
    args: &TokenStream,
    derived: Derived,
) -> Result<Option<TokenStream>, Error> {
    let (derives, rest): (Vec<_>, Vec<_>) = item
        .attrs
        .iter()
        .partition(|attribute| attribute.path().is_ident("derive"));
    if derives.is_empty() {
        return Ok(None);
    }
    for attribute in &derives {
        reject_derived(attribute, derived)?;
    }
    let mut item = item.clone();
    item.attrs = rest.into_iter().cloned().collect();
    let attribute = if args.is_empty() {
        quote!(#[::zerialize::zerializable])
    } else {
        quote!(#[::zerialize::zerializable(#args)])
    };
    Ok(Some(quote! {
        #(#derives)*
        #attribute
        #item
    }))
}

/// Rejects deriving what the attribute was asked to implement, which would
/// otherwise be reported as a pair of conflicting implementations.
fn reject_derived(attribute: &Attribute, derived: Derived) -> Result<(), Error> {
    if !derived.partial_eq {
        return Ok(());
    }
    attribute.parse_nested_meta(|derived| match derived.path.get_ident() {
        Some(derived) if derived == "PartialEq" => Err(Error::new_spanned(
            derived,
            "#[zerializable(derive(PartialEq))] implements PartialEq against any \
             instantiation, which covers this one",
        )),
        _ => Ok(()),
    })
}

/// What a schema is asked to implement beyond the schema itself.
///
/// Both are asked for rather than generated, because both are implementations
/// on the schema's own types that nothing else needs: a schema that is never
/// printed or compared has no use for either, and generating them anyway would
/// claim traits an author may want to implement differently.
#[derive(Clone, Copy, Default)]
struct Derived {
    debug: bool,
    partial_eq: bool,
}

/// Parses `#[zerializable(derive(..))]`.
///
/// `Debug` is only offered where the type it would be implemented for is
/// generated: an enum is declared by its author, so its `Debug` is an ordinary
/// `#[derive(Debug)]`.
fn parse_arguments(args: TokenStream, debug_offered: bool) -> Result<Derived, Error> {
    if args.is_empty() {
        return Ok(Derived::default());
    }
    let offered: &[&str] = if debug_offered {
        &["Debug", "PartialEq"]
    } else {
        &["PartialEq"]
    };
    let expected = || {
        Error::new_spanned(
            args.clone(),
            format!(
                "#[zerializable] takes `derive(..)`, over {}",
                offered.join(" and ")
            ),
        )
    };
    let Ok(Meta::List(derive)) = syn::parse2::<Meta>(args.clone()) else {
        return Err(expected());
    };
    if !derive.path.is_ident("derive") {
        return Err(expected());
    }

    let mut derived = Derived::default();
    for path in derive.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)? {
        let asked = match path.get_ident() {
            Some(asked) if offered.contains(&asked.to_string().as_str()) => asked.clone(),
            // Naming what an enum cannot be asked for is worth more than
            // reporting it as an unknown name, since `Debug` is spelled the
            // ordinary way rather than not being available.
            Some(asked) if asked == "Debug" => {
                return Err(Error::new_spanned(
                    &path,
                    "#[zerializable] enums are printed by #[derive(Debug)], as any enum is",
                ));
            }
            _ => {
                return Err(Error::new_spanned(
                    &path,
                    format!("#[zerializable] implements only {}", offered.join(" and ")),
                ));
            }
        };
        let already = if asked == "Debug" {
            std::mem::replace(&mut derived.debug, true)
        } else {
            std::mem::replace(&mut derived.partial_eq, true)
        };
        if already {
            return Err(Error::new_spanned(
                &path,
                format!("{asked} is derived twice"),
            ));
        }
    }
    Ok(derived)
}

fn strip_variant_attributes(item: &mut ItemEnum) {
    for variant in &mut item.variants {
        strip(&mut variant.attrs);
        for field in &mut variant.fields {
            strip(&mut field.attrs);
        }
    }
}

/// Turns a `Copy` struct or enum into a value: a type a schema holds by value,
/// and which decoding hands back as itself rather than as a view.
///
/// A value struct declares the slot each of its fields occupies with
/// `#[n(N)]`, exactly as a schema's methods do, and a value enum declares
/// the number each of its variants is written as with `#[variant(N)]`. Both are
/// the identity of what they name on the wire, so renaming and reordering are
/// safe, and a struct may gain fields without breaking readers built against
/// the older one. An enum may not: a reader rejects a variant it does not know.
///
/// Fields may be scalars or other values, which is what keeps a value `Copy`:
/// nothing it holds can borrow from the buffer it was read from. Nothing more
/// is asked of the type, but a schema asked to print or compare its fields asks
/// it of this one too, so a value a printed view carries must be `Debug`.
#[proc_macro_derive(Zerializable, attributes(n, variant))]
pub fn derive_zerializable(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let item = match syn::parse::<DeriveInput>(item) {
        Ok(item) => item,
        Err(error) => return error.to_compile_error().into(),
    };
    match parse_value(&item) {
        Ok(value) => generate_value(&value),
        Err(error) => error.to_compile_error(),
    }
    .into()
}

/// Removes the attributes the macro consumes, which are not attributes the
/// compiler knows.
fn strip(attributes: &mut Vec<Attribute>) {
    attributes.retain(|attribute| {
        !(attribute.path().is_ident("n") || attribute.path().is_ident("variant"))
    });
}

// ============================================================
// Schema
// ============================================================

/// A schema declared as a trait: one message, with a field per method.
struct Schema<'a> {
    visibility: &'a Visibility,
    name: &'a Ident,
    methods: Vec<Method<'a>>,
}

/// A schema declared as an enum: one of a set of variants, each carrying the
/// fields declared for it.
struct Choice<'a> {
    visibility: &'a Visibility,
    name: &'a Ident,
    /// The parameters the enum carries nested schemas as, in declaration order.
    params: Vec<Param<'a>>,
    variants: Vec<Case<'a>>,
}

/// One parameter of an enum, standing for a schema its variants carry.
#[derive(Clone, Copy)]
struct Param<'a> {
    name: &'a Ident,
    /// The trait the parameter is bound by, which names the schema its values
    /// are encoded as.
    schema: &'a Path,
}

struct Case<'a> {
    name: &'a Ident,
    tag: u32,
    fields: Vec<CaseField<'a>>,
    style: Style,
}

/// How a variant is written, which is how it has to be matched and built: `V`,
/// `V(..)`, and `V { .. }` are three different declarations to Rust, even where
/// they carry the same fields.
#[derive(Clone, Copy)]
enum Style {
    Unit,
    Tuple,
    Named,
}

struct CaseField<'a> {
    slot: u32,
    /// The field's own name, where its variant gives it one. What names a field
    /// on the wire is its slot, so this is only how the field is written.
    name: Option<&'a Ident>,
    payload: Payload<'a>,
}

enum Payload<'a> {
    /// A fixed width primitive, named by its Rust type.
    Scalar(&'a Ident),
    /// A nested message, carried by one of the enum's parameters.
    Nested(Param<'a>),
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
    /// A `Copy` type held by value, named by its path.
    Value(&'a Path),
    /// A nested schema.
    Nested(Nested<'a>),
    /// A sequence of nested schemas.
    Repeated(Nested<'a>),
}

/// A schema one method returns, named the way the method returns it.
#[derive(Clone, Copy)]
enum Nested<'a> {
    /// A message, returned as `impl Trait + '_` and named by that trait.
    Message(&'a Path),
    /// An enum, returned as the enum instantiated over the schemas it carries,
    /// `Enum<impl Trait + '_>`.
    Choice(&'a TypePath),
}

/// Names a nested schema as `Zerializable` implements it.
fn schema_of(nested: Nested<'_>) -> TokenStream {
    match nested {
        // The `'static` object lifetime that `impl Zerializable for dyn Trait`
        // is written against is spelled out, because the default object
        // lifetime in a return type is the one elided from `&self`.
        Nested::Message(path) => quote!((dyn #path + 'static)),
        // An enum names a schema by carrying their names, so each of its
        // arguments is the schema that argument returns.
        Nested::Choice(path) => carried_schemas(path).into_token_stream(),
    }
}

/// The view type of another schema, named through its `Zerializable` impl so
/// that nested schemas do not have to live in the same module.
fn view_of(nested: Nested<'_>) -> TokenStream {
    let schema = schema_of(nested);
    quote!(<#schema as ::zerialize::Zerializable>::View<'buf>)
}

/// The enum with each schema it carries, written as `impl Trait + '_` where the
/// method returns it, named as `Zerializable` implements it.
fn carried_schemas(path: &TypePath) -> TypePath {
    let mut path = path.clone();
    let Some(segment) = path.path.segments.last_mut() else {
        return path;
    };
    if let PathArguments::AngleBracketed(arguments) = &mut segment.arguments {
        for argument in &mut arguments.args {
            if let GenericArgument::Type(ty) = argument
                && let Type::ImplTrait(carried) = ty
                && let Ok(bound) = trait_bound(carried)
            {
                let schema = &bound.path;
                *ty = parse_quote!((dyn #schema + 'static));
            }
        }
    }
    path
}

/// Whether an enum is returned carrying schemas, which is what makes the return
/// type an `impl Trait` and so needs `where Self: Sized`.
fn carries_schemas(path: &TypePath) -> bool {
    path.path.segments.last().is_some_and(|segment| {
        matches!(&segment.arguments, PathArguments::AngleBracketed(arguments)
        if arguments.args.iter().any(|argument| {
            matches!(argument, GenericArgument::Type(Type::ImplTrait(_)))
        }))
    })
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
    require_no_generics(&item.generics, "#[zerializable] traits")?;
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

/// Rejects the generics neither macro can generate code for: nothing either one
/// produces is parameterized.
fn require_no_generics(generics: &Generics, what: &str) -> Result<(), Error> {
    if !generics.params.is_empty() {
        return Err(Error::new_spanned(
            generics,
            format!("{what} may not have generic parameters"),
        ));
    }
    if let Some(where_clause) = &generics.where_clause {
        return Err(Error::new_spanned(
            where_clause,
            format!("{what} may not have a where clause"),
        ));
    }
    Ok(())
}

/// Parses one method, given the methods of the same trait already parsed, which
/// is what a slot is checked for uniqueness against.
fn parse_method<'a>(function: &'a TraitItemFn, parsed: &[Method<'a>]) -> Result<Method<'a>, Error> {
    let declared = declared_number(&function.attrs, "n")?;

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
            "every #[zerializable] method requires a #[n(N)] attribute",
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

/// The number the last `#[name(N)]` attribute declares, with the attribute it
/// came from, which is what a duplicate is reported against.
fn declared_number<'a>(
    attributes: &'a [Attribute],
    name: &str,
) -> Result<Option<(u32, &'a Attribute)>, Error> {
    let mut declared = None;
    for attribute in attributes {
        if attribute.path().is_ident(name) {
            declared = Some((parse_number(attribute, name)?, attribute));
        }
    }
    Ok(declared)
}

fn parse_number(attribute: &Attribute, name: &str) -> Result<u32, Error> {
    let invalid = || {
        Error::new_spanned(
            attribute,
            format!("expected `#[{name}(N)]`, where N is a u32"),
        )
    };
    let number: LitInt = attribute.parse_args().map_err(|_| invalid())?;
    number.base10_parse().map_err(|_| invalid())
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

/// The scalar a type path names, if it names one.
fn scalar(path: &TypePath) -> Option<&Ident> {
    named(path).filter(|name| SCALARS.iter().any(|scalar| *name == scalar))
}

fn parse_return_type(ty: &Type) -> Result<Kind<'_>, Error> {
    let unsupported = || {
        Error::new_spanned(
            ty,
            "unsupported return type: expected a scalar, a value type, `&str`, \
             `&[u8]`, `impl Trait + '_`, `Enum<impl Trait + '_>`, or a list of \
             either",
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
        // A path carrying schemas is one declared as an enum, returned as
        // itself rather than as `impl Trait` because an enum is a type and not
        // a trait. Anything else named outright is a value: a
        // `#[derive(Zerializable)]` type, which is returned as itself rather
        // than as a view of the buffer.
        Type::Path(path) if path.qself.is_none() => Ok(match scalar(path) {
            Some(scalar) => Kind::Scalar(scalar),
            None if carries_schemas(path) => Kind::Nested(Nested::Choice(path)),
            None => Kind::Value(&path.path),
        }),
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
                Ok(Kind::Nested(Nested::Message(&bound.path)))
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

/// Extracts the element schema out of the `<Item = ..>` that follows
/// `impl List`, named there the way a method returning one element would.
fn parse_list_item(list: &PathSegment) -> Result<Nested<'_>, Error> {
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
    match item {
        Type::ImplTrait(item) => Ok(Nested::Message(&trait_bound(item)?.path)),
        // As in a return type, a path is a schema declared as an enum only
        // where it carries the schemas it holds.
        Type::Path(path) if carries_schemas(path) => Ok(Nested::Choice(path)),
        _ => Err(expected()),
    }
}

fn parse_choice(item: &ItemEnum) -> Result<Choice<'_>, Error> {
    if let Some(where_clause) = &item.generics.where_clause {
        return Err(Error::new_spanned(
            where_clause,
            "#[zerializable] enums may not have a where clause",
        ));
    }

    let mut params = Vec::new();
    for param in &item.generics.params {
        params.push(parse_param(param)?);
    }

    let mut variants = Vec::new();
    for variant in &item.variants {
        variants.push(parse_case(variant, &params, &variants)?);
    }
    if variants.is_empty() {
        return Err(Error::new_spanned(
            item,
            "#[zerializable] enums must have at least one variant",
        ));
    }

    // A parameter no variant carries would leave the generated enum with a
    // parameter it does not use, which is not a legal enum.
    for param in &params {
        let carried = variants
            .iter()
            .flat_map(|variant| &variant.fields)
            .any(|field| matches!(field.payload, Payload::Nested(by) if by.name == param.name));
        if !carried {
            return Err(Error::new_spanned(
                param.name,
                format!("no field carries `{}`", param.name),
            ));
        }
    }

    Ok(Choice {
        visibility: &item.vis,
        name: &item.ident,
        params,
        variants,
    })
}

/// Parses one parameter of an enum, which stands for a schema and so must name
/// the trait declaring it.
fn parse_param(param: &GenericParam) -> Result<Param<'_>, Error> {
    let GenericParam::Type(param) = param else {
        return Err(Error::new_spanned(
            param,
            "#[zerializable] enums may only be generic over the schemas they carry",
        ));
    };
    if let Some((_, default)) = &param.default {
        return Err(Error::new_spanned(
            default,
            "#[zerializable] parameters may not have a default",
        ));
    }
    let mut bounds = param.bounds.iter();
    let schema = match (bounds.next(), bounds.next()) {
        (Some(TypeParamBound::Trait(bound)), None) if bound.maybe.is_none() => &bound.path,
        _ => {
            return Err(Error::new_spanned(
                param,
                "every #[zerializable] parameter must be bound by exactly one trait, \
                 naming the schema it carries",
            ));
        }
    };
    Ok(Param {
        name: &param.ident,
        schema,
    })
}

/// Parses one variant, given the variants of the same enum already parsed,
/// which is what a tag is checked for uniqueness against.
fn parse_case<'a>(
    variant: &'a syn::Variant,
    params: &[Param<'a>],
    parsed: &[Case<'a>],
) -> Result<Case<'a>, Error> {
    if let Some((_, discriminant)) = &variant.discriminant {
        return Err(Error::new_spanned(
            discriminant,
            "#[zerializable] variants may not have a discriminant: \
             what names a variant on the wire is its #[variant(N)]",
        ));
    }

    let style = match &variant.fields {
        Fields::Unit => Style::Unit,
        Fields::Unnamed(_) => Style::Tuple,
        Fields::Named(_) => Style::Named,
    };
    let mut fields = Vec::new();
    for field in &variant.fields {
        fields.push(parse_case_field(field, params, &fields)?);
    }

    let Some((tag, attribute)) = declared_number(&variant.attrs, "variant")? else {
        return Err(Error::new_spanned(
            variant,
            "every #[zerializable] variant requires a #[variant(N)] attribute",
        ));
    };
    if let Some(previous) = parsed.iter().find(|variant| variant.tag == tag) {
        return Err(Error::new_spanned(
            attribute,
            format!("variant {tag} is already used by `{}`", previous.name),
        ));
    }

    Ok(Case {
        name: &variant.ident,
        tag,
        fields,
        style,
    })
}

fn parse_case_field<'a>(
    field: &'a syn::Field,
    params: &[Param<'a>],
    parsed: &[CaseField<'a>],
) -> Result<CaseField<'a>, Error> {
    let payload = parse_payload(&field.ty, params)?;

    let Some((slot, attribute)) = declared_number(&field.attrs, "n")? else {
        return Err(Error::new_spanned(
            field,
            "every field of a #[zerializable] variant requires a #[n(N)] attribute",
        ));
    };
    if let Some(index) = parsed.iter().position(|other| other.slot == slot) {
        let previous = match parsed[index].name {
            Some(name) => format!("`{name}`"),
            None => format!("field {index}"),
        };
        return Err(Error::new_spanned(
            attribute,
            format!("slot {slot} is already used by {previous}"),
        ));
    }

    Ok(CaseField {
        slot,
        name: field.ident.as_ref(),
        payload,
    })
}

fn parse_payload<'a>(ty: &'a Type, params: &[Param<'a>]) -> Result<Payload<'a>, Error> {
    let unsupported = || {
        Error::new_spanned(
            ty,
            "unsupported field type: expected a scalar, or a parameter naming \
             the schema the field carries",
        )
    };

    let Type::Path(path) = ty else {
        return Err(unsupported());
    };
    let name = named(path).ok_or_else(unsupported)?;
    if let Some(param) = params.iter().find(|param| param.name == name) {
        Ok(Payload::Nested(*param))
    } else if SCALARS.iter().any(|scalar| name == scalar) {
        Ok(Payload::Scalar(name))
    } else {
        Err(unsupported())
    }
}

// ============================================================
// Code generation
// ============================================================

fn generate_schema(schema: &Schema<'_>, derived: Derived) -> TokenStream {
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
            Kind::Value(path) => quote! {
                <#path as ::zerialize::Value>::encode_value(&#value, __writer);
            },
            Kind::Nested(nested) => {
                let schema = schema_of(*nested);
                quote! {
                    let __value = #value;
                    <#schema as ::zerialize::Zerializable>::encode_source(&__value, __writer);
                }
            }
            Kind::Repeated(nested) => {
                let schema = schema_of(*nested);
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
            Kind::Value(path) => (
                quote!(#path),
                quote! {
                    <#path as ::zerialize::Value>::decode_value(__message, #slot)
                        .expect(#VALIDATED)
                },
            ),
            Kind::Nested(nested) => {
                let schema = schema_of(*nested);
                (
                    view_of(*nested),
                    quote! {
                        <#schema as ::zerialize::Zerializable>::decode_view(
                            __message.read_message(#slot).expect(#VALIDATED),
                        )
                        .expect(#VALIDATED)
                    },
                )
            }
            Kind::Repeated(nested) => {
                let schema = schema_of(*nested);
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
            Kind::Value(path) => {
                quote!(<#path as ::zerialize::Value>::decode_value(__message, #slot)?;)
            }
            Kind::Nested(nested) => {
                let schema = schema_of(*nested);
                quote! {
                    <#schema as ::zerialize::Zerializable>::decode_view(
                        __message.read_message(#slot)?,
                    )?;
                }
            }
            Kind::Repeated(nested) => {
                let schema = schema_of(*nested);
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

    // A view is compared against any implementation of its schema, which is
    // what makes a round trip assertable, and is an implementation only the
    // schema can write: the view's fields are read out of the buffer.
    let compared = derived.partial_eq.then(|| {
        quote! {
            impl<__S: #name> ::core::cmp::PartialEq<__S> for #view<'_> {
                fn eq(&self, __other: &__S) -> bool {
                    #(#comparisons)*
                    true
                }
            }
        }
    });
    let printed = derived.debug.then(|| {
        quote! {
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
        }
    });

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

        #compared
        #printed

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

        // So that an enum may carry this schema: an enum over the name of a
        // schema carries nothing, since it is a name rather than a value.
        impl ::zerialize::SchemaArg for dyn #name {
            type Value = ::zerialize::SchemaOnly;
        }
    }
}

/// The enum instantiated over `arguments`, which is the enum itself when it
/// carries no schemas.
fn instantiate<T: ToTokens>(name: &Ident, arguments: &[T]) -> TokenStream {
    if arguments.is_empty() {
        quote!(#name)
    } else {
        quote!(#name<#(#arguments),*>)
    }
}

fn generate_choice(item: &ItemEnum, parsed: &Choice<'_>, derived: Derived) -> TokenStream {
    let Choice {
        visibility,
        name,
        params,
        variants,
    } = parsed;
    let source = format_ident!("__{}Source", name);
    let declaration = declaration(item, params);

    // The enum is instantiated three ways: over implementations of the schemas
    // it carries, which is what an encodable value is; over the names of those
    // schemas, which is the name of this one; and over their views, which is
    // what decoding it returns.
    let bounds = params
        .iter()
        .map(|param| {
            let (name, schema) = (param.name, param.schema);
            quote!(#name: #schema)
        })
        .collect::<Vec<_>>();
    let generics = if bounds.is_empty() {
        TokenStream::new()
    } else {
        quote!(<#(#bounds),*>)
    };
    let parameters = params.iter().map(|param| param.name).collect::<Vec<_>>();
    let encodable = instantiate(name, &parameters);

    let schemas = params
        .iter()
        .map(|param| schema_of(Nested::Message(param.schema)))
        .collect::<Vec<_>>();
    let schema = instantiate(name, &schemas);
    let views = params
        .iter()
        .map(|param| view_of(Nested::Message(param.schema)))
        .collect::<Vec<_>>();
    let view = instantiate(name, &views);

    let encoded = variants.iter().map(|variant| {
        let tag = Literal::u32_suffixed(variant.tag);
        let pattern = pattern(name, variant, "__field");
        let payload = if variant.fields.is_empty() {
            TokenStream::new()
        } else {
            // The payload's table is indexed by slot, so it is as long as the
            // highest one the variant declares.
            let slots = Literal::usize_suffixed(
                variant
                    .fields
                    .iter()
                    .map(|field| field.slot as usize + 1)
                    .max()
                    .expect("the variant has fields"),
            );
            let written = variant.fields.iter().enumerate().map(|(index, field)| {
                let entry = Literal::usize_suffixed(field.slot as usize);
                let binding = binding("__field", index);
                let write = match &field.payload {
                    Payload::Scalar(scalar) => {
                        let write = format_ident!("write_{}", scalar);
                        quote!(__writer.#write(*#binding);)
                    }
                    Payload::Nested(carried) => {
                        let schema = schema_of(Nested::Message(carried.schema));
                        quote! {
                            <#schema as ::zerialize::Zerializable>::encode_source(
                                #binding,
                                __writer,
                            );
                        }
                    }
                };
                quote! {
                    __writer.begin_entry(&__payload, #entry);
                    #write
                }
            });
            quote! {
                let __payload = __writer.begin_payload(&__frame, #slots);
                #(#written)*
                __writer.end_frame(__payload);
            }
        };
        quote! {
            #pattern => {
                let __frame = __writer.begin_variant(#tag);
                #payload
                __writer.end_frame(__frame);
            }
        }
    });

    let decoded = variants.iter().map(|variant| {
        let tag = Literal::u32_suffixed(variant.tag);
        let read = variant
            .fields
            .iter()
            .map(|field| {
                let slot = Literal::u32_suffixed(field.slot);
                match &field.payload {
                    Payload::Scalar(scalar) => {
                        let read = format_ident!("read_{}", scalar);
                        quote!(__payload.#read(#slot)?)
                    }
                    Payload::Nested(carried) => {
                        let schema = schema_of(Nested::Message(carried.schema));
                        quote! {
                            <#schema as ::zerialize::Zerializable>::decode_view(
                                __payload.read_message(#slot)?,
                            )?
                        }
                    }
                }
            })
            .collect::<Vec<_>>();
        let built = construct(name, variant, &read);
        // A variant carrying nothing was written without a payload, so there is
        // none to read here either.
        if variant.fields.is_empty() {
            return quote!(#tag => ::core::result::Result::Ok(#built),);
        }
        quote! {
            #tag => {
                let __payload = __message.read_payload()?;
                ::core::result::Result::Ok(#built)
            }
        }
    });

    // A comparison across instantiations is what a view carrying an enum needs
    // of it: the enum a view holds is the enum over views, while the one it is
    // compared against is the enum over some other implementation. A `derive`
    // writes `PartialEq<Self>` and so cannot say that.
    let others = params
        .iter()
        .map(|param| format_ident!("__Other{}", param.name))
        .collect::<Vec<_>>();
    let compared_generics = if params.is_empty() {
        TokenStream::new()
    } else {
        let bounds = params.iter().zip(&others).map(|(param, other)| {
            let (name, schema) = (param.name, param.schema);
            quote!(#name: #schema + ::core::cmp::PartialEq<#other>, #other: #schema)
        });
        quote!(<#(#bounds),*>)
    };
    let compared_to = instantiate(name, &others);

    // The enum over references to what it carries. A message hands out an enum
    // by value, since the enum a view holds is decided when it is read rather
    // than stored anywhere to be pointed at, so this is how an implementation
    // hands out one of its own without copying what it carries.
    let borrowed_to = instantiate(
        name,
        &params
            .iter()
            .map(|param| {
                let name = param.name;
                quote!(&#name)
            })
            .collect::<Vec<_>>(),
    );
    let borrowed = variants.iter().map(|variant| {
        let pattern = pattern(name, variant, "__field");
        let fields = variant
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let binding = binding("__field", index);
                match &field.payload {
                    // A scalar is carried by value, so a reference to one is not
                    // what the borrowed enum holds.
                    Payload::Scalar(_) => quote!(*#binding),
                    Payload::Nested(_) => quote!(#binding),
                }
            })
            .collect::<Vec<_>>();
        let built = construct(name, variant, &fields);
        quote!(#pattern => #built,)
    });
    let borrowing = format!(
        "Borrows what every variant carries, so that a `{name}` an \
         implementation stores can be handed out as the one it encodes as."
    );
    let compared = derived.partial_eq.then(|| {
        let arms = variants.iter().map(|variant| {
            let mine = pattern(name, variant, "__field");
            let theirs = pattern(name, variant, "__other");
            let equal = (0..variant.fields.len()).map(|index| {
                let (mine, theirs) = (binding("__field", index), binding("__other", index));
                quote!(#mine == #theirs)
            });
            quote! {
                (#mine, #theirs) => true #(&& #equal)*,
            }
        });
        // A single variant leaves nothing for a catch all arm to match.
        let unequal = if variants.len() > 1 {
            quote!(_ => false,)
        } else {
            TokenStream::new()
        };
        quote! {
            impl #compared_generics ::core::cmp::PartialEq<#compared_to> for #encodable {
                fn eq(&self, __other: &#compared_to) -> bool {
                    match (self, __other) {
                        #(#arms)*
                        #unequal
                    }
                }
            }
        }
    });

    quote! {
        #declaration

        #[allow(dead_code)]
        impl #generics #encodable {
            #[doc = #borrowing]
            #visibility fn as_ref(&self) -> #borrowed_to {
                match self {
                    #(#borrowed)*
                }
            }
        }

        #compared

        // The object safe adapter that gives `encode::<Enum<dyn Trait>>(&value)`
        // one dynamic call to dispatch on, whatever the enum is instantiated
        // over.
        #[doc(hidden)]
        #visibility trait #source {
            fn __zerialize_encode(&self, __writer: &mut ::zerialize::Writer);
        }

        impl #generics #source for #encodable {
            fn __zerialize_encode(&self, __writer: &mut ::zerialize::Writer) {
                match self {
                    #(#encoded)*
                }
            }
        }

        impl ::zerialize::Zerializable for #schema {
            type Source<'src> = dyn #source + 'src;
            type View<'buf> = #view;

            fn encode_source<'src>(
                __source: &'src Self::Source<'src>,
                __writer: &mut ::zerialize::Writer,
            ) {
                #source::__zerialize_encode(__source, __writer)
            }

            // Unlike a message, whose view is the bytes it was decoded from,
            // the variant a value holds is decided here: the tag names it, and
            // reading the fields it carries is what validates them.
            fn decode_view<'buf>(
                __message: ::zerialize::Message<'buf>,
            ) -> ::core::result::Result<Self::View<'buf>, ::zerialize::Error> {
                match __message.read_tag()? {
                    #(#decoded)*
                    _ => ::core::result::Result::Err(::zerialize::Error::UnknownVariant),
                }
            }
        }
    }
}

/// The enum as it was declared, with every parameter rewritten to stand for
/// what its variants carry: an implementation stands for itself, and the name
/// of a schema for nothing that can be constructed.
fn declaration(item: &ItemEnum, params: &[Param<'_>]) -> TokenStream {
    let mut item = item.clone();
    strip_variant_attributes(&mut item);
    for param in &mut item.generics.params {
        if let GenericParam::Type(param) = param {
            param.bounds.push(parse_quote!(::zerialize::SchemaArg));
            param.bounds.push(parse_quote!(?Sized));
        }
    }
    for variant in &mut item.variants {
        for field in &mut variant.fields {
            let carried = params
                .iter()
                .find(|param| matches!(&field.ty, Type::Path(path) if named(path) == Some(param.name)))
                .map(|param| param.name);
            if let Some(carried) = carried {
                field.ty = parse_quote!(<#carried as ::zerialize::SchemaArg>::Value);
            }
        }
    }
    item.into_token_stream()
}

/// What a field is bound to when its variant is matched. A field is bound by
/// position rather than by whatever it is called, so that every variant is
/// matched the same way whether or not its fields are named.
fn binding(prefix: &str, index: usize) -> Ident {
    format_ident!("{}{}", prefix, index)
}

/// One variant over whatever its fields are given, written the way it was
/// declared. The same shape is both the expression building a variant and the
/// pattern matching it, which is what lets one function write either.
fn construct<T: ToTokens>(name: &Ident, variant: &Case<'_>, values: &[T]) -> TokenStream {
    let variant_name = variant.name;
    match variant.style {
        Style::Unit => quote!(#name::#variant_name),
        Style::Tuple => quote!(#name::#variant_name(#(#values),*)),
        Style::Named => {
            let names = variant
                .fields
                .iter()
                .map(|field| field.name.expect("a named variant's fields are named"));
            quote!(#name::#variant_name { #(#names: #values),* })
        }
    }
}

/// The pattern matching one variant, binding every field it carries.
fn pattern(name: &Ident, variant: &Case<'_>, prefix: &str) -> TokenStream {
    let bindings = (0..variant.fields.len())
        .map(|index| binding(prefix, index))
        .collect::<Vec<_>>();
    construct(name, variant, &bindings)
}

// ============================================================
// Values
// ============================================================

/// A `Copy` type a schema holds by value.
struct Value<'a> {
    name: &'a Ident,
    shape: Shape<'a>,
}

/// How a value is written where the message that holds it has left room for it.
enum Shape<'a> {
    /// As a frame of fields indexed by slot, exactly like a message.
    Struct(Vec<Field<'a>>),
    /// As the number of the variant, which is all a unit variant carries.
    Enum(Vec<Variant<'a>>),
}

struct Field<'a> {
    name: &'a Ident,
    slot: u32,
    /// A value's fields are what a `Copy` type may hold: nothing that borrows.
    kind: FieldKind<'a>,
}

enum FieldKind<'a> {
    Scalar(&'a Ident),
    Value(&'a Path),
}

struct Variant<'a> {
    name: &'a Ident,
    number: u32,
}

fn parse_value(item: &DeriveInput) -> Result<Value<'_>, Error> {
    require_no_generics(&item.generics, "#[derive(Zerializable)] types")?;
    let shape = match &item.data {
        Data::Struct(data) => Shape::Struct(parse_fields(data)?),
        Data::Enum(data) => Shape::Enum(parse_variants(data)?),
        Data::Union(data) => {
            return Err(Error::new_spanned(
                data.union_token,
                "#[derive(Zerializable)] may only be applied to a struct or an enum",
            ));
        }
    };
    Ok(Value {
        name: &item.ident,
        shape,
    })
}

fn parse_fields(data: &DataStruct) -> Result<Vec<Field<'_>>, Error> {
    const NAMED: &str = "#[derive(Zerializable)] structs must have named fields, \
                         each declaring the slot it occupies";
    let fields = match &data.fields {
        Fields::Named(fields) => &fields.named,
        Fields::Unnamed(fields) => return Err(Error::new_spanned(fields, NAMED)),
        Fields::Unit => return Err(Error::new_spanned(data.struct_token, NAMED)),
    };

    let mut parsed: Vec<Field<'_>> = Vec::new();
    for field in fields {
        let Some((slot, attribute)) = declared_number(&field.attrs, "n")? else {
            return Err(Error::new_spanned(
                field,
                "every field of a #[derive(Zerializable)] struct requires a #[n(N)] attribute",
            ));
        };
        if let Some(previous) = parsed.iter().find(|other| other.slot == slot) {
            return Err(Error::new_spanned(
                attribute,
                format!("slot {slot} is already used by `{}`", previous.name),
            ));
        }
        parsed.push(Field {
            name: field
                .ident
                .as_ref()
                .expect("named fields have an identifier"),
            slot,
            kind: parse_field_type(&field.ty)?,
        });
    }
    Ok(parsed)
}

fn parse_field_type(ty: &Type) -> Result<FieldKind<'_>, Error> {
    match ty {
        Type::Path(path) if path.qself.is_none() => Ok(match scalar(path) {
            Some(scalar) => FieldKind::Scalar(scalar),
            None => FieldKind::Value(&path.path),
        }),
        _ => Err(Error::new_spanned(
            ty,
            "unsupported field type: expected a scalar or another value type. A value is \
             `Copy`, so it may not hold anything borrowed or owned",
        )),
    }
}

fn parse_variants(data: &DataEnum) -> Result<Vec<Variant<'_>>, Error> {
    if data.variants.is_empty() {
        return Err(Error::new_spanned(
            data.enum_token,
            "#[derive(Zerializable)] enums must have at least one variant",
        ));
    }

    let mut parsed: Vec<Variant<'_>> = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(Error::new_spanned(
                &variant.fields,
                "#[derive(Zerializable)] enum variants may not hold data",
            ));
        }
        let Some((number, attribute)) = declared_number(&variant.attrs, "variant")? else {
            return Err(Error::new_spanned(
                variant,
                "every variant of a #[derive(Zerializable)] enum requires a \
                 #[variant(N)] attribute",
            ));
        };
        if let Some(previous) = parsed.iter().find(|other| other.number == number) {
            return Err(Error::new_spanned(
                attribute,
                format!("variant {number} is already used by `{}`", previous.name),
            ));
        }
        parsed.push(Variant {
            name: &variant.ident,
            number,
        });
    }
    Ok(parsed)
}

fn generate_value(value: &Value<'_>) -> TokenStream {
    let Value { name, shape } = value;
    let (encode, decode) = match shape {
        Shape::Struct(fields) => generate_struct(fields),
        Shape::Enum(variants) => generate_enum(variants),
    };
    quote! {
        impl ::zerialize::Value for #name {
            fn encode_value(&self, __writer: &mut ::zerialize::Writer) {
                #encode
            }

            fn decode_value(
                __message: ::zerialize::Message<'_>,
                __slot: ::core::primitive::u32,
            ) -> ::core::result::Result<Self, ::zerialize::Error> {
                #decode
            }
        }
    }
}

/// A value struct is a frame, so it is read and written exactly as a message
/// is, and gains and loses fields with the same consequences.
fn generate_struct(fields: &[Field<'_>]) -> (TokenStream, TokenStream) {
    let slots = Literal::usize_suffixed(
        fields
            .iter()
            .map(|field| field.slot as usize + 1)
            .max()
            .unwrap_or(0),
    );

    let written = fields.iter().map(|field| {
        let entry = Literal::usize_suffixed(field.slot as usize);
        let name = field.name;
        let write = match &field.kind {
            FieldKind::Scalar(scalar) => {
                let write = format_ident!("write_{}", scalar);
                quote!(__writer.#write(self.#name);)
            }
            FieldKind::Value(path) => quote! {
                <#path as ::zerialize::Value>::encode_value(&self.#name, __writer);
            },
        };
        quote! {
            __writer.begin_entry(&__frame, #entry);
            #write
        }
    });

    let read = fields.iter().map(|field| {
        let slot = Literal::u32_suffixed(field.slot);
        let name = field.name;
        let read = match &field.kind {
            FieldKind::Scalar(scalar) => {
                let read = format_ident!("read_{}", scalar);
                quote!(__fields.#read(#slot)?)
            }
            FieldKind::Value(path) => {
                quote!(<#path as ::zerialize::Value>::decode_value(__fields, #slot)?)
            }
        };
        quote!(#name: #read,)
    });

    (
        quote! {
            let __frame = __writer.begin_frame(#slots);
            #(#written)*
            __writer.end_frame(__frame);
        },
        quote! {
            let __fields = __message.read_message(__slot)?;
            ::core::result::Result::Ok(Self { #(#read)* })
        },
    )
}

/// A value enum is its variant number and nothing else, so it costs a `u32`
/// wherever it is held.
fn generate_enum(variants: &[Variant<'_>]) -> (TokenStream, TokenStream) {
    let written = variants.iter().map(|variant| {
        let name = variant.name;
        let number = Literal::u32_suffixed(variant.number);
        quote!(Self::#name => #number,)
    });

    let read = variants.iter().map(|variant| {
        let name = variant.name;
        let number = Literal::u32_suffixed(variant.number);
        quote!(#number => ::core::result::Result::Ok(Self::#name),)
    });

    (
        quote! {
            __writer.write_u32(match self { #(#written)* });
        },
        quote! {
            match __message.read_u32(__slot)? {
                #(#read)*
                _ => ::core::result::Result::Err(::zerialize::Error::UnknownVariant),
            }
        },
    )
}
