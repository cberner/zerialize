//! Zero-copy serialization driven by schema traits.
//!
//! A schema is an ordinary Rust trait annotated with [`macro@zerializable`],
//! where every method declares the slot it occupies on the wire:
//!
//! ```
//! use zerialize::{decode, encode, zerializable};
//!
//! #[zerializable]
//! trait Point {
//!     #[n(0)]
//!     fn x(&self) -> i32;
//!
//!     #[n(1)]
//!     fn y(&self) -> i32;
//! }
//!
//! struct OwnedPoint {
//!     x: i32,
//!     y: i32,
//! }
//!
//! impl Point for OwnedPoint {
//!     fn x(&self) -> i32 {
//!         self.x
//!     }
//!     fn y(&self) -> i32 {
//!         self.y
//!     }
//! }
//!
//! let bytes = encode::<dyn Point>(&OwnedPoint { x: 1, y: 2 });
//! let point = decode::<dyn Point>(&bytes).unwrap();
//! assert_eq!((point.x(), point.y()), (1, 2));
//! ```
//!
//! `dyn Point` names the schema itself: any implementation may be encoded as it,
//! and decoding returns the generated zero-copy view, which borrows from the
//! input buffer instead of owning its contents. Because the view implements the
//! schema trait too, decoded data can be passed anywhere the trait is accepted,
//! including back into [`encode`].
//!
//! # Values
//!
//! A `Copy` struct or enum annotated with [`macro@Zerializable`] is a *value*:
//! a field type that decodes back to itself rather than to a view, because it
//! is read out whole rather than a field at a time. A value enum's variants may
//! carry fields of their own, as the struct beside them does; what separates a
//! value from a choice is that a value is `Copy`, and so is handed back as
//! itself.
//!
//! ```
//! use zerialize::{Zerializable, decode, encode, zerializable};
//!
//! #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
//! enum Unit {
//!     #[variant(0)]
//!     Celsius,
//!     #[variant(1)]
//!     Fahrenheit,
//! }
//!
//! #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
//! struct Temperature {
//!     #[n(0)]
//!     degrees: f32,
//!     #[n(1)]
//!     unit: Unit,
//! }
//!
//! #[zerializable]
//! trait Reading {
//!     #[n(0)]
//!     fn temperature(&self) -> Temperature;
//! }
//!
//! struct OwnedReading(Temperature);
//!
//! impl Reading for OwnedReading {
//!     fn temperature(&self) -> Temperature {
//!         self.0
//!     }
//! }
//!
//! let reading = OwnedReading(Temperature { degrees: 21.5, unit: Unit::Celsius });
//! let bytes = encode::<dyn Reading>(&reading);
//! assert_eq!(decode::<dyn Reading>(&bytes).unwrap().temperature(), reading.0);
//! ```
//!
//! A value may hold `&str` and `&[u8]`, which point into the buffer as the same
//! fields of a message do. One that does declares the lifetime it points into,
//! and is named with that lifetime wherever it is named:
//!
//! ```
//! use zerialize::{Zerializable, decode, encode, zerializable};
//!
//! #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
//! struct Stamp<'a> {
//!     #[n(0)]
//!     code: &'a str,
//!     #[n(1)]
//!     grams: u32,
//! }
//!
//! #[zerializable]
//! trait Parcel {
//!     #[n(0)]
//!     fn stamp(&self) -> Stamp<'_>;
//! }
//!
//! # struct OwnedParcel;
//! # impl Parcel for OwnedParcel {
//! #     fn stamp(&self) -> Stamp<'_> {
//! #         Stamp { code: "AB12", grams: 5 }
//! #     }
//! # }
//! let bytes = encode::<dyn Parcel>(&OwnedParcel);
//! assert_eq!(decode::<dyn Parcel>(&bytes).unwrap().stamp().code, "AB12");
//! ```
//!
//! Only a value that borrows declares a lifetime, so a value that declares none
//! holds nothing pointing into the buffer it was read from, and outlives it.
//! Either way it is `Copy`, which is what lets it be read out whole.
//!
//! A value and a choice are written alike where a field names one, so which of
//! the two a name stands for is which of them declared it: `Stamp<'_>` is a
//! value above, and would be a choice had a `#[zerializable]` enum declared it.
//!
//! # Optional fields
//!
//! Any field may be wrapped in an `Option`, which makes it one a message need
//! not carry:
//!
//! ```
//! use zerialize::{decode, encode, zerializable};
//!
//! #[zerializable]
//! trait Station {
//!     #[n(0)]
//!     fn degrees(&self) -> f32;
//!
//!     #[n(1)]
//!     fn name(&self) -> Option<&str>;
//! }
//!
//! struct OwnedStation(f32, Option<&'static str>);
//!
//! impl Station for OwnedStation {
//!     fn degrees(&self) -> f32 {
//!         self.0
//!     }
//!     fn name(&self) -> Option<&str> {
//!         self.1
//!     }
//! }
//!
//! let bytes = encode::<dyn Station>(&OwnedStation(21.5, None));
//! assert_eq!(decode::<dyn Station>(&bytes).unwrap().name(), None);
//! ```
//!
//! `None` is the slot left unwritten, which is exactly what a reader sees of a
//! slot the writer did not have. A field added as an optional one is therefore
//! readable out of a message written before it existed, where a required one
//! would be [`Error::MissingField`], and a reader built before it skips it as it
//! skips any slot it does not know. There is only one way for a slot to be
//! absent, so `Option<Option<..>>` is rejected.
//!
//! # Choices
//!
//! A schema may also be a choice between messages, written as an enum whose
//! variants declare the tag naming them on the wire, and whose fields declare
//! their slots as a message's do:
//!
//! ```
//! use zerialize::{decode, encode, zerializable};
//!
//! # #[zerializable]
//! # trait Point {
//! #     #[n(0)]
//! #     fn x(&self) -> i32;
//! #     #[n(1)]
//! #     fn y(&self) -> i32;
//! # }
//! # struct OwnedPoint {
//! #     x: i32,
//! #     y: i32,
//! # }
//! # impl Point for OwnedPoint {
//! #     fn x(&self) -> i32 {
//! #         self.x
//! #     }
//! #     fn y(&self) -> i32 {
//! #         self.y
//! #     }
//! # }
//! #[zerializable]
//! #[derive(Clone)]
//! enum Shape<P: Point> {
//!     #[variant(0)]
//!     Dot(#[n(0)] P),
//!     #[variant(1)]
//!     Empty,
//! }
//!
//! # impl Clone for OwnedPoint {
//! #     fn clone(&self) -> Self {
//! #         OwnedPoint { x: self.x, y: self.y }
//! #     }
//! # }
//! let dot: Shape<OwnedPoint> = Shape::Dot(OwnedPoint { x: 1, y: 2 });
//! let bytes = encode::<Shape<dyn Point>>(&dot);
//! match decode::<Shape<dyn Point>>(&bytes).unwrap() {
//!     Shape::Dot(point) => assert_eq!((point.x(), point.y()), (1, 2)),
//!     Shape::Empty => unreachable!(),
//! }
//! ```
//!
//! An enum is generic over the schemas it carries, which is what lets the same
//! declaration be a value, `Shape<OwnedPoint>`, the name of the schema that
//! value encodes as, `Shape<dyn Point>`, and, once decoded, the enum over
//! views, `Shape<PointView<'_>>`. Since a variant's payload is written in terms
//! of its parameter, building a value means giving it a type, as `dot` is given
//! one above.
//!
//! A variant carries nothing, a tuple of fields, or named fields, and is built
//! and matched the way it was declared: written `Dot { #[n(0)] at: P }`, the
//! variant above is built and matched as `Shape::Dot { at }`. What names a
//! field on the wire is its slot either way, so naming a field, or renaming
//! one, changes how the enum reads rather than what it encodes as.
//!
//! A variant may also carry a field that borrows, which is a handle over the
//! buffer as the same field is where a message holds it. An enum that has one
//! declares the lifetime it points into, and is named with that lifetime
//! wherever it is named:
//!
//! ```
//! # use zerialize::{decode, encode, zerializable};
//! #[zerializable(derive(PartialEq))]
//! #[derive(Debug)]
//! enum Label<'a> {
//!     #[variant(0)]
//!     Text(#[n(0)] &'a str),
//!     #[variant(1)]
//!     None,
//! }
//!
//! let bytes = encode::<Label<'_>>(&Label::Text("hello"));
//! assert_eq!(decode::<Label<'_>>(&bytes).unwrap(), Label::Text("hello"));
//! ```
//!
//! Only an enum that borrows declares a lifetime, so one that does not is named
//! as it was before: `Shape<dyn Point>`, not `Shape<'_, dyn Point>`.
//!
//! A variant may carry a list, which is a parameter of the enum as a nested
//! schema is, bound by what the list holds. The enum is named with the view of
//! that list in the parameter's place, because a list is a handle over the
//! buffer rather than a name:
//!
//! ```
//! # use zerialize::{List, ListView, OwnedList, decode, encode, zerializable};
//! # #[zerializable]
//! # trait Point {
//! #     #[n(0)]
//! #     fn x(&self) -> i32;
//! # }
//! # struct OwnedPoint(i32);
//! # impl Point for OwnedPoint {
//! #     fn x(&self) -> i32 {
//! #         self.0
//! #     }
//! # }
//! #[zerializable]
//! enum Path<P: List<Item: Point>> {
//!     #[variant(0)]
//!     Along(#[n(0)] P),
//!     #[variant(1)]
//!     Nowhere,
//! }
//!
//! let points: OwnedList<OwnedPoint> = vec![OwnedPoint(1), OwnedPoint(2)].into();
//! let along: Path<&OwnedList<OwnedPoint>> = Path::Along(&points);
//! let bytes = encode::<Path<ListView<'_, dyn Point>>>(&along);
//! match decode::<Path<ListView<'_, dyn Point>>>(&bytes).unwrap() {
//!     Path::Along(points) => assert_eq!(points.get(1).unwrap().x(), 2),
//!     Path::Nowhere => unreachable!(),
//! }
//! ```
//!
//! What every instantiation of the enum has in common is what its elements are,
//! which is why a list of messages is bound by the trait declaring them:
//! `Path<&OwnedList<OwnedPoint>>` holds points, and the list decoding gives
//! holds views of them. A list an enum carries may not itself hold an enum,
//! since an enum is a type rather than a trait and so has no bound that names
//! every instantiation of it; a message may hold that list instead.
//!
//! The enum is otherwise an ordinary enum, so what it needs is derived, above
//! the attribute or below it, bounding the enum's parameters the way a `derive`
//! does: `Shape<OwnedPoint>` is `Clone` where `OwnedPoint` is.
//!
//! # Printing and comparing
//!
//! A schema is asked for those rather than given them, because both are
//! implementations on its own types that a schema which is never printed or
//! compared has no use for:
//!
//! ```
//! use zerialize::{decode, encode, zerializable};
//!
//! #[zerializable(derive(Debug, PartialEq))]
//! trait Point {
//!     #[n(0)]
//!     fn x(&self) -> i32;
//! }
//!
//! # #[derive(Debug)]
//! struct OwnedPoint(i32);
//! # impl Point for OwnedPoint {
//! #     fn x(&self) -> i32 {
//! #         self.0
//! #     }
//! # }
//!
//! let point = OwnedPoint(1);
//! let bytes = encode::<dyn Point>(&point);
//! assert_eq!(decode::<dyn Point>(&bytes).unwrap(), point);
//! ```
//!
//! A view compares against *any* implementation of its schema, which is what
//! makes the assertion above read the way it does, and prints the fields it
//! stands for rather than the bytes it holds. Neither is an implementation that
//! could be written outside the schema, and asking for either asks the same of
//! the schemas and values that schema carries.
//!
//! A choice asks for `PartialEq` the same way, and means something stronger by
//! it: `#[zerializable(derive(PartialEq))]` compares any two instantiations, so
//! that `Shape<PointView<'_>>` compares against the `Shape<OwnedPoint>` it was
//! encoded from. That is the one implementation a `derive` cannot write, and it
//! covers the ordinary case as well, so `#[derive(PartialEq)]` is rejected
//! beside it. `Debug` is not offered there: an enum is declared by its author,
//! so it is printed by an ordinary `#[derive(Debug)]`.
//!
//! A message carries an enum by naming it the way its declaration reads, as
//! `fn shape(&self) -> Shape<impl Point + '_> where Self: Sized`, and an enum
//! carries an enum by naming it over its own parameters, as
//! `Outline(#[n(0)] Shape<P>)`, so a schema is free to be a tree of messages and
//! choices.

