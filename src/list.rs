//! Sequences of elements.

use crate::Error;
use crate::wire::{Frame, Message, Writer};
use std::fmt::{self, Debug};
use std::marker::PhantomData;

/// A sequence of elements: messages, choices, values, or primitives.
///
/// Implementations return elements by value rather than by reference, which is
/// what lets [`ListView`] decode an element only when it is asked for.
pub trait List {
    type Item;

    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn get(&self, index: usize) -> Option<Self::Item>;

    fn iter(&self) -> ListIter<'_, Self>
    where
        Self: Sized,
    {
        ListIter {
            list: self,
            index: 0,
        }
    }
}

// Implemented for the reference so that a list may be handed out borrowed, as a
// schema may: a variant carrying a list is borrowed by `as_ref` the way one
// carrying a message is.
impl<L: List + ?Sized> List for &L {
    type Item = L::Item;

    fn len(&self) -> usize {
        L::len(self)
    }

    fn get(&self, index: usize) -> Option<L::Item> {
        L::get(self, index)
    }
}

pub struct ListIter<'a, L> {
    list: &'a L,
    index: usize,
}

impl<L: List> Iterator for ListIter<'_, L> {
    type Item = L::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.list.get(self.index)?;
        self.index += 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.list.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl<L: List> ExactSizeIterator for ListIter<'_, L> {}

/// A list of `Copy` items handed out by value, over one that hands out
/// references to them.
///
/// What a list of primitives or values holds, it holds by value, so this is
/// what an implementation storing them, an `OwnedList<u32>`, is handed out
/// through: `Copied(&self.scores)`.
#[derive(Clone, Copy, Debug)]
pub struct Copied<L>(pub L);

impl<'a, T: Copy + 'a, L: List<Item = &'a T>> List for Copied<L> {
    type Item = T;

    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<T> {
        self.0.get(index).copied()
    }
}

/// A list that owns its elements, for implementations that build data in
/// memory rather than decoding it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedList<T> {
    items: Vec<T>,
}

impl<T> OwnedList<T> {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn push(&mut self, item: T) {
        self.items.push(item);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn as_slice(&self) -> &[T] {
        &self.items
    }
}

impl<T> Default for OwnedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> From<Vec<T>> for OwnedList<T> {
    fn from(items: Vec<T>) -> Self {
        Self { items }
    }
}

impl<T> FromIterator<T> for OwnedList<T> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        Self {
            items: items.into_iter().collect(),
        }
    }
}

// Implemented for the reference so that an accessor can hand out `&self.field`,
// with elements borrowed from the list rather than cloned out of it.
impl<'a, T> List for &'a OwnedList<T> {
    type Item = &'a T;

    fn len(&self) -> usize {
        self.items.len()
    }

    fn get(&self, index: usize) -> Option<&'a T> {
        self.items.get(index)
    }
}

/// What an entry of a frame holds.
///
/// A list is a frame, exactly as a message is, with its entries indexed by
/// position rather than by slot, so an element and a field are the same thing:
/// both are addressed by the entry they occupy rather than handed their bytes,
/// because each chooses its own shape on the wire. That is what lets one trait
/// stand for both, and it is what decides whether a name a field is written
/// with is a value or a choice: which of them it is, is which implementation of
/// this exists, rather than how the name is written.
///
/// Implemented here for the primitives, and by the macros for every schema and
/// value.
pub trait Element {
    /// Types that may be encoded as this element.
    ///
    /// This is a GAT so sources containing borrowed data work, and is what a
    /// schema stands apart from an implementation of it by: the source of a
    /// choice is any instantiation of it, and the source of a value is the
    /// value itself.
    type Source<'src>: ?Sized + 'src;

    /// What reading one element gives back, which borrows from the buffer
    /// wherever the element itself does.
    type Item<'buf>;

    #[doc(hidden)]
    fn encode_element<'src>(source: &'src Self::Source<'src>, writer: &mut Writer);

    #[doc(hidden)]
    fn decode_element<'buf>(list: Message<'buf>, index: u32) -> Result<Self::Item<'buf>, Error>;
}

