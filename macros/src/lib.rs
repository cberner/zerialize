//! Procedural macro implementation for the `zerialize` crate.
//!
//! See the `zerialize` crate for documentation; this crate is an implementation
//! detail and its macros are re-exported from there.

#![forbid(unsafe_code)]

use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::{
    Attribute, Data, DataEnum, DataStruct, DeriveInput, Error, Fields, FnArg, GenericArgument,
    GenericParam, Generics, Ident, Item, ItemEnum, ItemTrait, Lifetime, LitInt, Meta, Path,
    PathArguments, PathSegment, ReceiverKind, ReturnType, Safety, Signature, Token, TraitBound,
    TraitItem, TraitItemFn, Type, TypeImplTrait, TypeParamBound, TypePath, Visibility,
    WherePredicate, parse_quote, punctuated::Punctuated,
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
/// enum as `Enum<impl Trait + '_>`, or a sequence of any of them as
/// `impl List<Item = ..> + '_`. Everything named as an `impl Trait` must be
/// declared `where Self: Sized` to keep `dyn Trait` usable as the schema's
/// name. A value type, being `Copy`, is instead returned as itself: see
/// [`macro@Zerializable`].
///
/// A list holds what a field holds, so `impl List<Item = u32> + '_` and
/// `impl List<Item = &str> + '_` are lists as much as a list of messages is.
/// What a list holds it hands out by value, which is what `Copied` hands out of
/// an implementation storing it.
///
/// Any of those may be wrapped in an `Option`, which makes the field one the
/// message need not carry. `None` is the slot left unwritten, which is exactly
/// what a reader sees of a slot the writer did not have, so a field added as an
/// optional one reads as `None` out of a message written before it existed.
/// `Option<Option<..>>` is rejected: there is only one way for a slot to be
/// absent.
///
/// # Enums
///
/// An enum is a choice between messages. Every variant must declare the tag
/// that names it with `#[variant(N)]`, and every field of a variant the slot it
/// occupies with `#[n(N)]`. A field is a scalar, `&str`, `&[u8]`, a value type,
/// one of the enum's parameters, each of which stands for a nested schema or a
/// list and so must be bound by what it stands for, another enum written over
/// this one's parameters, or an `Option` of any of them. A variant carries
/// nothing, a tuple of fields, or named fields, and is built and matched the
/// way it was declared. Naming a field changes how the enum reads rather than
/// what it encodes as, since a slot is what names a field on the wire.
///
/// A parameter bound by `List` is a list the enum carries, and its bound names
/// what that list holds: `L: List<Item = u32>`, `List<Item = Weight>`,
/// `List<Item = &'a str>`, and `List<Item = &'a [u8]>` name an element
/// outright, and `List<Item: Person>` names a list of messages by the trait
/// declaring them, which is what every instantiation of the enum has in common:
/// a source's list holds its own elements, and the list decoding gives holds
/// the views of them. The enum is named with `ListView<'_, ..>` in that
/// parameter's place, over the schema the list holds:
/// `Roster<'_, ListView<'_, dyn Person>>` is the schema of
/// `enum Roster<'a, L: List<Item: Person>>`, and decoding it gives
/// `Roster<'buf, ListView<'buf, dyn Person>>`.
///
/// A list an enum carries may not hold another enum: an enum is a type rather
/// than a trait, so no bound names every instantiation of one. A message may
/// hold that list instead, since a message holds its elements rather than
/// standing for them.
///
/// An enum whose fields borrow declares the lifetime they point into, and is
/// named with it wherever it is named: `enum Note<'a, P: Person>` with a field
/// written `&'a str` is `Note<'_, dyn Person>` as a schema, and
/// `Note<'buf, PersonView<'buf>>` once decoded. Only an enum that borrows
/// declares one, so an enum that does not is named exactly as before. A list of
/// `&str` or `&[u8]` is written in terms of that lifetime as a field of one is,
/// and a list of anything else keeps the buffer it borrows from to itself.
///
/// The macro generates, from the enum:
///
/// * the enum itself, with every parameter carrying a schema rewritten so that
///   it may be either an implementation of that schema or the name of it, and
///   every parameter carrying a list left standing for the list itself,
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
/// `fn role(&self) -> Worker<impl Person + '_> where Self: Sized`, and an enum
/// carries an enum the same way, written over its own parameters rather than
/// over `impl Trait`, so a schema is free to be a tree of messages and choices.
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
/// A variant may carry fields of its own, declaring their slots as a struct
/// does, and is built and matched the way it was declared. An enum whose
/// variants all carry nothing is written as the number naming the one it holds,
/// and so costs a `u32` wherever it is held; one whose variants carry fields is
/// written the way a choice is, as that number and the frame of the fields it
/// names. Giving a variant its first field therefore changes what the enum
/// encodes as, which readers built against the older one cannot read.
///
/// Fields may be scalars, other values, or an `Option` of either, which is what
/// keeps a value `Copy`: nothing it holds can borrow from the buffer it was read
/// from, which is what separates a value enum from a choice. Nothing more is
/// asked of the type, but a schema asked to print or compare its fields asks it
/// of this one too, so a value a printed view carries must be `Debug`.
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
    /// The lifetime the enum's borrowed fields point into, where it has any.
    /// Only an enum that borrows declares one, and only such an enum is named
    /// with one.
    lifetime: Option<&'a Lifetime>,
    /// The parameters the enum carries nested schemas as, in declaration order.
    params: Vec<Param<'a>>,
    variants: Vec<Case<'a>>,
}

/// One parameter of an enum, standing for a schema its variants carry, or for a
/// list of one.
#[derive(Clone)]
struct Param<'a> {
    name: &'a Ident,
    carried: Carried<'a>,
}

/// What a parameter stands for, which is what its bound names.
#[derive(Clone)]
enum Carried<'a> {
    /// A message, named by the trait declaring it.
    Message(&'a Path),
    /// A list, named by the schema its `Item` holds. A list is a handle over
    /// the buffer as a `&str` is, so the parameter stands for one of three
    /// things: the source's own list, `ListView` where the enum is named, and
    /// `ListView` over the buffer once it is decoded.
    List(Kind<'a>),
}

struct Case<'a> {
    written: Written<'a>,
    tag: u32,
    fields: Vec<CaseField<'a>>,
}

/// How a variant is written, which is how it has to be matched and built: `V`,
/// `V(..)`, and `V { .. }` are three different declarations to Rust, even where
/// they carry the same fields. A choice's variants and a value enum's are
/// written alike, so both are built and matched through this.
struct Written<'a> {
    name: &'a Ident,
    style: Style,
    /// The name of each field, where its variant gives it one.
    fields: Vec<Option<&'a Ident>>,
}

#[derive(Clone, Copy)]
enum Style {
    Unit,
    Tuple,
    Named,
}

