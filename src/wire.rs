//! Wire format primitives used by `#[zerializable]`-generated code.
//!
//! The encoding is deliberately simple, and is not stable. Messages, variants
//! and lists are all the same primitive, a frame: a control byte naming the
//! frame's shape, a length, an entry count, and a table of offsets indexed by
//! slot number for a message, or by position for a list.
//!
//! ```text
//! frame  := ctrl: u8, len: uW, count: uW?, body
//! body   := offset[count]: uW, data                      a message or a list
//!         | offset[count]: uW, tag: varint, data         an enum
//!         | stride: u8, element[count]                   a packed list
//! str    := len: varint, utf8[len]
//! bytes  := len: varint, byte[len]
//! scalar := fixed width, little endian
//! char   := u32, a Unicode scalar value
//! ```
//!
//! The control byte holds the width `W` that the length, the count and every
//! offset are stored in, which is the narrowest of one, two, four and eight
//! bytes the frame fits in. A frame that fits in a page therefore addresses its
//! entries a byte at a time, and one that does not pays for what it is. The
//! byte also holds the count itself where it is small enough, which covers a
//! message of fourteen slots or fewer, and the shape of the frame's body.
//!
//! A frame is begun at the narrowest width and widened by whatever outgrows it,
//! which moves the frames nested in it. That is sound because every frame
//! addresses itself: offsets are relative to the start of their own frame, so
//! what a writer appends decodes the same wherever it lands.
//!
//! An enum is a frame carrying the tag that names its variant, whose entries
//! are that variant's fields. The tag is read before them, so the fields of one
//! variant are free to occupy the same slots as the fields of another.
//!
//! A list whose elements are all one width holds them one after another with no
//! table at all, since the offset of an element is its position times that
//! width. That is what keeps a list of numbers the size of the numbers in it.
//!
//! Offsets being relative to their own frame leaves 0 free to mean "absent": no
//! entry can begin inside the header. Indexing the table by slot number is what
//! makes reading a field a constant number of loads rather than a walk over the
//! fields before it, and it is why a decoded view can be nothing more than the
//! bytes of its frame. A reader ignores slots it does not know by never
//! indexing them, so a schema can gain fields without breaking readers built
//! against the older one.
//!
//! An absent entry is also what an optional field encodes `None` as, which is
//! why a slot a writer never knew about and one it deliberately left out read
//! alike: a field added as an optional one is readable in both directions.

use crate::{Element, Error, ListView};

/// The widest a frame's length, count and offsets are stored in.
const WORD: usize = size_of::<u64>();

/// The two bits of the control byte naming the width a frame's length, count
/// and offsets are stored in: the width is one shifted left by what they say,
/// so one, two, four or eight bytes.
const WIDTH_MASK: u8 = 0b11;
const KIND_SHIFT: u32 = 2;
const KIND_MASK: u8 = 0b11;
const COUNT_SHIFT: u32 = 4;

/// The count a control byte holds when the count follows it instead, which is
/// every count the four bits it has cannot name.
const COUNT_FOLLOWS: usize = 15;

/// A frame whose entries are reached through a table of offsets: a message, a
/// value, or a list of anything that is not one fixed width.
const TABLE: u8 = 0;

/// A table frame that also carries the tag naming an enum's variant.
const TAGGED: u8 = 1;

/// A list holding elements of one width one after another, with no table.
const PACKED: u8 = 2;

/// Bytes one frame may occupy.
///
/// A reader keeps a frame's count and table position beside its bytes, so that
/// walking the frame does not read its header again, and this is what lets
/// those be words rather than double words. Data larger than this belongs in a
/// `&[u8]`, whose length is not bounded here.
const MAX_FRAME: usize = u32::MAX as usize;

/// Maximum message nesting accepted when decoding.
///
/// Validation recurses once per level of nesting, so this bounds stack usage
/// on hostile input.
const MAX_DEPTH: u32 = 64;

/// Bytes a LEB128 integer may occupy, which is what a `u64` needs.
const MAX_VARINT: usize = 10;

