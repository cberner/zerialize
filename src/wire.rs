//! Wire format primitives used by `#[zerializable]`-generated code.
//!
//! The encoding is deliberately simple, and is not stable. Messages and lists
//! are the same primitive, a frame: a length, an entry count, and a table of
//! offsets indexed by slot number for a message, or by position for a list.
//!
//! ```text
//! frame  := len: u64, count: u64, offset[count]: u64, data
//! str    := len: u64, utf8[len]
//! bytes  := len: u64, byte[len]
//! scalar := fixed width, little endian
//! ```
//!
//! An enum is a frame of two entries: the tag naming its variant, and the frame
//! of that variant's fields, absent when the variant has none. The tag is read
//! before the payload, so the fields of one variant are free to occupy the same
//! slots as the fields of another.
//!
//! Offsets are relative to the start of their own frame, which leaves 0 free
//! to mean "absent": no entry can begin inside the header. Indexing the table
//! by slot number is what makes reading a field a constant number of loads
//! rather than a walk over the fields before it, and it is why a decoded view
//! can be nothing more than the bytes of its frame. A reader ignores slots it
//! does not know by never indexing them, so a schema can gain fields without
//! breaking readers built against the older one.

use crate::{Error, ListView, Zerializable};

const WORD: usize = size_of::<u64>();

/// Bytes preceding a frame's offset table: its length and entry count.
const HEADER: usize = 2 * WORD;

/// Maximum message nesting accepted when decoding.
///
/// Validation recurses once per level of nesting, so this bounds stack usage
/// on hostile input.
const MAX_DEPTH: u32 = 64;

/// Slots of the frame an enum is encoded as.
const TAG: u32 = 0;
const PAYLOAD: u32 = 1;
const VARIANT_SLOTS: usize = 2;

fn read_word(bytes: &[u8], at: usize) -> Result<usize, Error> {
    let end = at.checked_add(WORD).ok_or(Error::UnexpectedEof)?;
    let word = bytes.get(at..end).ok_or(Error::UnexpectedEof)?;
    // A value that does not fit in a usize cannot address anything in the
    // buffer, so it is as out of bounds as a value past its end.
    usize::try_from(u64::from_le_bytes(
        word.try_into().expect("the slice is exactly a word"),
    ))
    .map_err(|_| Error::UnexpectedEof)
}

/// Builds an encoded message.
#[derive(Default)]
pub struct Writer {
    output: Vec<u8>,
}

/// Position of a frame's header, filled in by [`Writer::end_frame`].
#[must_use = "the frame is only completed by Writer::end_frame"]
pub struct FrameMark {
    start: usize,
    table: usize,
}

macro_rules! write_scalars {
    ($($name:ident: $ty:ty,)*) => {
        $(
            pub fn $name(&mut self, value: $ty) {
                self.output.extend_from_slice(&value.to_le_bytes());
            }
        )*
    };
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the encoded bytes.
    pub fn finish(self) -> Vec<u8> {
        self.output
    }

    /// Begins a frame of `count` entries, reserving its offset table. Entries
    /// left unwritten keep their zeroed offset, and so decode as absent.
    pub fn begin_frame(&mut self, count: usize) -> FrameMark {
        let start = self.output.len();
        self.output.extend_from_slice(&0u64.to_le_bytes());
        self.output.extend_from_slice(&(count as u64).to_le_bytes());
        let table = self.output.len();
        let size = count
            .checked_mul(WORD)
            .expect("the frame is too large to encode");
        self.output.resize(table + size, 0);
        FrameMark { start, table }
    }

    /// Begins the frame of an enum, writing the tag that names its variant.
    /// The variant's fields follow through [`Writer::begin_payload`].
    pub fn begin_variant(&mut self, tag: u32) -> FrameMark {
        let mark = self.begin_frame(VARIANT_SLOTS);
        self.begin_entry(&mark, TAG as usize);
        self.write_u32(tag);
        mark
    }

    /// Begins the frame holding the fields of the variant `mark` was begun for.
    /// A variant without fields leaves it unwritten, and so encodes as its tag
    /// alone.
    pub fn begin_payload(&mut self, mark: &FrameMark, slots: usize) -> FrameMark {
        self.begin_entry(mark, PAYLOAD as usize);
        self.begin_frame(slots)
    }

    /// Records that whatever is written next is the entry at `index`.
    pub fn begin_entry(&mut self, mark: &FrameMark, index: usize) {
        let offset = (self.output.len() - mark.start) as u64;
        let entry = mark.table + index * WORD;
        self.output[entry..entry + WORD].copy_from_slice(&offset.to_le_bytes());
    }

    /// Backfills the length reserved by [`Writer::begin_frame`].
    pub fn end_frame(&mut self, mark: FrameMark) {
        let length = (self.output.len() - mark.start) as u64;
        self.output[mark.start..mark.start + WORD].copy_from_slice(&length.to_le_bytes());
    }

    pub fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }

    pub fn write_bytes(&mut self, value: &[u8]) {
        self.output
            .extend_from_slice(&(value.len() as u64).to_le_bytes());
        self.output.extend_from_slice(value);
    }

    pub fn write_bool(&mut self, value: bool) {
        self.output.push(u8::from(value));
    }

    write_scalars! {
        write_u8: u8,
        write_u16: u16,
        write_u32: u32,
        write_u64: u64,
        write_i8: i8,
        write_i16: i16,
        write_i32: i32,
        write_i64: i64,
        write_f32: f32,
        write_f64: f64,
    }
}