#![forbid(unsafe_code)]

mod list;
mod wire;

use std::fmt::{self, Display, Formatter};

pub use list::{Copied, Element, List, ListIter, ListView, OwnedList};
pub use wire::{FrameMark, Message, Writer};
pub use zerialize_macros::{Zerializable, zerializable};

/// Why a buffer could not be decoded as a schema.
///
/// Every variant describes the message rather than the reader: a buffer that
/// fails to decode is one that no reader of this schema accepts. More ways for
/// a message to be rejected may be named later, so a match on this needs a
/// wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    UnexpectedEof,
    InvalidUtf8,
    InvalidBool,
    /// A `char` that is not a Unicode scalar value.
    InvalidChar,
    TrailingBytes,
    MissingField,
    RecursionLimit,
    /// A variant number no variant of the reader's enum claims. Unlike an
    /// unknown slot, which a reader skips by never asking for it, an enum that
    /// gained a variant cannot be read by a reader built before it.
    UnknownVariant,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Error::UnexpectedEof => "message ended before a field it points at",
            Error::InvalidUtf8 => "string field is not valid UTF-8",
            Error::InvalidBool => "bool field is neither 0 nor 1",
            Error::InvalidChar => "char field is not a Unicode scalar value",
            Error::TrailingBytes => "bytes remain after the message",
            Error::MissingField => "required field is absent",
            Error::RecursionLimit => "message is nested deeper than the recursion limit",
            Error::UnknownVariant => "enum tag names no variant of this schema",
        };
        f.write_str(message)
    }
}

