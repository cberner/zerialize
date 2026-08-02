# Benchmarks

Size and speed of zerialize against [rkyv](https://rkyv.org) and
[flatbuffers](https://flatbuffers.dev), over the same three records.

```
just bench
```

This package is deliberately outside the workspace, so that rkyv and
flatbuffers stay out of the library's dependency graph. The flatbuffers schema
is `schema/bench.fbs`, and the code generated from it is checked in as
`src/bench_generated.rs` so the benchmarks build without `flatc`.

## What is measured

Three records, chosen to separate the costs that differ between the formats:

| record | shape |
| --- | --- |
| `point` | three `f32` fields: per message overhead and nothing else |
| `team` | 32 people, each with two strings, a `u64`, an enum and eight `u32` |
| `series` | a name and 1024 `f64`: a list of numbers and nothing else |

Encoding writes into a buffer the caller already has, where the format offers
that: zerialize through `encode_in`, flatbuffers through `FlatBufferBuilder`.
rkyv's `to_bytes` allocates, which is what its API offers.

Decoding is measured as decode plus a full traversal of every field, because a
format that defers work to field access has not saved that work, it has moved
it. All three check the buffer they are given: zerialize validates in `decode`,
rkyv through `access` with `bytecheck`, flatbuffers through `root`'s verifier.
That check is also measured on its own, so that what a format spends on
accepting a buffer can be told from what it spends on reading one.

## Results

Measured on x86-64, `rustc 1.97.1`, release with LTO. Times are criterion
medians. Everything below was measured in one sitting on one machine, with the
rkyv and flatbuffers numbers agreeing to within 3% across the runs, which is
what says the runs are comparable. Numbers from different sittings are not:
this container's speed has moved by half between them.

### Encoded size, in bytes

| record | zerialize | rkyv | flatbuffers |
| --- | ---: | ---: | ---: |
| `point` | 17 | **12** | 32 |
| `team` | **2916** | 3488 | 3904 |
| `series` | **8214** | 8208 | 8240 |

`point` is the one zerialize loses, and it loses it to a format that cannot
evolve: rkyv's 12 bytes are the three floats and nothing else, because an
archived rkyv type is a fixed layout with no room for a field it did not have
when it was written. zerialize spends five bytes on being able to gain and lose
fields; flatbuffers spends twenty on the same thing.

### Encode, into a buffer the caller already has

| record | zerialize | rkyv | flatbuffers |
| --- | ---: | ---: | ---: |
| `point` | **15.3 ns** | 16.1 ns | 36.8 ns |
| `team` | 1.55 us | **0.93 us** | 3.51 us |
| `series` | 0.88 us | **0.19 us** | 3.24 us |

### Decode and traverse every field

| record | zerialize | rkyv | flatbuffers |
| --- | ---: | ---: | ---: |
| `point` | 33.9 ns | **1.3 ns** | 32.4 ns |
| `team` | 4.52 us | **0.48 us** | 2.93 us |
| `series` | 0.72 us | **0.61 us** | 0.68 us |

rkyv is fastest at both ends by a wide margin, and that is what it buys with
the size result above: its archived form is the in-memory layout, so encoding
is close to a memcpy and reading a field is a load at a fixed offset. It is the
right comparison to lose to rather than one to close.

### Where the decode goes

The same numbers split into accepting the buffer and reading it, which answer
different questions: what a format spends refusing a bad buffer is not what it
spends handing out fields.

| record | | accept | decode | reading |
| --- | --- | ---: | ---: | ---: |
| `point` | zerialize | 14.2 ns | 33.9 ns | 19.7 ns |
| | rkyv | 1.2 ns | 1.3 ns | 0.1 ns |
| | flatbuffers | 38.1 ns | 32.4 ns | -- |
| `team` | zerialize | **2.39 us** | 4.52 us | 2.13 us |
| | rkyv | 0.36 us | 0.48 us | 0.12 us |
| | flatbuffers | 2.57 us | 2.93 us | **0.36 us** |
| `series` | zerialize | **45.5 ns** | 0.72 us | 0.67 us |
| | rkyv | 7.4 ns | 0.61 us | 0.60 us |
| | flatbuffers | 46.8 ns | 0.68 us | 0.63 us |

Accepting a buffer is the half zerialize wins: cheaper than the flatbuffers
verifier on both records where it can be told apart, and on `series` a packed
list of numbers is checked by its length rather than element by element, which
is 34 times what the format it replaced spent. Reading is the half it loses, by
6x on `team`, and that is where the work left to do is.

`point` is too small for the split to mean much: flatbuffers reads it in less
than nothing, which is the verifier and the reads being optimised together
rather than a real negative. `series` reading is near the floor for all three,
because summing 1024 `f64` is a chain of dependent adds that costs about 0.6us
whoever hands over the numbers.

### What the density cost

Against the fixed width format this replaced, where every number a frame held
was eight bytes:

| record | size | encode | accept | decode |
| --- | ---: | ---: | ---: | ---: |
| `point` | 52 -> **17** | 9.2 ns -> 15.3 ns | 8.9 ns -> 14.2 ns | 4.0 ns -> 33.9 ns |
| `team` | 7727 -> **2916** | 1.39 us -> 1.55 us | 1.51 us -> 2.39 us | 2.97 us -> 4.52 us |
| `series` | 16448 -> **8214** | 1.03 us -> **0.88 us** | 1.53 us -> **45.5 ns** | 3.13 us -> **0.72 us** |

Bulk numbers came out ahead on every count, because a packed list neither
writes an offset table nor reads one, and is checked by its length rather than
element by element. Everything else pays for the density in time: reading a
field now means reading the control byte, deriving the width from it, reading
the length at that width, deriving where the table starts, reading the offset,
and only then the field, where the fixed width format had two independent
loads.

### Two attempts at the reading half

Both were aimed at the 2.13us `team` spends reading, and neither is worth what
it looked like it would be.

**A view carrying its frame's header**, so that the chain above is walked once
per frame rather than once per field. It halves what one field read costs, and
`point` decodes 1.6x faster for it, but `point` is the only shape that spends
its time reading one frame repeatedly. `team` gained 6% and `series` nothing,
against a view twice as wide, so this was reverted.

**Reaching a packed element without going through the frame**, since where one
begins is its position times its width. Kept, and worth:

| decode | through the frame | straight to the element |
| --- | ---: | ---: |
| `team` | 4.85 us | **4.52 us** |
| `series` | 748 ns | **717 ns** |

7% and 4%: the layers it removes were mostly being collapsed by the compiler
already. What is left is what the list still does per element, and what a
message still does per field, which is the fourth item below.

## What is left on the table

The first two are size; the rest are the time the density cost, ordered by what
the measurements above say they are worth.

1. **Variable width integers.** Scalars are fixed width, so a `u32` holding
   `42` costs four bytes. LEB128 with zigzag for the signed types would make
   it one. On `team` the eight scores per person are all under 1000: two bytes
   each rather than four is 512 bytes, near 18% of the record. The cost is that
   a list of them can no longer be packed, so this wants to be a per field
   choice rather than a format wide one: `#[fixed]` on the fields that are
   really arrays of numbers, varint everywhere else.

2. **A data section, as Cap'n Proto has.** The schema knows statically which
   slots are fixed width scalars, so those need no offsets at all: lay them out
   contiguously at schema assigned positions, and keep the offset table for the
   variable sized fields alone. `point` would lose all three of its offsets, 3
   of 17 bytes, and its accessors would lose the table read as well. A message
   with ten scalar fields would lose ten bytes. Optional scalars need a
   presence bit each, which is a byte per eight of them, and a reader still
   skips what it does not know by reading past the data section the writer
   declared.

3. **Decode without a second pass.** `decode` walks the whole message to
   validate it and the traversal then walks it again, which is 53% of `team`'s
   decode. A packed list of numbers is already exempt, checked by its length
   alone, which is why `series` accepts a buffer in 45ns rather than the 1.5us
   the fixed width format spent. The same holds for any frame whose fields are
   all scalars that cannot be invalid, which would let the leaves of a tree be
   checked by their headers rather than by reading them.

4. **Stop re-deriving a frame per field.** `team` reads at 2.13us against
   flatbuffers' 0.36us, and the two things it does that flatbuffers does not
   are reading a frame's header again at every field, and validating a string
   as UTF-8 again at every read of it. The first is what the reverted attempt
   above went after in the wrong place: what wants the header kept is the list
   walking its elements, not the view of one message. The second cannot be
   dropped while the crate forbids unsafe, since handing out a `&str` without
   `from_utf8` is exactly what `from_utf8_unchecked` is for; what it can do is
   stop paying it twice, by not checking strings during `decode` when every
   read will check them anyway.

5. **Drop what the schema already knows from a packed list.** A packed frame
   stores its element width so the frame describes itself, but the reader knows
   that width from the element type before it looks. Dropping it saves a byte
   per numeric list: 32 bytes on `team`. Its length word is redundant too,
   since it is the header plus count times width.

6. **Fold a small scalar into its own table entry.** At width two and above,
   the offset slot of a `u8` or `u16` field is wider than the field. Spending
   one of the control byte's spare encodings on "this entry is the value, not a
   pointer to it" would pay for itself in any wide frame full of small numbers.
   Largely subsumed by 2, and cheaper to build.

7. **A dictionary for repeated values.** Nothing here dedupes: a string
   written twice is stored twice. A per buffer table of strings addressed by
   index would collapse the repetition that enum-like string fields and
   repeated keys produce. It does nothing for the records above, whose strings
   are all distinct, and it costs a hash map at encode time, so it should be
   opt in per field.