impl<'a> Written<'a> {
    fn new(variant: &'a syn::Variant) -> Self {
        Self {
            name: &variant.ident,
            style: match &variant.fields {
                Fields::Unit => Style::Unit,
                Fields::Unnamed(_) => Style::Tuple,
                Fields::Named(_) => Style::Named,
            },
            fields: variant
                .fields
                .iter()
                .map(|field| field.ident.as_ref())
                .collect(),
        }
    }
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
    /// A `&str`, which points into the buffer rather than being copied out of
    /// it, and so is written in terms of the enum's lifetime.
    Str,
    /// A `&[u8]`, borrowed as a `&str` is.
    Bytes,
    /// A `Copy` type held by value, named by its path.
    Value(&'a Path),
    /// A nested message, carried by one of the enum's parameters.
    Nested(Param<'a>),
    /// A nested enum, carried as itself: its payload is written in terms of
    /// this enum's parameters, so one declaration instantiates with the other.
    Choice(&'a TypePath),
    /// A list, carried by one of the enum's parameters.
    Repeated(Param<'a>),
    /// A field that may be absent, written as `Option<..>`.
    Optional(Box<Payload<'a>>),
}

struct Method<'a> {
    name: &'a Ident,
    slot: u32,
    /// The trait's declaration of this method, reused verbatim so that the
    /// generated implementations are guaranteed to match it.
    signature: &'a Signature,
    kind: Kind<'a>,
}

#[derive(Clone)]
enum Kind<'a> {
    Str,
    Bytes,
    /// A fixed width primitive, named by its Rust type.
    Scalar(&'a Ident),
    /// A `Copy` type held by value, named by its path.
    Value(&'a Path),
    /// A nested schema.
    Nested(Nested<'a>),
    /// A sequence of elements, each of whatever kind the list holds.
    Repeated(Box<Kind<'a>>),
    /// A field that may be absent, written as `Option<..>`. An absent field is
    /// a slot left unwritten, which is what a reader that does not know the
    /// slot at all sees too.
    Optional(Box<Kind<'a>>),
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

/// The schema a parameter stands for, over `lifetime` where it needs one: a
/// message by the name of the trait declaring it, and a list by the view of it,
/// since a list is a handle over the buffer as a `&str` is.
fn param_schema(param: &Param<'_>, lifetime: &TokenStream) -> TokenStream {
    match &param.carried {
        Carried::Message(schema) => schema_of(Nested::Message(schema)),
        Carried::List(item) => {
            let element = element_of(item);
            quote!(::zerialize::ListView<#lifetime, #element>)
        }
    }
}

/// What a parameter stands for once the enum is decoded.
fn param_view(param: &Param<'_>) -> TokenStream {
    match &param.carried {
        Carried::Message(schema) => view_of(Nested::Message(schema)),
        Carried::List(item) => {
            let element = element_of(item);
            quote!(::zerialize::ListView<'buf, #element>)
        }
    }
}

/// The bound a parameter is declared with, which every instantiation of the enum
/// satisfies: an implementation of the schema it stands for, the name of that
/// schema, and the view decoding gives.
fn param_bound(param: &Param<'_>, lifetime: Option<&TokenStream>) -> TokenStream {
    match &param.carried {
        Carried::Message(schema) => quote!(#schema),
        Carried::List(item) => {
            let held = held_bound(item, lifetime);
            quote!(::zerialize::List<#held>)
        }
    }
}

/// How the `Item` of a list is bound: by what it is where an element is named
/// outright, and by the trait its elements implement where they are messages,
/// since `Item` is sized and the name of a message is not. A list of messages is
/// therefore bound alike wherever the enum is named, holding the source's own
/// elements where it holds a list and the views of them where it was decoded.
fn held_bound(item: &Kind<'_>, lifetime: Option<&TokenStream>) -> TokenStream {
    match item {
        Kind::Str => quote!(Item = &#lifetime ::core::primitive::str),
        Kind::Bytes => quote!(Item = &#lifetime [::core::primitive::u8]),
        Kind::Scalar(scalar) => quote!(Item = #scalar),
        Kind::Value(path) => quote!(Item = #path),
        Kind::Nested(Nested::Message(schema)) => quote!(Item: #schema),
        Kind::Nested(Nested::Choice(_)) | Kind::Optional(_) | Kind::Repeated(_) => {
            unreachable!("a list an enum carries holds none of these")
        }
    }
}

/// Whether a parameter carries a list, which the enum holds as it stands rather
/// than as a name, and which is why the schema is named over a lifetime.
fn is_list(param: &Param<'_>) -> bool {
    matches!(param.carried, Carried::List(_))
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
            match argument {
                GenericArgument::Type(ty) => {
                    if let Type::ImplTrait(carried) = ty
                        && let Ok(bound) = trait_bound(carried)
                    {
                        *ty = match carried_list(bound) {
                            // A list is named by the view of it, which is what
                            // the parameter standing for one holds wherever the
                            // enum is named.
                            Some(list) => list,
                            None => {
                                let schema = &bound.path;
                                parse_quote!((dyn #schema + 'static))
                            }
                        };
                    }
                }
                // A schema is a name rather than a buffer, and is named for any
                // lifetime, so the one an enum borrows from is spelled out here
                // rather than left as the `'_` the method returning it wrote.
                GenericArgument::Lifetime(lifetime) => *lifetime = parse_quote!('static),
                _ => (),
            }
        }
    }
    path
}

/// The list `impl List<Item = ..>` names, where that is what a bound names.
fn carried_list(bound: &TraitBound) -> Option<Type> {
    let segment = bound.path.segments.last()?;
    if segment.ident != "List" {
        return None;
    }
    // An element is named the way a method returning one names it, or by the
    // trait it implements, which is how the parameter carrying the list names
    // the messages it holds.
    let item = parse_list_item(segment)
        .or_else(|_| parse_carried_item(segment))
        .ok()?;
    let element = element_of(&item);
    Some(parse_quote!(::zerialize::ListView<'static, #element>))
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

/// Whether a path names a schema declared as an enum rather than a value named
/// outright, which is what its arguments say: an enum is named over the schemas
/// it carries and the buffer it borrows from, and a value, being `Copy` and free
/// of both, has neither.
fn names_a_choice(path: &TypePath) -> bool {
    path.path.segments.last().is_some_and(|segment| {
        matches!(&segment.arguments, PathArguments::AngleBracketed(arguments)
            if !arguments.args.is_empty())
    })
}

const SCALARS: [&str; 14] = [
    "bool", "char", "u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "i128", "f32",
    "f64",
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
    if returns_impl_trait(&kind) && !requires_sized(signature) {
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

/// How a field is named where a duplicate slot is reported against it.
fn describe(name: Option<&Ident>, index: usize) -> String {
    match name {
        Some(name) => format!("`{name}`"),
        None => format!("field {index}"),
    }
}

fn takes_shared_self(signature: &Signature) -> bool {
    signature.variadic.is_none()
        && signature.inputs.len() == 1
        && matches!(signature.inputs.first(), Some(FnArg::Receiver(receiver))
            if matches!(receiver.kind, ReceiverKind::Reference(_, _, None)))
}

/// Whether a return type is written as an `impl Trait`, which is what a method
/// has to be declared `where Self: Sized` for.
fn returns_impl_trait(kind: &Kind<'_>) -> bool {
    match kind {
        // An enum is a type rather than a trait, so it is only an `impl Trait`
        // where it carries one.
        Kind::Nested(Nested::Choice(path)) => carries_schemas(path),
        Kind::Nested(Nested::Message(_)) | Kind::Repeated(_) => true,
        Kind::Optional(inner) => returns_impl_trait(inner),
        _ => false,
    }
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

/// The type an `Option<..>` wraps, if the path names one.
fn optional(path: &TypePath) -> Option<&Type> {
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    match arguments.args.first() {
        Some(GenericArgument::Type(ty)) if arguments.args.len() == 1 => Some(ty),
        _ => None,
    }
}

/// Rejects `Option<Option<..>>`, which the wire has nothing to distinguish:
/// what encodes `None` is an unwritten slot, and a slot is either written or
/// it is not.
fn require_not_optional(ty: &Type, optional: bool) -> Result<(), Error> {
    if optional {
        return Err(Error::new_spanned(
            ty,
            "`Option<Option<..>>` has no encoding: an absent field is a slot left \
             unwritten, so there is nothing left for the inner one to be absent in",
        ));
    }
    Ok(())
}

fn parse_return_type(ty: &Type) -> Result<Kind<'_>, Error> {
    let unsupported = || {
        Error::new_spanned(
            ty,
            "unsupported return type: expected a scalar, a value type, `&str`, \
             `&[u8]`, `impl Trait + '_`, `Enum<impl Trait + '_>`, a list of \
             either, or an `Option` of any of them",
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
        // An optional field is whatever it wraps, in a slot that may be left
        // unwritten, so it is parsed as what it wraps.
        Type::Path(path) if optional(path).is_some() => {
            let inner = parse_return_type(optional(path).expect("the path names an Option"))?;
            require_not_optional(ty, matches!(inner, Kind::Optional(_)))?;
            Ok(Kind::Optional(Box::new(inner)))
        }
        // A path carrying schemas is one declared as an enum, returned as
        // itself rather than as `impl Trait` because an enum is a type and not
        // a trait. Anything else named outright is a value: a
        // `#[derive(Zerializable)]` type, which is returned as itself rather
        // than as a view of the buffer.
        Type::Path(path) if path.qself.is_none() => Ok(match scalar(path) {
            Some(scalar) => Kind::Scalar(scalar),
            None if names_a_choice(path) => Kind::Nested(Nested::Choice(path)),
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
                Ok(Kind::Repeated(Box::new(parse_list_item(segment)?)))
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

/// Extracts the element out of the `<Item = ..>` that follows `impl List`,
/// named there the way a method returning one element would name it.
fn parse_list_item(list: &PathSegment) -> Result<Kind<'_>, Error> {
    let expected = || {
        Error::new_spanned(
            list,
            "expected `impl List<Item = ..> + '_`, naming what the list holds",
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
    require_one_element(parse_return_type(item)?, item)
}

/// Parses what the list a parameter carries holds, out of the `List<..>` bound
/// naming it. An element that is a message is named by the trait declaring it,
/// `List<Item: Person>`, rather than by the name of that schema: `Item` is
/// sized, and `dyn Person` is not.
fn parse_carried_item(list: &PathSegment) -> Result<Kind<'_>, Error> {
    let expected = || {
        Error::new_spanned(
            list,
            "expected `List<Item = ..>`, naming what the list holds, or \
             `List<Item: Trait>` where it holds messages",
        )
    };
    let PathArguments::AngleBracketed(arguments) = &list.arguments else {
        return Err(expected());
    };
    for argument in &arguments.args {
        match argument {
            GenericArgument::AssocType(item) if item.ident == "Item" => {
                return require_one_element(parse_item(&item.ty)?, &item.ty);
            }
            // A message is bound by the trait declaring it, which is what every
            // instantiation of the enum has in common: the source's own
            // elements implement it, and so do the views decoding gives.
            GenericArgument::Constraint(item) if item.ident == "Item" => {
                return match item.bounds.first() {
                    Some(TypeParamBound::Trait(bound)) if bound.maybe.is_none() => {
                        Ok(Kind::Nested(Nested::Message(&bound.path)))
                    }
                    _ => Err(Error::new_spanned(
                        item,
                        "expected the trait a list's messages implement, `Item: Trait`",
                    )),
                };
            }
            _ => (),
        }
    }
    Err(expected())
}

/// An element is an entry of the list's frame, and an entry holds one thing: a
/// shorter list is what an absent element is, and a list of lists has no entry
/// to nest the inner one in.
fn require_one_element<'a>(item: Kind<'a>, ty: &Type) -> Result<Kind<'a>, Error> {
    match item {
        Kind::Optional(_) => Err(Error::new_spanned(
            ty,
            "a list may not hold `Option`: an element that is not there is a \
             shorter list",
        )),
        Kind::Repeated(_) => Err(Error::new_spanned(ty, "a list may not hold lists")),
        item => Ok(item),
    }
}

/// Parses one element of a list a parameter carries, named there the way a
/// variant carrying one element would name it.
fn parse_item(ty: &Type) -> Result<Kind<'_>, Error> {
    let unsupported = || {
        Error::new_spanned(
            ty,
            "unsupported list item: expected a scalar, a value type, `&str`, or \
             `&[u8]`, or, where the list holds messages, the trait declaring \
             them as `Item: Trait`",
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
        // A schema is a name rather than a type, so it is not what a list holds:
        // what every instantiation of the enum holds is something implementing
        // it, which is what the trait bound below says.
        Type::TraitObject(_) => Err(Error::new_spanned(
            ty,
            "a list of messages is named by the trait declaring them, \
             `List<Item: Trait>`: the name of a schema is not a type an element \
             can be",
        )),
        Type::Path(path) if optional(path).is_some() => Ok(Kind::Optional(Box::new(parse_item(
            optional(path).expect("the path names an Option"),
        )?))),
        // An enum is a type rather than a trait, so there is nothing to bound
        // `Item` by that every instantiation of it satisfies. A message may hold
        // the list instead, which holds its elements rather than standing for
        // them.
        Type::Path(path) if names_a_choice(path) => Err(Error::new_spanned(
            ty,
            "a list a variant carries may not hold an enum: an enum is a type \
             rather than a trait, so no bound names every instantiation of one. \
             A view schema trait's list may hold one",
        )),
        Type::Path(path) if path.qself.is_none() => Ok(match scalar(path) {
            Some(scalar) => Kind::Scalar(scalar),
            None => Kind::Value(&path.path),
        }),
        _ => Err(unsupported()),
    }
}

fn parse_choice(item: &ItemEnum) -> Result<Choice<'_>, Error> {
    if let Some(where_clause) = &item.generics.where_clause {
        return Err(Error::new_spanned(
            where_clause,
            "#[zerializable] enums may not have a where clause",
        ));
    }

    let mut lifetime = None;
    let mut params = Vec::new();
    for param in &item.generics.params {
        match param {
            GenericParam::Lifetime(declared) if lifetime.is_none() => {
                if !declared.bounds.is_empty() {
                    return Err(Error::new_spanned(
                        &declared.bounds,
                        "a #[zerializable] enum's lifetime may not have bounds: it is the \
                         buffer its borrowed fields point into",
                    ));
                }
                lifetime = Some(&declared.lifetime);
            }
            GenericParam::Lifetime(declared) => {
                return Err(Error::new_spanned(
                    declared,
                    "a #[zerializable] enum may declare one lifetime, which is the buffer \
                     every borrowed field of it points into",
                ));
            }
            _ => params.push(parse_param(param, lifetime.is_some())?),
        }
    }

    let mut variants = Vec::new();
    for variant in &item.variants {
        variants.push(parse_case(variant, &params, lifetime.is_some(), &variants)?);
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
            .any(|field| carries(&field.payload, param));
        if !carried {
            return Err(Error::new_spanned(
                param.name,
                format!("no field carries `{}`", param.name),
            ));
        }
    }

    // A lifetime nothing borrows from would leave the generated enum with a
    // parameter it does not use, exactly as an uncarried parameter would.
    if let Some(lifetime) = lifetime
        && !variants
            .iter()
            .flat_map(|variant| &variant.fields)
            .any(|field| borrows(&field.payload))
    {
        return Err(Error::new_spanned(
            lifetime,
            format!("no field borrows from `{lifetime}`"),
        ));
    }

    Ok(Choice {
        visibility: &item.vis,
        name: &item.ident,
        lifetime,
        params,
        variants,
    })
}

/// Parses one parameter of an enum, which stands for a schema or for a list of
/// one, and so must name the trait declaring it or what the list holds.
fn parse_param(param: &GenericParam, borrows: bool) -> Result<Param<'_>, Error> {
    let GenericParam::Type(param) = param else {
        return Err(Error::new_spanned(
            param,
            "#[zerializable] enums may only be generic over the schemas and lists they \
             carry, beside the one lifetime their borrowed fields point into",
        ));
    };
    if let Some((_, default)) = &param.default {
        return Err(Error::new_spanned(
            default,
            "#[zerializable] parameters may not have a default",
        ));
    }
    let mut bounds = param.bounds.iter();
    let bound = match (bounds.next(), bounds.next()) {
        (Some(TypeParamBound::Trait(bound)), None) if bound.maybe.is_none() => bound,
        _ => {
            return Err(Error::new_spanned(
                param,
                "every #[zerializable] parameter must be bound by exactly one trait, \
                 naming the schema it carries or, as `List<Item = ..>`, what the \
                 list it carries holds",
            ));
        }
    };
    let segment = bound
        .path
        .segments
        .last()
        .expect("a path has at least one segment");
    let carried = if segment.ident == "List" {
        let item = parse_carried_item(segment)?;
        if !borrows && borrowed_item(&item) {
            return Err(Error::new_spanned(
                segment,
                "a list of borrowed elements needs the enum to declare the lifetime they \
                 point into, `enum Name<'a, ..>`, and to name them as `&'a str` or \
                 `&'a [u8]`",
            ));
        }
        Carried::List(item)
    } else {
        Carried::Message(&bound.path)
    };
    Ok(Param {
        name: &param.ident,
        carried,
    })
}

/// Whether the elements of a list point into the buffer, which is what the
/// enum's lifetime stands for.
fn borrowed_item(item: &Kind<'_>) -> bool {
    matches!(item, Kind::Str | Kind::Bytes)
}

/// Parses one variant, given the variants of the same enum already parsed,
/// which is what a tag is checked for uniqueness against.
fn parse_case<'a>(
    variant: &'a syn::Variant,
    params: &[Param<'a>],
    borrows: bool,
    parsed: &[Case<'a>],
) -> Result<Case<'a>, Error> {
    if let Some((_, discriminant)) = &variant.discriminant {
        return Err(Error::new_spanned(
            discriminant,
            "#[zerializable] variants may not have a discriminant: \
             what names a variant on the wire is its #[variant(N)]",
        ));
    }

    let mut fields = Vec::new();
    for field in &variant.fields {
        fields.push(parse_case_field(field, params, borrows, &fields)?);
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
            format!(
                "variant {tag} is already used by `{}`",
                previous.written.name
            ),
        ));
    }

    Ok(Case {
        written: Written::new(variant),
        tag,
        fields,
    })
}

fn parse_case_field<'a>(
    field: &'a syn::Field,
    params: &[Param<'a>],
    borrows: bool,
    parsed: &[CaseField<'a>],
) -> Result<CaseField<'a>, Error> {
    let payload = parse_payload(&field.ty, params, borrows)?;

    let Some((slot, attribute)) = declared_number(&field.attrs, "n")? else {
        return Err(Error::new_spanned(
            field,
            "every field of a #[zerializable] variant requires a #[n(N)] attribute",
        ));
    };
    if let Some(index) = parsed.iter().position(|other| other.slot == slot) {
        let previous = describe(parsed[index].name, index);
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

fn parse_payload<'a>(
    ty: &'a Type,
    params: &[Param<'a>],
    borrows: bool,
) -> Result<Payload<'a>, Error> {
    let unsupported = || {
        Error::new_spanned(
            ty,
            "unsupported field type: expected a scalar, `&str`, `&[u8]`, a value \
             type, a parameter naming the schema or the list the field carries, \
             an enum over those parameters, or an `Option` of any of them",
        )
    };

    // A borrowed field points into the buffer the enum was decoded from, which
    // is what the enum's lifetime stands for, so an enum that has one may hold
    // them and an enum that does not may not.
    if let Type::Reference(reference) = ty
        && reference.mutability.is_none()
    {
        if !borrows {
            return Err(Error::new_spanned(
                ty,
                "a variant carrying a borrowed field needs the enum to declare the \
                 lifetime it points into, `enum Name<'a, ..>`, and to write the \
                 field as `&'a str` or `&'a [u8]`",
            ));
        }
        return match &*reference.elem {
            Type::Path(path) if named(path).is_some_and(|name| name == "str") => Ok(Payload::Str),
            Type::Slice(slice) => match &*slice.elem {
                Type::Path(path) if named(path).is_some_and(|name| name == "u8") => {
                    Ok(Payload::Bytes)
                }
                _ => Err(unsupported()),
            },
            _ => Err(unsupported()),
        };
    }

    let Type::Path(path) = ty else {
        return Err(unsupported());
    };
    if let Some(inner) = optional(path) {
        let inner = parse_payload(inner, params, borrows)?;
        require_not_optional(ty, matches!(inner, Payload::Optional(_)))?;
        return Ok(Payload::Optional(Box::new(inner)));
    }
    if path.qself.is_some() {
        return Err(unsupported());
    }
    // A path carrying schemas is one declared as an enum, carried as itself:
    // written over this enum's parameters, it instantiates wherever this one
    // does, and so is a value here, a name where this enum is named, and a view
    // of the buffer once this one is decoded.
    if names_a_choice(path) {
        return Ok(Payload::Choice(path));
    }
    let name = named(path).ok_or_else(unsupported)?;
    if let Some(param) = params.iter().find(|param| param.name == name) {
        Ok(match param.carried {
            Carried::Message(_) => Payload::Nested(param.clone()),
            Carried::List(_) => Payload::Repeated(param.clone()),
        })
    } else if SCALARS.iter().any(|scalar| name == scalar) {
        Ok(Payload::Scalar(name))
    // Anything else named outright is a value, exactly as it is where a method
    // returns one: a `#[derive(Zerializable)]` type, carried by value.
    } else {
        Ok(Payload::Value(&path.path))
    }
}

// ============================================================
// Code generation
// ============================================================

const VALIDATED: &str = "the message was validated when it was decoded";

/// Writes the field a method returns into the entry it occupies of the frame
/// `frame` marks, given the expression the field is read from.
///
/// Every generator below is written this way, over the field's kind rather than
/// over the method holding it, so that `Option<..>` is the same field in a slot
/// that may be left unwritten.
fn write_field(
    kind: &Kind<'_>,
    frame: &Ident,
    entry: &TokenStream,
    value: &TokenStream,
) -> TokenStream {
    // An absent field is a slot left unwritten, so `None` writes nothing at
    // all: the offset reserved for it is already zero.
    if let Kind::Optional(inner) = kind {
        let written = write_field(inner, frame, entry, &quote!(__some));
        return quote! {
            if let ::core::option::Option::Some(__some) = #value {
                #written
            }
        };
    }

    let write = match kind {
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
        // A list is a frame of its own, so an element is written into the entry
        // it occupies exactly as a field is written into its slot.
        Kind::Repeated(item) => {
            let list = format_ident!("__list");
            let written = write_field(item, &list, &quote!(__index), &quote!(__item));
            quote! {
                let __items = #value;
                let __length = ::zerialize::List::len(&__items);
                let __list = __writer.begin_frame(__length);
                for __index in 0..__length {
                    let __item = ::zerialize::List::get(&__items, __index)
                        .expect("List::get returned None below List::len");
                    #written
                }
                __writer.end_frame(__list);
            }
        }
        Kind::Optional(_) => unreachable!("an optional field is written above"),
    };
    quote! {
        __writer.begin_entry(&#frame, #entry);
        #write
    }
}

/// The entry of a frame a field occupies, which is its slot.
fn entry_of(slot: u32) -> TokenStream {
    let entry = Literal::usize_suffixed(slot as usize);
    quote!(#entry)
}

/// How long the offset table of a frame holding `slots` is. The table is
/// indexed by slot, so it is as long as the highest one, whatever the frame
/// holds and however many of them it leaves empty.
fn table(slots: impl Iterator<Item = u32>) -> Literal {
    Literal::usize_suffixed(slots.map(|slot| slot as usize + 1).max().unwrap_or(0))
}

/// Names what a list holds, as [`::zerialize::Element`] is implemented for it:
/// a schema by the name of the schema, a value by its own name, and a primitive
/// by what it is, which for the two that borrow is what they point at.
fn element_of(item: &Kind<'_>) -> TokenStream {
    match item {
        Kind::Str => quote!(::core::primitive::str),
        Kind::Bytes => quote!([::core::primitive::u8]),
        Kind::Scalar(scalar) => quote!(#scalar),
        Kind::Value(path) => quote!(#path),
        Kind::Nested(nested) => schema_of(*nested),
        Kind::Optional(_) | Kind::Repeated(_) => {
            unreachable!("a list holds neither an Option nor a list")
        }
    }
}

/// The type an accessor hands back: the field's own, with every nested schema
/// named as the view it decodes to.
fn view_type(kind: &Kind<'_>) -> TokenStream {
    match kind {
        Kind::Str => quote!(&'buf str),
        Kind::Bytes => quote!(&'buf [u8]),
        Kind::Scalar(scalar) => quote!(#scalar),
        Kind::Value(path) => quote!(#path),
        Kind::Nested(nested) => view_of(*nested),
        Kind::Repeated(item) => {
            let element = element_of(item);
            quote!(::zerialize::ListView<'buf, #element>)
        }
        Kind::Optional(inner) => {
            let inner = view_type(inner);
            quote!(::core::option::Option<#inner>)
        }
    }
}

/// Reads the field out of `__message`, which decoding has already validated,
/// so that nothing here can fail.
fn read_field(kind: &Kind<'_>, slot: u32) -> TokenStream {
    let entry = Literal::u32_suffixed(slot);
    match kind {
        Kind::Str => quote!(__message.read_str(#entry).expect(#VALIDATED)),
        Kind::Bytes => quote!(__message.read_bytes(#entry).expect(#VALIDATED)),
        Kind::Scalar(scalar) => {
            let read = format_ident!("read_{}", scalar);
            quote!(__message.#read(#entry).expect(#VALIDATED))
        }
        Kind::Value(path) => quote! {
            <#path as ::zerialize::Value>::decode_value(__message, #entry).expect(#VALIDATED)
        },
        Kind::Nested(nested) => {
            let schema = schema_of(*nested);
            quote! {
                <#schema as ::zerialize::Zerializable>::decode_view(
                    __message.read_message(#entry).expect(#VALIDATED),
                )
                .expect(#VALIDATED)
            }
        }
        Kind::Repeated(_) => quote!(__message.read_list(#entry).expect(#VALIDATED)),
        Kind::Optional(inner) => {
            let read = read_field(inner, slot);
            quote! {
                if __message.is_present(#entry).expect(#VALIDATED) {
                    ::core::option::Option::Some(#read)
                } else {
                    ::core::option::Option::None
                }
            }
        }
    }
}

/// Reads the field to check it, which is what validation is: it leaves the
/// accessors above nothing that can fail.
fn check_field(kind: &Kind<'_>, slot: u32) -> TokenStream {
    let entry = Literal::u32_suffixed(slot);
    match kind {
        Kind::Str => quote!(__message.read_str(#entry)?;),
        Kind::Bytes => quote!(__message.read_bytes(#entry)?;),
        Kind::Scalar(scalar) => {
            let read = format_ident!("read_{}", scalar);
            quote!(__message.#read(#entry)?;)
        }
        Kind::Value(path) => {
            quote!(<#path as ::zerialize::Value>::decode_value(__message, #entry)?;)
        }
        Kind::Nested(nested) => {
            let schema = schema_of(*nested);
            quote! {
                <#schema as ::zerialize::Zerializable>::decode_view(
                    __message.read_message(#entry)?,
                )?;
            }
        }
        Kind::Repeated(item) => {
            let element = element_of(item);
            quote!(__message.read_list::<#element>(#entry)?;)
        }
        // An absent field is not a missing one, so it is checked only where it
        // is present.
        Kind::Optional(inner) => {
            let check = check_field(inner, slot);
            quote! {
                if __message.is_present(#entry)? {
                    #check
                }
            }
        }
    }
}

/// Statements that return `false` from a comparison where the two sides differ.
fn compare_field(kind: &Kind<'_>, mine: &TokenStream, theirs: &TokenStream) -> TokenStream {
    match kind {
        Kind::Repeated(_) => compare_lists(mine, theirs),
        Kind::Optional(inner) => {
            let compared = compare_field(inner, &quote!(__some), &quote!(__other));
            quote! {
                match (#mine, #theirs) {
                    (
                        ::core::option::Option::Some(__some),
                        ::core::option::Option::Some(__other),
                    ) => {
                        #compared
                    }
                    (::core::option::Option::None, ::core::option::Option::None) => {}
                    _ => return false,
                }
            }
        }
        _ => quote! {
            if #mine != #theirs {
                return false;
            }
        },
    }
}

/// Statements that return `false` where two lists differ, which is where they
/// hold different numbers of elements or any pair of them differs. The two
/// sides hold their own elements, so this is a comparison across whatever each
/// was decoded or built from.
fn compare_lists(mine: &TokenStream, theirs: &TokenStream) -> TokenStream {
    quote! {
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
    }
}

fn generate_schema(schema: &Schema<'_>, derived: Derived) -> TokenStream {
    let Schema {
        visibility,
        name,
        methods,
    } = schema;
    let view = format_ident!("{}View", name);
    let source = format_ident!("__{}Source", name);
    // The offset table is indexed by slot, so it is as long as the highest one.
    let slots = table(methods.iter().map(|method| method.slot));

    let frame = format_ident!("__frame");
    let encoded = methods.iter().map(|method| {
        let value = {
            let method = method.name;
            quote!(<Self as #name>::#method(self))
        };
        write_field(&method.kind, &frame, &entry_of(method.slot), &value)
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
        let return_type = view_type(&method.kind);
        let body = read_field(&method.kind, method.slot);
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
        compare_field(&method.kind, &mine, &theirs)
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
    let checks = methods
        .iter()
        .map(|method| check_field(&method.kind, method.slot));
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

        // So that a list may hold this schema. A list is a frame, so an
        // element is read out of it the way a field is read out of a message.
        impl ::zerialize::Element for dyn #name {
            type Item<'buf> = #view<'buf>;

            fn decode_element<'buf>(
                __list: ::zerialize::Message<'buf>,
                __index: ::core::primitive::u32,
            ) -> ::core::result::Result<Self::Item<'buf>, ::zerialize::Error> {
                <Self as ::zerialize::Zerializable>::decode_view(__list.read_message(__index)?)
            }
        }

        // So that an enum may carry this schema: an enum over the name of a
        // schema carries nothing, since it is a name rather than a value.
        impl ::zerialize::SchemaArg for dyn #name {
            type Value = ::zerialize::SchemaOnly;
        }
    }
}

/// Writes what a variant carries into the entry it occupies of the payload
/// frame `frame` marks, given the binding its field was matched to.
fn write_payload(
    payload: &Payload<'_>,
    params: &[Param<'_>],
    frame: &Ident,
    slot: u32,
    binding: &TokenStream,
) -> TokenStream {
    if let Payload::Optional(inner) = payload {
        let written = write_payload(inner, params, frame, slot, &quote!(__some));
        return quote! {
            if let ::core::option::Option::Some(__some) = #binding {
                #written
            }
        };
    }

    // A list is a frame of its own wherever it is held, so a variant carrying
    // one writes it exactly as a message holding one does.
    if let Payload::Repeated(param) = payload {
        let item = carried_item(param).clone();
        let entry = entry_of(slot);
        return write_field(&Kind::Repeated(Box::new(item)), frame, &entry, binding);
    }

    let entry = Literal::usize_suffixed(slot as usize);
    let write = match payload {
        Payload::Scalar(scalar) => {
            let write = format_ident!("write_{}", scalar);
            quote!(__writer.#write(*#binding);)
        }
        Payload::Str => quote!(__writer.write_str(#binding);),
        Payload::Bytes => quote!(__writer.write_bytes(#binding);),
        Payload::Value(path) => quote! {
            <#path as ::zerialize::Value>::encode_value(#binding, __writer);
        },
        Payload::Nested(carried) => {
            let schema = carried_schema(carried);
            quote! {
                <#schema as ::zerialize::Zerializable>::encode_source(#binding, __writer);
            }
        }
        Payload::Choice(path) => {
            let schema = choice_schema(path, params);
            quote! {
                <#schema as ::zerialize::Zerializable>::encode_source(#binding, __writer);
            }
        }
        Payload::Repeated(_) => unreachable!("a list is written above"),
        Payload::Optional(_) => unreachable!("an optional field is written above"),
    };
    quote! {
        __writer.begin_entry(&#frame, #entry);
        #write
    }
}

/// The schema a parameter carrying a message stands for.
fn carried_schema(param: &Param<'_>) -> TokenStream {
    let Carried::Message(schema) = &param.carried else {
        unreachable!("a nested field carries a message");
    };
    schema_of(Nested::Message(schema))
}

/// What the list a parameter carries holds.
fn carried_item<'a>(param: &'a Param<'_>) -> &'a Kind<'a> {
    let Carried::List(item) = &param.carried else {
        unreachable!("a repeated field carries a list");
    };
    item
}

/// The schema a nested enum names, which is that enum over the schemas this
/// one's parameters stand for: a field written `Reachable<P>` is carried as
/// itself, and named `Reachable<dyn Person>`.
fn choice_schema(path: &TypePath, params: &[Param<'_>]) -> TokenStream {
    let mut path = path.clone();
    if let Some(segment) = path.path.segments.last_mut()
        && let PathArguments::AngleBracketed(arguments) = &mut segment.arguments
    {
        for argument in &mut arguments.args {
            match argument {
                GenericArgument::Type(ty) => {
                    if let Type::Path(carried) = ty
                        && let Some(name) = named(carried)
                        && let Some(param) = params.iter().find(|param| param.name == name)
                    {
                        let schema = param_schema(param, &quote!('static));
                        *ty = parse_quote!(#schema);
                    }
                }
                // A schema is a name rather than a buffer, and is named for any
                // lifetime, exactly as the enum holding it is.
                GenericArgument::Lifetime(lifetime) => *lifetime = parse_quote!('static),
                _ => (),
            }
        }
    }
    path.into_token_stream()
}

/// Reads what a variant carries out of `__payload`, which is where reading a
/// choice is checked: the fields of the variant its tag named.
fn read_payload(payload: &Payload<'_>, params: &[Param<'_>], slot: u32) -> TokenStream {
    let entry = Literal::u32_suffixed(slot);
    match payload {
        Payload::Scalar(scalar) => {
            let read = format_ident!("read_{}", scalar);
            quote!(__payload.#read(#entry)?)
        }
        Payload::Str => quote!(__payload.read_str(#entry)?),
        Payload::Bytes => quote!(__payload.read_bytes(#entry)?),
        Payload::Value(path) => {
            quote!(<#path as ::zerialize::Value>::decode_value(__payload, #entry)?)
        }
        Payload::Nested(carried) => {
            let schema = carried_schema(carried);
            quote! {
                <#schema as ::zerialize::Zerializable>::decode_view(
                    __payload.read_message(#entry)?,
                )?
            }
        }
        Payload::Choice(path) => {
            let schema = choice_schema(path, params);
            quote! {
                <#schema as ::zerialize::Zerializable>::decode_view(
                    __payload.read_message(#entry)?,
                )?
            }
        }
        Payload::Repeated(param) => {
            let element = element_of(carried_item(param));
            quote!(__payload.read_list::<#element>(#entry)?)
        }
        Payload::Optional(inner) => {
            let read = read_payload(inner, params, slot);
            quote! {
                if __payload.is_present(#entry)? {
                    ::core::option::Option::Some(#read)
                } else {
                    ::core::option::Option::None
                }
            }
        }
    }
}

/// What a matched field is handed out as by `as_ref`: a message and a list are
/// borrowed, and a scalar, which is carried by value, is copied.
fn borrow_payload(payload: &Payload<'_>, binding: &TokenStream) -> TokenStream {
    match payload {
        // A scalar and a value are carried by value, and a borrowed field is
        // already a handle, so a reference to one is not what the borrowed enum
        // holds.
        Payload::Scalar(_) | Payload::Str | Payload::Bytes | Payload::Value(_) => {
            quote!(*#binding)
        }
        Payload::Nested(_) | Payload::Repeated(_) => quote!(#binding),
        // A nested enum hands out what it carries the same way, which is what
        // makes the enum holding it one over references too.
        Payload::Choice(_) => quote!(#binding.as_ref()),
        Payload::Optional(inner) => match &**inner {
            Payload::Nested(_) | Payload::Repeated(_) => {
                quote!(::core::option::Option::as_ref(#binding))
            }
            Payload::Choice(_) => quote! {
                ::core::option::Option::map(
                    ::core::option::Option::as_ref(#binding),
                    |__nested| __nested.as_ref(),
                )
            },
            _ => quote!(*#binding),
        },
    }
}

/// Statements that return `false` from a comparison where the two sides differ.
fn compare_payload(payload: &Payload<'_>, mine: &TokenStream, theirs: &TokenStream) -> TokenStream {
    match payload {
        // A list is compared element by element, as the same list is where a
        // message holds one: the two sides hold their own elements, which are
        // only compared against each other.
        Payload::Repeated(_) => compare_lists(mine, theirs),
        Payload::Optional(inner) => {
            let compared = compare_payload(inner, &quote!(__some), &quote!(__other));
            quote! {
                match (#mine, #theirs) {
                    (
                        ::core::option::Option::Some(__some),
                        ::core::option::Option::Some(__other),
                    ) => {
                        #compared
                    }
                    (::core::option::Option::None, ::core::option::Option::None) => {}
                    _ => return false,
                }
            }
        }
        _ => quote! {
            if #mine != #theirs {
                return false;
            }
        },
    }
}

/// Whether a field carries `param`, which is what an enum's parameters are
/// checked against: a parameter no field carries is one the generated enum
/// would not use.
fn carries(payload: &Payload<'_>, param: &Param<'_>) -> bool {
    match payload {
        Payload::Nested(by) | Payload::Repeated(by) => by.name == param.name,
        // A nested enum is written over this one's parameters, so it carries
        // whichever of them it is named with.
        Payload::Choice(path) => carried_by(path, param),
        Payload::Optional(inner) => carries(inner, param),
        Payload::Scalar(_) | Payload::Str | Payload::Bytes | Payload::Value(_) => false,
    }
}

/// Whether a nested enum is named with `param`.
fn carried_by(path: &TypePath, param: &Param<'_>) -> bool {
    path.path.segments.last().is_some_and(|segment| {
        matches!(&segment.arguments, PathArguments::AngleBracketed(arguments)
        if arguments.args.iter().any(|argument| {
            matches!(argument, GenericArgument::Type(Type::Path(carried))
                if named(carried).is_some_and(|name| name == param.name))
        }))
    })
}

/// Whether a field points into the buffer, which is what the enum's lifetime
/// stands for.
fn borrows(payload: &Payload<'_>) -> bool {
    match payload {
        Payload::Str | Payload::Bytes => true,
        // A list is bound by what it holds, so it is written in terms of the
        // enum's lifetime exactly where its elements point into the buffer.
        Payload::Repeated(param) => borrowed_item(carried_item(param)),
        // A nested enum is named with the lifetime it borrows from, which is
        // the one the enum holding it declares.
        Payload::Choice(path) => borrows_from_a_lifetime(path),
        Payload::Optional(inner) => borrows(inner),
        Payload::Scalar(_) | Payload::Value(_) | Payload::Nested(_) => false,
    }
}

/// Whether a nested enum is named with a lifetime, which is what an enum that
/// borrows is named with.
fn borrows_from_a_lifetime(path: &TypePath) -> bool {
    path.path.segments.last().is_some_and(|segment| {
        matches!(&segment.arguments, PathArguments::AngleBracketed(arguments)
        if arguments.args.iter().any(|argument| {
            matches!(argument, GenericArgument::Lifetime(_))
        }))
    })
}

/// The enum instantiated over `lifetime` and `arguments`, which is the enum
/// itself when it neither borrows nor carries schemas.
fn instantiate<T: ToTokens>(
    name: &Ident,
    lifetime: Option<&TokenStream>,
    arguments: &[T],
) -> TokenStream {
    match (lifetime, arguments.is_empty()) {
        (None, true) => quote!(#name),
        (None, false) => quote!(#name<#(#arguments),*>),
        (Some(lifetime), true) => quote!(#name<#lifetime>),
        (Some(lifetime), false) => quote!(#name<#lifetime, #(#arguments),*>),
    }
}

fn generate_choice(item: &ItemEnum, parsed: &Choice<'_>, derived: Derived) -> TokenStream {
    let Choice {
        visibility,
        name,
        lifetime,
        params,
        variants,
    } = parsed;
    let source = format_ident!("__{}Source", name);

    // An enum that borrows is written in terms of the lifetime it declares, so
    // every instantiation below gives that lifetime whatever it stands for
    // there: the buffer a view borrows from, and nothing in particular where
    // the enum is only being named.
    let declared = lifetime.map(|lifetime| quote!(#lifetime));
    let buffer = lifetime.map(|_| quote!('buf));
    let named = lifetime.map(|_| quote!('__schema));
    let declaration = declaration(item, params, declared.as_ref());

    // The enum is instantiated three ways: over implementations of the schemas
    // it carries, which is what an encodable value is; over the names of those
    // schemas, which is the name of this one; and over their views, which is
    // what decoding it returns.
    let bounds = params
        .iter()
        .map(|param| {
            let name = param.name;
            let bound = param_bound(param, declared.as_ref());
            quote!(#name: #bound)
        })
        .collect::<Vec<_>>();
    let generics = match (&declared, bounds.is_empty()) {
        (None, true) => TokenStream::new(),
        (None, false) => quote!(<#(#bounds),*>),
        (Some(lifetime), true) => quote!(<#lifetime>),
        (Some(lifetime), false) => quote!(<#lifetime, #(#bounds),*>),
    };
    let parameters = params.iter().map(|param| param.name).collect::<Vec<_>>();
    let encodable = instantiate(name, declared.as_ref(), &parameters);

    // A list is a view of the buffer wherever the enum is named, so an enum
    // carrying one is named for a lifetime whether or not it declares one.
    let schema_lifetime = quote!('__schema);
    let schemas = params
        .iter()
        .map(|param| param_schema(param, &schema_lifetime))
        .collect::<Vec<_>>();
    // The schema is named for any lifetime, so that naming it never has to name
    // one: `Enum<'_, dyn Trait>` is the schema wherever it is written.
    let schema_generics =
        (lifetime.is_some() || params.iter().any(is_list)).then(|| quote!(<#schema_lifetime>));
    let schema = instantiate(name, named.as_ref(), &schemas);
    let views = params.iter().map(param_view).collect::<Vec<_>>();
    let view = instantiate(name, buffer.as_ref(), &views);

    let encoded = variants.iter().map(|variant| {
        let tag = Literal::u32_suffixed(variant.tag);
        let pattern = pattern(name, &variant.written, "__field");
        let payload = if variant.fields.is_empty() {
            TokenStream::new()
        } else {
            let slots = table(variant.fields.iter().map(|field| field.slot));
            let frame = format_ident!("__payload");
            let written = variant.fields.iter().enumerate().map(|(index, field)| {
                let binding = binding("__field", index);
                write_payload(
                    &field.payload,
                    params,
                    &frame,
                    field.slot,
                    &quote!(#binding),
                )
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
            .map(|field| read_payload(&field.payload, params, field.slot))
            .collect::<Vec<_>>();
        let built = construct(name, &variant.written, &read);
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
    // The enum a view holds borrows from the buffer, and the one it is compared
    // against borrows from wherever it was built, so the two sides are named
    // over their own lifetimes.
    let other_lifetime = lifetime.map(|_| quote!('__other));
    let compared_generics = {
        let bounds = params.iter().zip(&others).map(|(param, other)| {
            let name = param.name;
            let bound = param_bound(param, declared.as_ref());
            match &param.carried {
                Carried::Message(_) => {
                    quote!(#name: #bound + ::core::cmp::PartialEq<#other>, #other: #bound)
                }
                // Two lists are compared element by element, so what each side
                // is bound by is its own elements: the comparison between them
                // is a bound on those, below.
                Carried::List(_) => {
                    let other_bound = param_bound(param, other_lifetime.as_ref());
                    quote!(#name: #bound, #other: #other_bound)
                }
            }
        });
        let lifetimes = declared.iter().chain(other_lifetime.iter());
        let bounds = lifetimes
            .map(|lifetime| quote!(#lifetime))
            .chain(bounds)
            .collect::<Vec<_>>();
        if bounds.is_empty() {
            TokenStream::new()
        } else {
            quote!(<#(#bounds),*>)
        }
    };
    // What a list holds is only known to be comparable where it is asked for,
    // because a list is bound by what it holds rather than by the schema of it.
    let compared_elements = params
        .iter()
        .zip(&others)
        .filter(|(param, _)| is_list(param))
        .map(|(param, other)| {
            let name = param.name;
            quote! {
                <#name as ::zerialize::List>::Item:
                    ::core::cmp::PartialEq<<#other as ::zerialize::List>::Item>,
            }
        })
        .collect::<Vec<_>>();
    let compared_where =
        (!compared_elements.is_empty()).then(|| quote!(where #(#compared_elements)*));
    let compared_to = instantiate(name, other_lifetime.as_ref(), &others);

    // The enum over references to what it carries. A message hands out an enum
    // by value, since the enum a view holds is decided when it is read rather
    // than stored anywhere to be pointed at, so this is how an implementation
    // hands out one of its own without copying what it carries.
    let borrowed_to = instantiate(
        name,
        declared.as_ref(),
        &params
            .iter()
            .map(|param| {
                let name = param.name;
                quote!(&#name)
            })
            .collect::<Vec<_>>(),
    );
    let borrowed = variants.iter().map(|variant| {
        let pattern = pattern(name, &variant.written, "__field");
        let fields = variant
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let binding = binding("__field", index);
                borrow_payload(&field.payload, &quote!(#binding))
            })
            .collect::<Vec<_>>();
        let built = construct(name, &variant.written, &fields);
        quote!(#pattern => #built,)
    });
    let borrowing = format!(
        "Borrows what every variant carries, so that a `{name}` an \
         implementation stores can be handed out as the one it encodes as."
    );
    let compared = derived.partial_eq.then(|| {
        let arms = variants.iter().map(|variant| {
            let mine = pattern(name, &variant.written, "__field");
            let theirs = pattern(name, &variant.written, "__other");
            let equal = variant.fields.iter().enumerate().map(|(index, field)| {
                let (mine, theirs) = (binding("__field", index), binding("__other", index));
                compare_payload(&field.payload, &quote!(#mine), &quote!(#theirs))
            });
            quote! {
                (#mine, #theirs) => {
                    #(#equal)*
                    true
                }
            }
        });
        // A single variant leaves nothing for a catch all arm to match.
        let unequal = if variants.len() > 1 {
            quote!(_ => false,)
        } else {
            TokenStream::new()
        };
        quote! {
            impl #compared_generics ::core::cmp::PartialEq<#compared_to> for #encodable
            #compared_where
            {
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

        impl #schema_generics ::zerialize::Zerializable for #schema {
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

        // So that a list may hold this choice, as it holds any other schema.
        impl #schema_generics ::zerialize::Element for #schema {
            type Item<'buf> = #view;

            fn decode_element<'buf>(
                __list: ::zerialize::Message<'buf>,
                __index: ::core::primitive::u32,
            ) -> ::core::result::Result<Self::Item<'buf>, ::zerialize::Error> {
                <Self as ::zerialize::Zerializable>::decode_view(__list.read_message(__index)?)
            }
        }
    }
}

/// The enum as it was declared, with every parameter rewritten to stand for
/// what its variants carry: an implementation stands for itself, and the name
/// of a schema for nothing that can be constructed.
///
/// A parameter carrying a list stands for the list itself, since the list a
/// view holds is a handle over the buffer rather than a name, so its bound is
/// rewritten instead: what every instantiation has in common is what its
/// elements are, which is what [`held_bound`] says.
fn declaration(
    item: &ItemEnum,
    params: &[Param<'_>],
    lifetime: Option<&TokenStream>,
) -> TokenStream {
    let mut item = item.clone();
    strip_variant_attributes(&mut item);
    for declared in &mut item.generics.params {
        let GenericParam::Type(declared) = declared else {
            continue;
        };
        let Some(param) = params.iter().find(|param| *param.name == declared.ident) else {
            continue;
        };
        match &param.carried {
            Carried::Message(_) => {
                declared.bounds.push(parse_quote!(::zerialize::SchemaArg));
                declared.bounds.push(parse_quote!(?Sized));
            }
            Carried::List(_) => {
                let bound = param_bound(param, lifetime);
                declared.bounds = parse_quote!(#bound);
            }
        }
    }
    for variant in &mut item.variants {
        for field in &mut variant.fields {
            if let Some(carried) = carried_type(&field.ty, params) {
                field.ty = carried;
            }
        }
    }
    item.into_token_stream()
}

/// A field's type with the parameter it carries rewritten to stand for what the
/// enum is instantiated over, or `None` where the field carries no parameter.
fn carried_type(ty: &Type, params: &[Param<'_>]) -> Option<Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    if let Some(inner) = optional(path) {
        let inner = carried_type(inner, params)?;
        return Some(parse_quote!(::core::option::Option<#inner>));
    }
    let name = named(path)?;
    let param = params.iter().find(|param| param.name == name)?;
    // A list is held as itself: what the enum is instantiated over is the list,
    // and it is only the schema it carries that a name stands in for.
    if is_list(param) {
        return None;
    }
    let carried = param.name;
    Some(parse_quote!(<#carried as ::zerialize::SchemaArg>::Value))
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
fn construct<T: ToTokens>(name: &Ident, written: &Written<'_>, values: &[T]) -> TokenStream {
    let variant = written.name;
    match written.style {
        Style::Unit => quote!(#name::#variant),
        Style::Tuple => quote!(#name::#variant(#(#values),*)),
        Style::Named => {
            let names = written
                .fields
                .iter()
                .map(|field| field.expect("a named variant's fields are named"));
            quote!(#name::#variant { #(#names: #values),* })
        }
    }
}

/// The pattern matching one variant, binding every field it carries.
fn pattern(name: &Ident, written: &Written<'_>, prefix: &str) -> TokenStream {
    let bindings = (0..written.fields.len())
        .map(|index| binding(prefix, index))
        .collect::<Vec<_>>();
    construct(name, written, &bindings)
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
    /// As the number of the variant alone, where no variant carries fields, and
    /// otherwise as a choice is: the number, and a frame of the fields it names.
    Enum(Vec<Variant<'a>>),
}

struct Field<'a> {
    /// The field's own name, where what holds it gives it one: a struct's
    /// fields are named, and a variant's are named where it names them.
    name: Option<&'a Ident>,
    slot: u32,
    /// A value's fields are what a `Copy` type may hold: nothing that borrows.
    kind: FieldKind<'a>,
}

enum FieldKind<'a> {
    Scalar(&'a Ident),
    Value(&'a Path),
    /// A field that may be absent, written as `Option<..>`.
    Optional(Box<FieldKind<'a>>),
}

struct Variant<'a> {
    written: Written<'a>,
    number: u32,
    fields: Vec<Field<'a>>,
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
    parse_value_fields(fields, "struct")
}

/// Parses the fields of a value, which a struct and one variant of an enum
/// declare alike.
fn parse_value_fields<'a>(
    fields: impl IntoIterator<Item = &'a syn::Field>,
    holder: &str,
) -> Result<Vec<Field<'a>>, Error> {
    let mut parsed: Vec<Field<'a>> = Vec::new();
    for field in fields {
        let Some((slot, attribute)) = declared_number(&field.attrs, "n")? else {
            return Err(Error::new_spanned(
                field,
                format!(
                    "every field of a #[derive(Zerializable)] {holder} requires a \
                     #[n(N)] attribute"
                ),
            ));
        };
        if let Some(index) = parsed.iter().position(|other| other.slot == slot) {
            let previous = describe(parsed[index].name, index);
            return Err(Error::new_spanned(
                attribute,
                format!("slot {slot} is already used by {previous}"),
            ));
        }
        parsed.push(Field {
            name: field.ident.as_ref(),
            slot,
            kind: parse_field_type(&field.ty)?,
        });
    }
    Ok(parsed)
}

fn parse_field_type(ty: &Type) -> Result<FieldKind<'_>, Error> {
    match ty {
        Type::Path(path) if optional(path).is_some() => {
            let inner = parse_field_type(optional(path).expect("the path names an Option"))?;
            require_not_optional(ty, matches!(inner, FieldKind::Optional(_)))?;
            Ok(FieldKind::Optional(Box::new(inner)))
        }
        Type::Path(path) if path.qself.is_none() => Ok(match scalar(path) {
            Some(scalar) => FieldKind::Scalar(scalar),
            None => FieldKind::Value(&path.path),
        }),
        _ => Err(Error::new_spanned(
            ty,
            "unsupported field type: expected a scalar, another value type, or an `Option` \
             of either. A value is `Copy`, so it may not hold anything borrowed or owned",
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
        if let Some((_, discriminant)) = &variant.discriminant {
            return Err(Error::new_spanned(
                discriminant,
                "#[derive(Zerializable)] variants may not have a discriminant: \
                 what names a variant on the wire is its #[variant(N)]",
            ));
        }
        let fields = parse_value_fields(&variant.fields, "variant")?;

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
                format!(
                    "variant {number} is already used by `{}`",
                    previous.written.name
                ),
            ));
        }
        parsed.push(Variant {
            written: Written::new(variant),
            number,
            fields,
        });
    }
    Ok(parsed)
}

fn generate_value(value: &Value<'_>) -> TokenStream {
    let Value { name, shape } = value;
    let (encode, decode) = match shape {
        Shape::Struct(fields) => generate_struct(fields),
        Shape::Enum(variants) => generate_enum(name, variants),
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

        // So that a list may hold this value. An element is addressed by its
        // index exactly as a value is addressed by its slot, because a list and
        // a message are the same frame.
        impl ::zerialize::Element for #name {
            type Item<'buf> = Self;

            fn decode_element<'buf>(
                __list: ::zerialize::Message<'buf>,
                __index: ::core::primitive::u32,
            ) -> ::core::result::Result<Self, ::zerialize::Error> {
                <Self as ::zerialize::Value>::decode_value(__list, __index)
            }
        }
    }
}

/// Writes one field of a value into the entry it occupies of the frame `frame`
/// marks.
fn write_value(kind: &FieldKind<'_>, frame: &Ident, slot: u32, value: &TokenStream) -> TokenStream {
    if let FieldKind::Optional(inner) = kind {
        let written = write_value(inner, frame, slot, &quote!(__some));
        return quote! {
            if let ::core::option::Option::Some(__some) = #value {
                #written
            }
        };
    }

    let entry = Literal::usize_suffixed(slot as usize);
    let write = match kind {
        FieldKind::Scalar(scalar) => {
            let write = format_ident!("write_{}", scalar);
            quote!(__writer.#write(#value);)
        }
        FieldKind::Value(path) => quote! {
            <#path as ::zerialize::Value>::encode_value(&#value, __writer);
        },
        FieldKind::Optional(_) => unreachable!("an optional field is written above"),
    };
    quote! {
        __writer.begin_entry(&#frame, #entry);
        #write
    }
}

/// Reads one field of a value out of the frame it was written as, which `frame`
/// names.
fn read_value(kind: &FieldKind<'_>, frame: &Ident, slot: u32) -> TokenStream {
    let entry = Literal::u32_suffixed(slot);
    match kind {
        FieldKind::Scalar(scalar) => {
            let read = format_ident!("read_{}", scalar);
            quote!(#frame.#read(#entry)?)
        }
        FieldKind::Value(path) => {
            quote!(<#path as ::zerialize::Value>::decode_value(#frame, #entry)?)
        }
        FieldKind::Optional(inner) => {
            let read = read_value(inner, frame, slot);
            quote! {
                if #frame.is_present(#entry)? {
                    ::core::option::Option::Some(#read)
                } else {
                    ::core::option::Option::None
                }
            }
        }
    }
}

/// A value struct is a frame, so it is read and written exactly as a message
/// is, and gains and loses fields with the same consequences.
fn generate_struct(fields: &[Field<'_>]) -> (TokenStream, TokenStream) {
    let slots = table(fields.iter().map(|field| field.slot));
    // The frame is begun below, and read back as the fields it holds.
    let written_to = format_ident!("__frame");
    let read_from = format_ident!("__fields");

    let written = fields.iter().map(|field| {
        let name = field.name.expect("a struct's fields are named");
        write_value(&field.kind, &written_to, field.slot, &quote!(self.#name))
    });

    let read = fields.iter().map(|field| {
        let name = field.name.expect("a struct's fields are named");
        let read = read_value(&field.kind, &read_from, field.slot);
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

/// A value enum whose variants carry nothing is its variant number and nothing
/// else, so it costs a `u32` wherever it is held. One whose variants carry
/// fields is written the way a choice is: the number naming the variant, and
/// the frame of the fields that variant declares.
fn generate_enum(name: &Ident, variants: &[Variant<'_>]) -> (TokenStream, TokenStream) {
    if variants.iter().all(|variant| variant.fields.is_empty()) {
        return generate_numbered(name, variants);
    }

    let frame = format_ident!("__payload");
    let written = variants.iter().map(|variant| {
        let number = Literal::u32_suffixed(variant.number);
        let pattern = pattern(name, &variant.written, "__field");
        let payload = if variant.fields.is_empty() {
            TokenStream::new()
        } else {
            let slots = table(variant.fields.iter().map(|field| field.slot));
            let written = variant.fields.iter().enumerate().map(|(index, field)| {
                let binding = binding("__field", index);
                write_value(&field.kind, &frame, field.slot, &quote!(*#binding))
            });
            quote! {
                let __payload = __writer.begin_payload(&__frame, #slots);
                #(#written)*
                __writer.end_frame(__payload);
            }
        };
        quote! {
            #pattern => {
                let __frame = __writer.begin_variant(#number);
                #payload
                __writer.end_frame(__frame);
            }
        }
    });

    let read = variants.iter().map(|variant| {
        let number = Literal::u32_suffixed(variant.number);
        let fields = variant
            .fields
            .iter()
            .map(|field| read_value(&field.kind, &frame, field.slot))
            .collect::<Vec<_>>();
        let built = construct(name, &variant.written, &fields);
        // A variant carrying nothing was written without a payload, so there is
        // none to read here either.
        if variant.fields.is_empty() {
            return quote!(#number => ::core::result::Result::Ok(#built),);
        }
        quote! {
            #number => {
                let __payload = __variant.read_payload()?;
                ::core::result::Result::Ok(#built)
            }
        }
    });

    (
        quote! {
            match self {
                #(#written)*
            }
        },
        quote! {
            let __variant = __message.read_message(__slot)?;
            match __variant.read_tag()? {
                #(#read)*
                _ => ::core::result::Result::Err(::zerialize::Error::UnknownVariant),
            }
        },
    )
}

/// A value enum whose variants all carry nothing, written as the number naming
/// the one it holds.
fn generate_numbered(name: &Ident, variants: &[Variant<'_>]) -> (TokenStream, TokenStream) {
    // A variant carrying nothing is still built and matched the way it was
    // declared, since `V`, `V()`, and `V {}` are three declarations to Rust.
    let nothing: [TokenStream; 0] = [];
    let written = variants.iter().map(|variant| {
        let pattern = pattern(name, &variant.written, "__field");
        let number = Literal::u32_suffixed(variant.number);
        quote!(#pattern => #number,)
    });

    let read = variants.iter().map(|variant| {
        let built = construct(name, &variant.written, &nothing);
        let number = Literal::u32_suffixed(variant.number);
        quote!(#number => ::core::result::Result::Ok(#built),)
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
