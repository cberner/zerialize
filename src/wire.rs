//! FlatBuffers, as the wire format `#[zerializable]`-generated code reads and
//! writes.
//!
//! A message is a table: a vtable of `u16` offsets indexed by slot number, and
//! the fields those offsets address. Scalars are stored in the table itself; a
//! string, a list, or a nested message is written elsewhere in the buffer and
//! reached through a `u32` offset the table holds in its place.
//!
//! ```text
//! table  := vtable: i32 backwards from here, fields
//! vtable := len: u16, table len: u16, offset[]: u16
//! string := len: u32, utf8[len], NUL
//! vector := len: u32, offset[len]: u32 forwards from themselves
//! scalar := fixed width, little endian, aligned to its own width
//! ```
//!
//! An enum is a table of two slots: the tag naming its variant, and the message
//! of that variant's fields, absent when the variant has none. The tag is read
//! before the payload, so the fields of one variant are free to occupy the same
//! slots as the fields of another.
//!
//! A vtable entry of 0, or a vtable too short to reach a slot, means the field
//! is absent, which is what lets a reader ignore slots it does not know: a
//! schema can gain fields without breaking readers built against the older one.
//! Scalars are written even when they are zero, so absent still means absent
//! rather than "the default", which is what a reader built against a newer
//! schema needs in order to reject a message that predates its fields.
//!
//! Buffers are size prefixed, since a flatbuffer otherwise has no extent of its
//! own: without it there is nothing to tell trailing bytes apart from a buffer
//! that simply ends there.
//!
//! Writing goes through `flatbuffers::FlatBufferBuilder`, which is what decides
//! layout, alignment, and which tables may share a vtable. A flatbuffer is
//! built back to front, so everything a message points at is written before the
//! message itself: encoding a field is in two parts, one that writes what the
//! field refers to and one that fills in the slot naming it.
//!
//! Reading does not go through the same crate: its accessors are unsafe, and
//! are sound only against a buffer that was verified first, while a decode here
//! is what does the verifying. Fields are read below with bounds checks
//! instead, so that decoding hostile input stays a matter of returning an
//! error.

use crate::{Error, ListView, Zerializable};
use flatbuffers::{
    FLATBUFFERS_MAX_BUFFER_SIZE, FlatBufferBuilder, SIZE_SOFFSET, SIZE_UOFFSET, SIZE_VOFFSET,
    TableUnfinishedWIPOffset, UnionWIPOffset, VOffsetT, WIPOffset, field_index_to_field_offset,
};
use std::cmp::Ordering;

/// Bytes of a vtable preceding its offsets: its own length and the length of
/// the tables it describes.
const VTABLE_HEADER: usize = 2 * SIZE_VOFFSET;

/// Maximum message nesting accepted when decoding.
///
/// Validation recurses once per level of nesting, so this bounds stack usage
/// on hostile input.
const MAX_DEPTH: u16 = 64;

/// Slots of the message an enum is encoded as.
const TAG: u32 = 0;
const PAYLOAD: u32 = 1;

/// The highest slot a message may have, since a vtable counts its offsets in
/// bytes from its own start with a `u16`.
const MAX_SLOT: u32 = (VOffsetT::MAX as u32 - VTABLE_HEADER as u32) / SIZE_VOFFSET as u32;

fn word(bytes: &[u8], at: usize, size: usize) -> Result<&[u8], Error> {
    let end = at.checked_add(size).ok_or(Error::UnexpectedEof)?;
    bytes.get(at..end).ok_or(Error::UnexpectedEof)
}

