//! Sequences of messages.

use crate::wire::{Message, Vector};
use crate::{Error, Zerializable};
use std::fmt::{self, Debug};
use std::marker::PhantomData;

/// A sequence of messages.
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

/// A list of encoded messages, each decoded when it is asked for.
///
/// This is one vector's position in the buffer and nothing else, so it costs
/// the same however many elements it covers, and reaching any one of them is an
/// index into the offsets that vector is made of.
pub struct ListView<'buf, S: ?Sized> {
    vector: Vector<'buf>,
    schema: PhantomData<fn() -> *const S>,
}

impl<S: ?Sized> Clone for ListView<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: ?Sized> Copy for ListView<'_, S> {}

impl<'buf, S: ?Sized> ListView<'buf, S> {
    pub(crate) fn new(vector: Vector<'buf>) -> Self {
        Self {
            vector,
            schema: PhantomData,
        }
    }
}

impl<'buf, S: Zerializable + ?Sized> List for ListView<'buf, S> {
    type Item = S::View<'buf>;

    fn len(&self) -> usize {
        self.vector.len()
    }

    fn get(&self, index: usize) -> Option<S::View<'buf>> {
        let element = self
            .vector
            .element(index)
            .expect("elements are validated when the message is decoded")?;
        Some(
            S::decode_view(Message::element(self.vector.buf(), element, 0, false))
                .expect("elements are validated when the message is decoded"),
        )
    }
}

impl<'buf, S: Zerializable + ?Sized> ListView<'buf, S> {
    /// Decodes every element, so that [`List::get`] cannot fail afterwards.
    pub(crate) fn validate(&self, depth: u16) -> Result<(), Error> {
        for index in 0..self.len() {
            let element = self.vector.element(index)?.ok_or(Error::MissingField)?;
            S::decode_view(Message::element(self.vector.buf(), element, depth, true))?;
        }
        Ok(())
    }
}

impl<'buf, S: Zerializable + ?Sized> Debug for ListView<'buf, S>
where
    S::View<'buf>: Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}