/// How a list holds an element every one of which is the same size.
#[derive(Clone, Copy)]
pub struct Packing {
    /// Bytes one element occupies.
    pub width: u8,
    /// Whether every byte pattern of that width is an element, which is what
    /// lets a packed list of them be checked by its length alone.
    pub total: bool,
}

/// Reads a number of one of the four widths a frame stores its numbers in.
///
/// Where a whole word is in reach the number is read as one and masked down,
/// which is a load and a shift where copying a width the compiler cannot see
/// would be a call to `memcpy`. The bytes past the number belong to the same
/// buffer, and masking discards them.
#[inline]
fn read_uint(bytes: &[u8], at: usize, width: usize) -> Result<usize, Error> {
    let value = if at <= bytes.len().saturating_sub(WORD) {
        let word = u64::from_le_bytes(
            bytes[at..at + WORD]
                .try_into()
                .expect("the slice is exactly a word"),
        );
        word & (u64::MAX >> (8 * (WORD - width)))
    } else {
        let end = at.checked_add(width).ok_or(Error::UnexpectedEof)?;
        let slice = bytes.get(at..end).ok_or(Error::UnexpectedEof)?;
        let mut word = [0; WORD];
        word[..width].copy_from_slice(slice);
        u64::from_le_bytes(word)
    };
    // A value that does not fit in a usize cannot address anything in the
    // buffer, so it is as out of bounds as a value past its end.
    usize::try_from(value).map_err(|_| Error::UnexpectedEof)
}

fn write_uint(out: &mut [u8], value: usize, width: usize) {
    out[..width].copy_from_slice(&(value as u64).to_le_bytes()[..width]);
}

/// Bytes [`put_varint`] writes `value` in.
fn varint_len(value: u32) -> usize {
    let bits = (u32::BITS - value.leading_zeros()).max(1) as usize;
    bits.div_ceil(7)
}

fn put_varint(out: &mut [u8], mut value: u32) {
    let mut at = 0;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out[at] = byte;
            return;
        }
        out[at] = byte | 0x80;
        at += 1;
    }
}

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Reads a LEB128 integer, and how many bytes it occupied.
fn read_varint(bytes: &[u8], at: usize) -> Result<(u64, usize), Error> {
    let mut value = 0;
    let mut shift = 0;
    for size in 1..=MAX_VARINT {
        let byte = *bytes
            .get(at.checked_add(size - 1).ok_or(Error::UnexpectedEof)?)
            .ok_or(Error::UnexpectedEof)?;
        // The last group a u64 reaches into is the only one that can carry
        // bits past the end of it, and only one of its seven is inside.
        if shift == u64::BITS - 1 && byte & 0x7f > 1 {
            return Err(Error::UnexpectedEof);
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, size));
        }
        shift += 7;
    }
    Err(Error::UnexpectedEof)
}

/// Bytes a frame's header occupies before its offset table: the control byte,
/// the length, the count where the control byte said it follows rather than
/// holding it, and, in a packed frame, the width of one element.
///
/// A reader takes `follows` from the control byte rather than from the count
/// it read, so that a frame claiming a count it then writes small is read the
/// way its header says rather than a byte out of step with it.
fn prefix(width: usize, follows: bool, kind: u8) -> usize {
    1 + width + if follows { width } else { 0 } + usize::from(kind == PACKED)
}

/// Whether a number is small enough to be stored in `width` bytes.
fn fits(value: usize, width: usize) -> bool {
    width == WORD || value >> (8 * width) == 0
}

/// Bytes a frame's whole header occupies, after which its entries begin.
fn header_size(width: usize, count: usize, kind: u8, tag: u32) -> usize {
    prefix(width, count >= COUNT_FOLLOWS, kind)
        + match kind {
            PACKED => 0,
            TAGGED => count * width + varint_len(tag),
            _ => count * width,
        }
}

/// Builds an encoded message at the end of a buffer.
///
/// Every offset a frame holds is relative to the start of that frame, so a
/// message is position independent: what a writer appends decodes the same
/// whether the buffer was empty or already held other messages. That is also
/// what lets a frame be widened as it grows, since widening moves the frames it
/// holds without rewriting them.
pub struct Writer<'out> {
    output: &'out mut Vec<u8>,
}