impl std::error::Error for Error {}

pub trait Zerializable {
    /// Types that may be encoded as this schema.
    ///
    /// This is a GAT so sources containing borrowed data work.
    type Source<'src>: ?Sized + 'src;

    /// The zero-copy view returned by decoding.
    type View<'buf>: 'buf;

    #[doc(hidden)]
    fn encode_source<'src>(source: &'src Self::Source<'src>, writer: &mut Writer);

    #[doc(hidden)]
    fn decode_view<'buf>(message: Message<'buf>) -> Result<Self::View<'buf>, Error>;
}

/// A field that decodes back into itself rather than into a view.
///
/// A value is `Copy`, which is what lets it be read out whole: a schema's
/// accessor hands back the value itself, not a handle over the bytes it was
/// read from. A value that declares no lifetime holds nothing borrowed, and so
/// outlives the buffer it was read from; one that declares a lifetime points
/// into that buffer as a `&str` field of a schema does, and is `Copy` all the
/// same. Nothing more is required of it: a schema asked to print or compare its
/// fields needs this one to be `Debug` or `PartialEq`, but a schema that is
/// asked for neither does not.
///
/// Reading and writing a value is [`Element`], which a value shares with the
/// schemas and primitives a field may hold. This says what a value *is*.
///
/// Implemented by `#[derive(Zerializable)]` on a `Copy` struct or enum.
pub trait Value: Copy {}