/// A frame's bytes, exactly: its header, offset table and data.
///
/// Reading an entry is an index into the table, so every operation here is a
/// constant number of loads no matter how many entries the frame has.
#[derive(Clone, Copy)]
pub(crate) struct Frame<'buf> {
    bytes: &'buf [u8],
}

impl<'buf> Frame<'buf> {
    /// Reads the frame beginning at the start of `bytes`, checking only that
    /// its header and offset table are present. Entries are checked as they
    /// are read, which is what keeps this constant time.
    pub(crate) fn read(bytes: &'buf [u8]) -> Result<Self, Error> {
        let length = read_word(bytes, 0)?;
        let frame = bytes.get(..length).ok_or(Error::UnexpectedEof)?;
        let count = read_word(frame, WORD)?;
        let table = count
            .checked_mul(WORD)
            .and_then(|size| size.checked_add(HEADER))
            .ok_or(Error::UnexpectedEof)?;
        if table > frame.len() {
            return Err(Error::UnexpectedEof);
        }
        Ok(Self { bytes: frame })
    }

    /// Wraps bytes that are already known to be a frame, because they came
    /// from a decode that read them.
    pub(crate) fn trusted(bytes: &'buf [u8]) -> Self {
        Self { bytes }
    }

    pub(crate) fn bytes(&self) -> &'buf [u8] {
        self.bytes
    }

    pub(crate) fn count(&self) -> usize {
        read_word(self.bytes, WORD).expect("a frame's header was checked when it was read")
    }

    /// The bytes an entry starts at, or `None` when the frame does not carry
    /// that entry.
    pub(crate) fn entry(&self, index: usize) -> Result<Option<&'buf [u8]>, Error> {
        if index >= self.count() {
            return Ok(None);
        }
        let offset = read_word(self.bytes, HEADER + index * WORD)?;
        if offset == 0 {
            return Ok(None);
        }
        if offset < HEADER {
            return Err(Error::UnexpectedEof);
        }
        self.bytes
            .get(offset..)
            .map(Some)
            .ok_or(Error::UnexpectedEof)
    }
}

/// One encoded message, read by slot.
#[derive(Clone, Copy)]
pub struct Message<'buf> {
    frame: Frame<'buf>,
    depth: u32,
    /// Whether nested data is decoded as it is read. Decoding a message
    /// validates it in full, so that accessors on the resulting view cannot
    /// fail; reading through those accessors then skips the work.
    validate: bool,
}

macro_rules! read_scalars {
    ($($name:ident: $ty:ty,)*) => {
        $(
            pub fn $name(&self, slot: u32) -> Result<$ty, Error> {
                let bytes = self.slot(slot)?;
                let value = bytes.get(..size_of::<$ty>()).ok_or(Error::UnexpectedEof)?;
                Ok(<$ty>::from_le_bytes(
                    value.try_into().expect("the slice is exactly sized"),
                ))
            }
        )*
    };
}

impl<'buf> Message<'buf> {
    /// Reads the outermost message of a buffer, which must be the whole of it.
    pub(crate) fn root(bytes: &'buf [u8]) -> Result<Self, Error> {
        let frame = Frame::read(bytes)?;
        if frame.bytes().len() != bytes.len() {
            return Err(Error::TrailingBytes);
        }
        Ok(Self {
            frame,
            depth: 0,
            validate: true,
        })
    }

    /// A message over bytes a previous decode already validated.
    ///
    /// Only generated accessors should call this: reading through a message
    /// that was not validated can panic.
    #[doc(hidden)]
    pub fn trusted(bytes: &'buf [u8]) -> Self {
        Self {
            frame: Frame::trusted(bytes),
            depth: 0,
            validate: false,
        }
    }

    /// The bytes of this message, which is all a view needs to keep.
    pub fn bytes(&self) -> &'buf [u8] {
        self.frame.bytes()
    }

    /// Whether generated code should read this message's fields to check them.
    pub fn validates(&self) -> bool {
        self.validate
    }

    fn slot(&self, slot: u32) -> Result<&'buf [u8], Error> {
        self.frame.entry(slot as usize)?.ok_or(Error::MissingField)
    }

    pub fn read_str(&self, slot: u32) -> Result<&'buf str, Error> {
        str::from_utf8(self.read_bytes(slot)?).map_err(|_| Error::InvalidUtf8)
    }

    pub fn read_bytes(&self, slot: u32) -> Result<&'buf [u8], Error> {
        let bytes = self.slot(slot)?;
        let length = read_word(bytes, 0)?;
        bytes.get(WORD..WORD + length).ok_or(Error::UnexpectedEof)
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
            frame: Frame::read(self.slot(slot)?)?,
            depth: self.depth + 1,
            validate: self.validate,
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
        let list = ListView::new(Frame::read(self.slot(slot)?)?);
        if self.validate {
            list.validate(self.depth + 1)?;
        }
        Ok(list)
    }

    pub(crate) fn element(frame: Frame<'buf>, depth: u32, validate: bool) -> Self {
        Self {
            frame,
            depth,
            validate,
        }
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