/// Position of a frame's header, filled in by [`Writer::end_frame`].
///
/// The width the frame is being written at is not held here but in the control
/// byte the frame has already written, so that widening one is invisible to
/// whatever is writing into it.
#[must_use = "the frame is only completed by Writer::end_frame"]
pub struct FrameMark {
    start: usize,
    count: usize,
    kind: u8,
    tag: u32,
    stride: usize,
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

impl<'out> Writer<'out> {
    /// Writes at the end of `output`, leaving what it already holds alone.
    pub fn new(output: &'out mut Vec<u8>) -> Self {
        Self { output }
    }

    /// Begins a frame of `count` entries. Entries left unwritten decode as
    /// absent.
    pub fn begin_frame(&mut self, count: usize) -> FrameMark {
        self.begin(count, TABLE, 0, 0)
    }

    /// Begins the frame of an enum: the tag naming its variant, and that
    /// variant's fields as the frame's own entries.
    pub fn begin_variant(&mut self, tag: u32, count: usize) -> FrameMark {
        self.begin(count, TAGGED, tag, 0)
    }

    /// Begins the frame of a list of `count` elements, packed where every
    /// element of `E` is the same width.
    pub fn begin_list<E: Element + ?Sized>(&mut self, count: usize) -> FrameMark {
        match E::PACKING {
            Some(packing) => self.begin(count, PACKED, 0, usize::from(packing.width)),
            None => self.begin_frame(count),
        }
    }

    fn begin(&mut self, count: usize, kind: u8, tag: u32, stride: usize) -> FrameMark {
        assert!(count <= MAX_FRAME, "the frame is too large to encode");
        let start = self.output.len();
        // A frame begins at the narrowest width, which is where most of them
        // end, and is widened by whatever outgrows it.
        let inline = count.min(COUNT_FOLLOWS) as u8;
        self.output
            .push((kind << KIND_SHIFT) | (inline << COUNT_SHIFT));
        self.output
            .resize(start + header_size(1, count, kind, tag), 0);
        FrameMark {
            start,
            count,
            kind,
            tag,
            stride,
        }
    }

    #[inline]
    fn width(&self, mark: &FrameMark) -> usize {
        1 << (self.output[mark.start] & WIDTH_MASK)
    }

    /// Widens the frame until where it ends can be stored, which is what both
    /// an entry's offset and the frame's own length have to fit in, and hands
    /// back the width it settled at.
    ///
    /// Kept out of line, because a frame wide enough already is every frame
    /// but the few that outgrew a width.
    #[cold]
    fn widen_to_fit(&mut self, mark: &FrameMark) -> usize {
        loop {
            self.widen(mark);
            let width = self.width(mark);
            if fits(self.output.len() - mark.start, width) {
                return width;
            }
        }
    }

    /// Doubles the width `mark`'s frame stores its numbers in, moving what it
    /// holds along and rewriting the offsets recorded so far.
    fn widen(&mut self, mark: &FrameMark) {
        let width = self.width(mark);
        let wider = width * 2;
        let header = header_size(width, mark.count, mark.kind, mark.tag);
        let shift = header_size(wider, mark.count, mark.kind, mark.tag) - header;

        let from = mark.start + header;
        let data = self.output.len() - from;
        self.output.resize(self.output.len() + shift, 0);
        self.output.copy_within(from..from + data, from + shift);

        if mark.kind != PACKED {
            let follows = mark.count >= COUNT_FOLLOWS;
            let table = mark.start + prefix(width, follows, mark.kind);
            let wide = mark.start + prefix(wider, follows, mark.kind);
            // No entry moves left, so the table is rewritten from its end: a
            // wider entry would otherwise cover a narrow one still unread.
            for index in (0..mark.count).rev() {
                let offset = read_uint(self.output, table + index * width, width)
                    .expect("the table is inside the header");
                let offset = if offset == 0 { 0 } else { offset + shift };
                write_uint(&mut self.output[wide + index * wider..], offset, wider);
            }
        }

        let ctrl = self.output[mark.start] & !WIDTH_MASK;
        self.output[mark.start] = ctrl | wider.trailing_zeros() as u8;
    }