fn read_u16(bytes: &[u8], at: usize) -> Result<u16, Error> {
    let word = word(bytes, at, SIZE_VOFFSET)?;
    Ok(u16::from_le_bytes(
        word.try_into().expect("the slice is exactly sized"),
    ))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, Error> {
    let word = word(bytes, at, SIZE_UOFFSET)?;
    Ok(u32::from_le_bytes(
        word.try_into().expect("the slice is exactly sized"),
    ))
}

fn read_i32(bytes: &[u8], at: usize) -> Result<i32, Error> {
    let word = word(bytes, at, SIZE_SOFFSET)?;
    Ok(i32::from_le_bytes(
        word.try_into().expect("the slice is exactly sized"),
    ))
}

/// Follows the offset stored at `at`, which addresses forwards from itself.
fn follow(bytes: &[u8], at: usize) -> Result<u32, Error> {
    let offset = read_u32(bytes, at)? as usize;
    let target = at.checked_add(offset).ok_or(Error::UnexpectedEof)?;
    if target >= bytes.len() {
        return Err(Error::UnexpectedEof);
    }
    u32::try_from(target).map_err(|_| Error::UnexpectedEof)
}

/// Where a slot is recorded in the vtable of the message that holds it.
fn vtable_slot(slot: u32) -> VOffsetT {
    assert!(slot <= MAX_SLOT, "slot {slot} is too large to encode");
    field_index_to_field_offset(slot as VOffsetT)
}

/// Builds an encoded message.
#[derive(Default)]
pub struct Writer {
    builder: FlatBufferBuilder<'static>,
}

/// Where something already written into the buffer begins.
///
/// A flatbuffer is built back to front, so an offset is the handle on a string,
/// a list, or a message that exists in the buffer already, and is what a slot
/// of the message holding it is filled in with.
#[derive(Clone, Copy)]
pub struct Offset(WIPOffset<UnionWIPOffset>);

/// A message under construction, completed by [`Writer::end_message`].
#[must_use = "the message is only completed by Writer::end_message"]
pub struct MessageMark(WIPOffset<TableUnfinishedWIPOffset>);

macro_rules! write_scalars {
    ($($name:ident: $ty:ty,)*) => {
        $(
            pub fn $name(&mut self, slot: u32, value: $ty) {
                self.builder.push_slot_always(vtable_slot(slot), value);
            }
        )*
    };
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the encoded bytes, with `root` as the message they hold.
    pub fn finish(mut self, root: Offset) -> Vec<u8> {
        self.builder.finish_size_prefixed(root.0, None);
        self.builder.finished_data().to_vec()
    }

    /// Begins a message, whose slots are filled in through the `set_` methods
    /// until [`Writer::end_message`] closes it.
    ///
    /// Everything the message refers to must already be written: nothing else
    /// may be written into the buffer while a message is open.
    pub fn begin_message(&mut self) -> MessageMark {
        MessageMark(self.builder.start_table())
    }

    pub fn end_message(&mut self, mark: MessageMark) -> Offset {
        Offset(self.builder.end_table(mark.0).as_union_value())
    }

    pub fn write_str(&mut self, value: &str) -> Offset {
        Offset(self.builder.create_string(value).as_union_value())
    }

    pub fn write_bytes(&mut self, value: &[u8]) -> Offset {
        Offset(self.builder.create_vector(value).as_union_value())
    }

    /// Writes a list of messages, each of which is already written.
    pub fn write_list(&mut self, elements: &[Offset]) -> Offset {
        let elements = elements.iter().map(|element| element.0);
        Offset(
            self.builder
                .create_vector_from_iter(elements)
                .as_union_value(),
        )
    }

    /// Writes an enum: the tag naming its variant, and the message holding that
    /// variant's fields, which a variant carrying none does not have.
    pub fn write_variant(&mut self, tag: u32, payload: Option<Offset>) -> Offset {
        let mark = self.begin_message();
        self.set_u32(TAG, tag);
        if let Some(payload) = payload {
            self.set_offset(PAYLOAD, payload);
        }
        self.end_message(mark)
    }

    pub fn set_offset(&mut self, slot: u32, value: Offset) {
        self.builder.push_slot_always(vtable_slot(slot), value.0);
    }

    pub fn set_bool(&mut self, slot: u32, value: bool) {
        self.builder.push_slot_always(vtable_slot(slot), value);
    }

    write_scalars! {
        set_u8: u8,
        set_u16: u16,
        set_u32: u32,
        set_u64: u64,
        set_i8: i8,
        set_i16: i16,
        set_i32: i32,
        set_i64: i64,
        set_f32: f32,
        set_f64: f64,
    }
}

/// A list of encoded messages: the buffer it lives in, and where in it the
/// vector's length is.
///
/// This costs the same as a slice however many elements it covers, and reaching
/// any one of them is an index into the offsets that follow the length.
#[derive(Clone, Copy)]
pub(crate) struct Vector<'buf> {
    buf: &'buf [u8],
    loc: u32,
}