/// Encodes `source` as the schema `S`.
///
/// Encoding cannot fail: every source that fits in memory has an encoding.
pub fn encode<'src, S>(source: &'src S::Source<'src>) -> Vec<u8>
where
    S: Zerializable + ?Sized,
{
    let mut writer = Writer::new();
    <S as Zerializable>::encode_source(source, &mut writer);
    writer.finish()
}

/// Decodes a view of `bytes` as the schema `S`.
///
/// The returned view is a handle over `bytes`; nothing is copied out of it,
/// and fields are read from the buffer as they are asked for. Decoding checks
/// the whole message up front, which is what allows those later reads to be
/// infallible.
pub fn decode<'buf, S>(bytes: &'buf [u8]) -> Result<S::View<'buf>, Error>
where
    S: Zerializable + ?Sized,
{
    <S as Zerializable>::decode_view(Message::root(bytes)?)
}

/// What a schema argument stands for.
///
/// A generated enum is generic over the schemas its variants carry, which is
/// what lets one declaration be both an ordinary value, `Worker<OwnedPerson>`,
/// and the name of the schema that value encodes as, `Worker<dyn Person>`. An
/// implementation stands for itself, so a variant of `Worker<OwnedPerson>`
/// carries an `OwnedPerson`; a schema stands for [`SchemaOnly`], so the same
/// variant of `Worker<dyn Person>` carries nothing that can be constructed.
///
/// The blanket implementation covers every sized type, and [`macro@zerializable`]
/// implements this for the `dyn Trait` that names a schema. Code generic over a
/// schema an enum carries should bound its parameter by that schema's trait
/// rather than by this, which is what lets the payload resolve to the parameter
/// itself.
pub trait SchemaArg {
    type Value;
}

impl<T> SchemaArg for T {
    type Value = T;
}

/// What a variant carries under the name of a schema.
///
/// `Worker<dyn Person>` names a schema rather than holding one, so its variants
/// carry this, which no value inhabits.
pub enum SchemaOnly {}