    /// Records that whatever is written next is the entry at `index`. A packed
    /// frame needs no record: an element is where its position puts it.
    #[inline]
    pub fn begin_entry(&mut self, mark: &FrameMark, index: usize) {
        if mark.kind == PACKED {
            return;
        }
        let mut width = self.width(mark);
        if !fits(self.output.len() - mark.start, width) {
            width = self.widen_to_fit(mark);
        }
        let at = self.output.len() - mark.start;
        let table = mark.start + prefix(width, mark.count >= COUNT_FOLLOWS, mark.kind);
        write_uint(&mut self.output[table + index * width..], at, width);
    }

    /// Writes what only the finished frame knows: its length, and the tag or
    /// element width its shape calls for.
    pub fn end_frame(&mut self, mark: FrameMark) {
        let mut width = self.width(&mark);
        if !fits(self.output.len() - mark.start, width) {
            width = self.widen_to_fit(&mark);
        }
        let length = self.output.len() - mark.start;
        assert!(length <= MAX_FRAME, "the frame is too large to encode");

        let mut at = mark.start + 1;
        write_uint(&mut self.output[at..], length, width);
        at += width;
        if mark.count >= COUNT_FOLLOWS {
            write_uint(&mut self.output[at..], mark.count, width);
            at += width;
        }
        match mark.kind {
            PACKED => self.output[at] = mark.stride as u8,
            TAGGED => put_varint(&mut self.output[at + mark.count * width..], mark.tag),
            _ => {}
        }
    }

    pub fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }

    pub fn write_bytes(&mut self, value: &[u8]) {
        push_varint(self.output, value.len() as u64);
        self.output.extend_from_slice(value);
    }

    pub fn write_bool(&mut self, value: bool) {
        self.output.push(u8::from(value));
    }

    /// A `char` is written as the Unicode scalar value it is, which is what
    /// makes reading one a range check rather than a decode.
    pub fn write_char(&mut self, value: char) {
        self.write_u32(value as u32);
    }

    /// Writes the number naming the variant of a value enum that carries no
    /// fields, which is all such an enum is on the wire.
    pub fn write_number(&mut self, value: u32) {
        push_varint(self.output, u64::from(value));
    }

    write_scalars! {
        write_u8: u8,
        write_u16: u16,
        write_u32: u32,
        write_u64: u64,
        write_u128: u128,
        write_i8: i8,
        write_i16: i16,
        write_i32: i32,
        write_i64: i64,
        write_i128: i128,
        write_f32: f32,
        write_f64: f64,
    }
}

/// A frame's bytes, exactly, beside what reading its header said about them.
///
/// The header is kept rather than read back, because a frame is walked far more
/// often than it is opened: reaching an entry is then an index into the table,
/// a constant number of loads no matter how many entries the frame has.
#[derive(Clone, Copy)]
pub(crate) struct Frame<'buf> {
    bytes: &'buf [u8],
    /// Where the offset table begins, or, in a packed frame, the elements.
    table: u32,
    count: u32,
    /// Where the entries begin, which is the least an offset may be: one that
    /// began inside the header could not be told from an absent one, whose
    /// offset is zero. In a tagged frame this is past the tag as well.
    data: u32,
    /// Bytes one offset occupies, or one element of a packed frame.
    step: u8,
    kind: u8,
}