impl<'buf> Vector<'buf> {
    /// Reads the vector beginning at `loc`, checking only that its length and
    /// the offsets it counts are present. Elements are checked as they are
    /// read, which is what keeps this constant time.
    fn read(buf: &'buf [u8], loc: u32) -> Result<Self, Error> {
        let at = loc as usize;
        let elements = read_u32(buf, at)? as usize;
        let end = elements
            .checked_mul(SIZE_UOFFSET)
            .and_then(|size| size.checked_add(at + SIZE_UOFFSET))
            .ok_or(Error::UnexpectedEof)?;
        if end > buf.len() {
            return Err(Error::UnexpectedEof);
        }
        Ok(Self { buf, loc })
    }

    pub(crate) fn buf(&self) -> &'buf [u8] {
        self.buf
    }

    pub(crate) fn len(&self) -> usize {
        read_u32(self.buf, self.loc as usize)
            .expect("a vector's length was checked when it was read") as usize
    }

    /// Where the element at `index` begins, or `None` when the vector is not
    /// that long.
    pub(crate) fn element(&self, index: usize) -> Result<Option<u32>, Error> {
        if index >= self.len() {
            return Ok(None);
        }
        let at = self.loc as usize + SIZE_UOFFSET + index * SIZE_UOFFSET;
        follow(self.buf, at).map(Some)
    }
}

/// One encoded message, read by slot: the buffer it lives in, and where in it
/// the table begins.
///
/// A message is not a contiguous run of bytes the way a length prefixed frame
/// would be: its vtable is shared with every other message whose fields sit at
/// the same offsets, and everything it points at lies elsewhere in the buffer,
/// so a position in the whole buffer is the least a handle on one can be.
#[derive(Clone, Copy)]
pub struct Message<'buf> {
    buf: &'buf [u8],
    /// A flatbuffer addresses itself with 32 bit offsets, so a buffer larger
    /// than one can address is rejected outright, which is what keeps this and
    /// [`Vector::loc`] a `u32` rather than widening every handle over a buffer.
    loc: u32,
    depth: u16,
    /// Whether nested data is decoded as it is read. Decoding a message
    /// validates it in full, so that accessors on the resulting view cannot
    /// fail; reading through those accessors then skips the work.
    validate: bool,
}

macro_rules! read_scalars {
    ($($name:ident: $ty:ty,)*) => {
        $(
            pub fn $name(&self, slot: u32) -> Result<$ty, Error> {
                let value = word(self.buf, self.field(slot)?, size_of::<$ty>())?;
                Ok(<$ty>::from_le_bytes(
                    value.try_into().expect("the slice is exactly sized"),
                ))
            }
        )*
    };
}

impl<'buf> Message<'buf> {
    /// Reads the outermost message of a buffer, which the buffer's size prefix
    /// must account for exactly.
    pub(crate) fn root(buf: &'buf [u8]) -> Result<Self, Error> {
        // A flatbuffer addresses itself with 32 bit offsets, so one larger than
        // that cannot be read whatever it holds.
        if buf.len() > FLATBUFFERS_MAX_BUFFER_SIZE {
            return Err(Error::UnexpectedEof);
        }
        let size = read_u32(buf, 0)? as usize;
        match size.cmp(&(buf.len() - SIZE_UOFFSET)) {
            Ordering::Less => return Err(Error::TrailingBytes),
            Ordering::Greater => return Err(Error::UnexpectedEof),
            Ordering::Equal => (),
        }
        Ok(Self {
            buf,
            loc: follow(buf, SIZE_UOFFSET)?,
            depth: 0,
            validate: true,
        })
    }

    pub(crate) fn element(buf: &'buf [u8], loc: u32, depth: u16, validate: bool) -> Self {
        Self {
            buf,
            loc,
            depth,
            validate,
        }
    }