macro_rules! primitive_elements {
    ($($ty:ty: $write:ident, $read:ident,)*) => {
        $(
            impl Element for $ty {
                type Source<'src> = $ty;
                type Item<'buf> = $ty;

                fn encode_element(source: &$ty, writer: &mut Writer) {
                    writer.$write(*source);
                }

                fn decode_element<'buf>(
                    list: Message<'buf>,
                    index: u32,
                ) -> Result<$ty, Error> {
                    list.$read(index)
                }
            }
        )*
    };
}

primitive_elements! {
    bool: write_bool, read_bool,
    char: write_char, read_char,
    u8: write_u8, read_u8,
    u16: write_u16, read_u16,
    u32: write_u32, read_u32,
    u64: write_u64, read_u64,
    u128: write_u128, read_u128,
    i8: write_i8, read_i8,
    i16: write_i16, read_i16,
    i32: write_i32, read_i32,
    i64: write_i64, read_i64,
    i128: write_i128, read_i128,
    f32: write_f32, read_f32,
    f64: write_f64, read_f64,
}

// The two elements that are read as handles over the buffer rather than copied
// out of it are named by what they point at, since what a frame holds is the
// element rather than the reference to it.
impl Element for str {
    type Source<'src> = str;
    type Item<'buf> = &'buf str;

    fn encode_element(source: &str, writer: &mut Writer) {
        writer.write_str(source);
    }

    fn decode_element<'buf>(list: Message<'buf>, index: u32) -> Result<&'buf str, Error> {
        list.read_str(index)
    }
}

impl Element for [u8] {
    type Source<'src> = [u8];
    type Item<'buf> = &'buf [u8];

    fn encode_element(source: &[u8], writer: &mut Writer) {
        writer.write_bytes(source);
    }

    fn decode_element<'buf>(list: Message<'buf>, index: u32) -> Result<&'buf [u8], Error> {
        list.read_bytes(index)
    }
}

/// A list of encoded elements, each decoded when it is asked for.
///
/// This is the bytes of one frame and nothing else, so it costs the same as a
/// slice however many elements it covers, and reaching any one of them is an
/// index into that frame's offset table.
pub struct ListView<'buf, E: ?Sized> {
    frame: Frame<'buf>,
    element: PhantomData<fn() -> *const E>,
}

impl<E: ?Sized> Clone for ListView<'_, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E: ?Sized> Copy for ListView<'_, E> {}

impl<'buf, E: ?Sized> ListView<'buf, E> {
    pub(crate) fn new(frame: Frame<'buf>) -> Self {
        Self {
            frame,
            element: PhantomData,
        }
    }

    /// The list as the frame it is, which is what an element is read out of.
    fn as_message(&self, depth: u32, validate: bool) -> Message<'buf> {
        Message::nested(self.frame, depth, validate)
    }

    /// Where an element sits in that frame. An index past what an entry can be
    /// numbered by is one no list in a buffer this size can hold, since every
    /// entry costs a word of the offset table.
    fn position(index: usize) -> Option<u32> {
        u32::try_from(index).ok()
    }
}

impl<'buf, E: Element + ?Sized> List for ListView<'buf, E> {
    type Item = E::Item<'buf>;

    fn len(&self) -> usize {
        self.frame.count()
    }

    fn get(&self, index: usize) -> Option<E::Item<'buf>> {
        if index >= self.len() {
            return None;
        }
        let index = Self::position(index)?;
        Some(
            E::decode_element(self.as_message(0, false), index)
                .expect("elements are validated when the message is decoded"),
        )
    }
}

impl<'buf, E: Element + ?Sized> ListView<'buf, E> {
    /// Decodes every element, so that [`List::get`] cannot fail afterwards.
    pub(crate) fn validate(&self, depth: u32) -> Result<(), Error> {
        let list = self.as_message(depth, true);
        for index in 0..self.len() {
            let index = Self::position(index).ok_or(Error::UnexpectedEof)?;
            E::decode_element(list, index)?;
        }
        Ok(())
    }
}

impl<'buf, E: Element + ?Sized> Debug for ListView<'buf, E>
where
    E::Item<'buf>: Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}