/// Reads the header of the frame beginning at the start of `bytes`, checking
/// that the header and everything it addresses are within the length the frame
/// claims.
fn parse(bytes: &[u8]) -> Result<Frame<'_>, Error> {
    let ctrl = *bytes.first().ok_or(Error::UnexpectedEof)?;
    let width = 1 << (ctrl & WIDTH_MASK);
    let kind = (ctrl >> KIND_SHIFT) & KIND_MASK;
    let length = read_uint(bytes, 1, width)?;
    // Everything after the length is read out of the frame itself, so that a
    // frame claiming less than it holds cannot reach past what it claims.
    let frame = bytes.get(..length).ok_or(Error::UnexpectedEof)?;
    if kind == KIND_MASK || length > MAX_FRAME {
        return Err(Error::InvalidFrame);
    }

    let inline = usize::from(ctrl >> COUNT_SHIFT);
    let follows = inline == COUNT_FOLLOWS;
    let count = if follows {
        read_uint(frame, 1 + width, width)?
    } else {
        inline
    };
    let table = prefix(width, follows, kind);
    let step = if kind == PACKED {
        usize::from(*frame.get(table - 1).ok_or(Error::UnexpectedEof)?)
    } else {
        width
    };
    let entries = count
        .checked_mul(step)
        .and_then(|span| span.checked_add(table))
        .ok_or(Error::UnexpectedEof)?;
    if entries > length {
        return Err(Error::InvalidFrame);
    }
    // The tag of an enum sits between the table and the entries, so a frame
    // that does not hold one is as truncated as one missing an entry, and the
    // entries begin after it rather than at it. Reading the tag is what bounds
    // it: one that ran past the frame is one that could not be read.
    let data = if kind == TAGGED {
        let (_, size) = read_varint(frame, entries)?;
        entries + size
    } else {
        entries
    };

    Ok(Frame {
        bytes: frame,
        table: table as u32,
        count: count as u32,
        data: data as u32,
        step: step as u8,
        kind,
    })
}

impl<'buf> Frame<'buf> {
    /// Reads the frame beginning at the start of `bytes`, checking only its
    /// header. Entries are checked as they are read, which is what keeps this
    /// constant time.
    pub(crate) fn read(bytes: &'buf [u8]) -> Result<Self, Error> {
        parse(bytes)
    }

    /// Wraps bytes that are already known to be a frame, because they came
    /// from a decode that read them.
    pub(crate) fn trusted(bytes: &'buf [u8]) -> Self {
        parse(bytes).expect("a frame's header was checked when it was read")
    }