    /// The same message, over bytes a previous decode already validated.
    ///
    /// Only generated accessors should call this: reading through a message
    /// that was not validated can panic.
    #[doc(hidden)]
    pub fn trusted(self) -> Self {
        Self {
            depth: 0,
            validate: false,
            ..self
        }
    }

    /// Whether generated code should read this message's fields to check them.
    pub fn validates(&self) -> bool {
        self.validate
    }

    /// Where the field at `slot` begins, or `None` when the message does not
    /// carry it.
    fn entry(&self, slot: u32) -> Result<Option<usize>, Error> {
        if slot > MAX_SLOT {
            return Ok(None);
        }
        let table = self.loc as usize;
        // The vtable is addressed backwards from the table, and is shared by
        // every table whose fields sit at the same offsets, so it may lie on
        // either side of the one naming it.
        let vtable = i64::try_from(table).expect("a buffer is addressed by 32 bits")
            - i64::from(read_i32(self.buf, table)?);
        let vtable = usize::try_from(vtable).map_err(|_| Error::UnexpectedEof)?;
        let length = usize::from(read_u16(self.buf, vtable)?);
        if length < VTABLE_HEADER {
            return Err(Error::UnexpectedEof);
        }
        let entry = usize::from(vtable_slot(slot));
        if entry + SIZE_VOFFSET > length {
            return Ok(None);
        }
        match read_u16(self.buf, vtable + entry)? {
            0 => Ok(None),
            offset => Ok(Some(
                table
                    .checked_add(usize::from(offset))
                    .ok_or(Error::UnexpectedEof)?,
            )),
        }
    }

    fn field(&self, slot: u32) -> Result<usize, Error> {
        self.entry(slot)?.ok_or(Error::MissingField)
    }

    /// Where what the field at `slot` refers to begins: a string, a list, and a
    /// nested message are all written outside the message naming them.
    fn indirect(&self, slot: u32) -> Result<u32, Error> {
        follow(self.buf, self.field(slot)?)
    }

    pub fn read_str(&self, slot: u32) -> Result<&'buf str, Error> {
        str::from_utf8(self.read_bytes(slot)?).map_err(|_| Error::InvalidUtf8)
    }

    pub fn read_bytes(&self, slot: u32) -> Result<&'buf [u8], Error> {
        let at = self.indirect(slot)? as usize;
        let length = read_u32(self.buf, at)? as usize;
        word(self.buf, at + SIZE_UOFFSET, length)
    }

    pub fn read_bool(&self, slot: u32) -> Result<bool, Error> {
        match self.read_u8(slot)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::InvalidBool),
        }
    }

    /// Reads the tag of an encoded enum, naming the variant it holds.
    pub fn read_tag(&self) -> Result<u32, Error> {
        self.read_u32(TAG)
    }

    /// Reads the fields of the variant an enum holds, which are only meaningful
    /// once its tag has been read.
    pub fn read_payload(&self) -> Result<Message<'buf>, Error> {
        self.read_message(PAYLOAD)
    }

    /// Reads a nested message, which inherits this one's validation mode.
    pub fn read_message(&self, slot: u32) -> Result<Message<'buf>, Error> {
        if self.depth == MAX_DEPTH {
            return Err(Error::RecursionLimit);
        }
        Ok(Self {
            loc: self.indirect(slot)?,
            depth: self.depth + 1,
            ..*self
        })
    }

    /// Reads a list. When this message is being validated, every element is
    /// decoded here, so that reading one later cannot fail.
    pub fn read_list<S: Zerializable + ?Sized>(
        &self,
        slot: u32,
    ) -> Result<ListView<'buf, S>, Error> {
        if self.depth == MAX_DEPTH {
            return Err(Error::RecursionLimit);
        }
        let list = ListView::new(Vector::read(self.buf, self.indirect(slot)?)?);
        if self.validate {
            list.validate(self.depth + 1)?;
        }
        Ok(list)
    }

    read_scalars! {
        read_u8: u8,
        read_u16: u16,
        read_u32: u32,
        read_u64: u64,
        read_i8: i8,
        read_i16: i16,
        read_i32: i32,
        read_i64: i64,
        read_f32: f32,
        read_f64: f64,
    }
}
