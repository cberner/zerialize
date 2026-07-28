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
//!     #[slot(0)]
//!     fn x(&self) -> i32;
//!
//!     #[slot(1)]
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

#![forbid(unsafe_code)]

mod list;
mod wire;

pub use list::{List, ListIter, ListView, OwnedList};
pub use wire::{FrameMark, Message, Writer};
pub use zerialize_macros::zerializable;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    UnexpectedEof,
    InvalidUtf8,
    InvalidBool,
    TrailingBytes,
    MissingField,
    RecursionLimit,
}

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