    pub(crate) fn bytes(&self) -> &'buf [u8] {
        self.bytes
    }

    pub(crate) fn count(&self) -> usize {
        self.count as usize
    }

    pub(crate) fn kind(&self) -> u8 {
        self.kind
    }

    /// Bytes one element of a packed frame occupies.
    pub(crate) fn stride(&self) -> usize {
        usize::from(self.step)
    }

    /// The elements of a packed frame, one after another, which is where they
    /// are rather than where a table would have said they are.
    pub(crate) fn elements(&self) -> &'buf [u8] {
        &self.bytes[self.table as usize..]
    }

    /// Where the tag of a tagged frame sits, which is between the table and
    /// the entries.
    fn tag_at(&self) -> usize {
        self.table as usize + self.count as usize * usize::from(self.step)
    }

    /// The tag naming an enum's variant, which sits between the table and the
    /// entries.
    pub(crate) fn tag(&self) -> Result<u32, Error> {
        let (tag, _) = read_varint(self.bytes, self.tag_at())?;
        u32::try_from(tag).map_err(|_| Error::InvalidFrame)
    }

    /// The bytes an entry starts at, or `None` when the frame does not carry
    /// that entry.
    ///
    /// Only a frame that reaches its entries through a table is asked this:
    /// every way of reaching a frame checks its shape first, and a packed list
    /// reads an element out of [`Frame::elements`] by its position instead.
    #[inline]
    pub(crate) fn entry(&self, index: usize) -> Result<Option<&'buf [u8]>, Error> {
        if index >= self.count as usize {
            return Ok(None);
        }
        let step = usize::from(self.step);
        let at = self.table as usize + index * step;
        let offset = read_uint(self.bytes, at, step)?;
        if offset == 0 {
            return Ok(None);
        }
        if offset < self.data as usize {
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
        let (length, size) = read_varint(bytes, 0)?;
        // A length that cannot be added to is as out of bounds as one past the
        // end of the buffer: nothing that long was ever written.
        let length = usize::try_from(length).map_err(|_| Error::UnexpectedEof)?;
        let end = size.checked_add(length).ok_or(Error::UnexpectedEof)?;
        bytes.get(size..end).ok_or(Error::UnexpectedEof)
    }

    pub fn read_bool(&self, slot: u32) -> Result<bool, Error> {
        match self.read_u8(slot)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::InvalidBool),
        }
    }

    pub fn read_char(&self, slot: u32) -> Result<char, Error> {
        char::from_u32(self.read_u32(slot)?).ok_or(Error::InvalidChar)
    }

    /// Reads the number naming the variant of a value enum that carries no
    /// fields.
    pub fn read_number(&self, slot: u32) -> Result<u32, Error> {
        let (value, _) = read_varint(self.slot(slot)?, 0)?;
        u32::try_from(value).map_err(|_| Error::UnknownVariant)
    }

    /// Whether `slot` carries an entry, which is what tells `Some` from `None`.
    ///
    /// An optional field encodes `None` by leaving its slot unwritten, so this
    /// is also false for a slot the writer did not have at all: a field added
    /// as an optional one reads as absent rather than as missing.
    pub fn is_present(&self, slot: u32) -> Result<bool, Error> {
        Ok(self.frame.entry(slot as usize)?.is_some())
    }

    /// Checks that this frame is a message rather than an enum, which is what
    /// a schema decoding the outermost frame of a buffer asks of it. Every
    /// other frame is checked as it is reached.
    pub fn expect_message(&self) -> Result<(), Error> {
        if self.frame.kind() != TABLE {
            return Err(Error::InvalidFrame);
        }
        Ok(())
    }

    /// Reads the tag of an encoded enum, naming the variant it holds.
    pub fn read_tag(&self) -> Result<u32, Error> {
        if self.frame.kind() != TAGGED {
            return Err(Error::InvalidFrame);
        }
        self.frame.tag()
    }

    /// Reads a nested message, which inherits this one's validation mode.
    pub fn read_message(&self, slot: u32) -> Result<Message<'buf>, Error> {
        self.read_nested(slot, TABLE)
    }

    /// Reads a nested enum, whose entries are the fields of the variant its
    /// tag names.
    ///
    /// A variant that carries nothing is a frame of no entries rather than a
    /// missing one, so a variant that gained its first field reads that field
    /// the way a message that gained one does: absent rather than missing.
    pub fn read_variant(&self, slot: u32) -> Result<Message<'buf>, Error> {
        self.read_nested(slot, TAGGED)
    }

    fn read_nested(&self, slot: u32, kind: u8) -> Result<Message<'buf>, Error> {
        if self.depth == MAX_DEPTH {
            return Err(Error::RecursionLimit);
        }
        let frame = Frame::read(self.slot(slot)?)?;
        if frame.kind() != kind {
            return Err(Error::InvalidFrame);
        }
        Ok(Self {
            frame,
            depth: self.depth + 1,
            validate: self.validate,
        })
    }

    /// Reads a list. When this message is being validated, every element is
    /// decoded here, so that reading one later cannot fail.
    ///
    /// A list is validated at this message's own depth: what it holds is one
    /// level deeper than this message, which is the level an element that is
    /// itself a message is read at.
    pub fn read_list<E: Element + ?Sized>(&self, slot: u32) -> Result<ListView<'buf, E>, Error> {
        if self.depth == MAX_DEPTH {
            return Err(Error::RecursionLimit);
        }
        let frame = Frame::read(self.slot(slot)?)?;
        // A packed list is checked by its shape: reading the header already
        // said that every element is within the frame, so where any bytes of
        // that width are an element there is nothing left to read.
        let (kind, checked) = match E::PACKING {
            Some(packing) => {
                if frame.stride() != usize::from(packing.width) {
                    return Err(Error::InvalidFrame);
                }
                (PACKED, packing.total)
            }
            None => (TABLE, false),
        };
        if frame.kind() != kind {
            return Err(Error::InvalidFrame);
        }
        let list = ListView::new(frame);
        if self.validate && !checked {
            list.validate(self.depth)?;
        }
        Ok(list)
    }

    /// A message over a frame reached from another one, at the depth that frame
    /// sits at and validated as that one is.
    pub(crate) fn nested(frame: Frame<'buf>, depth: u32, validate: bool) -> Self {
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
        read_u128: u128,
        read_i8: i8,
        read_i16: i16,
        read_i32: i32,
        read_i64: i64,
        read_i128: i128,
        read_f32: f32,
        read_f64: f64,
    }
}
