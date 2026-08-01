# zerialize

![CI](https://github.com/cberner/zerialize/actions/workflows/ci.yml/badge.svg)
[![Crates.io](https://img.shields.io/crates/v/zerialize.svg)](https://crates.io/crates/zerialize)
[![Documentation](https://docs.rs/zerialize/badge.svg)](https://docs.rs/zerialize)
[![License](https://img.shields.io/crates/l/zerialize)](https://crates.io/crates/zerialize)
[![dependency status](https://deps.rs/repo/github/cberner/zerialize/status.svg)](https://deps.rs/repo/github/cberner/zerialize)

Zerialize provides zero-cost deserialization and a natural interface that allows code to be written
against the traits, structs, and enums in the codebase, without referencing any generated types.

## Supported types
### Primitives
* `bool`
* `char`
* `&str`
* `&[u8]`
* `u8`, `u16`, `u32`, `u64`, `u128`
* `i8`, `i16`, `i32`, `i64`, `i128`
* `f32`, `f64`

### Container types
* `Option<T>`, over any other supported type
* `List<T>`, over any other supported type

An optional field is absent on the wire when it is `None`, which is exactly what a reader sees of
a field the writer did not have, so a field added as an optional one can be read out of messages
written before it existed.

Containers may contain any primitive, value type, view trait, or view enum. An `Option` may contain
a list; `Option<Option<T>>`, a list of `Option<T>`, and a list of lists are not supported.

### Value types
Value types are copied, and may be structs or enums, without generic parameters. They must implement
`Copy`, and hold nothing borrowed: a `&str` field belongs to a view schema trait or a view enum.
A value enum's variants may carry fields of their own, each declaring its slot as a struct's fields
do.
For example:
```rust
#[derive(Zerializable, Copy, Clone)]
struct Date {
    #[n(0)]
    day: u8,
    #[n(1)]
    month: u8,
    #[n(2)]
    year: u16,
}
```

```rust
#[derive(Zerializable, Copy, Clone)]
enum Protocol {
    #[variant(0)]
    TCP {
        #[n(0)]
        port: u32,
    },
    #[variant(1)]
    UDP {
        #[n(0)]
        port: u32
    },
}
```

Value types may only contain primitives that are not borrowed, other value types, and an `Option`
of either. They may not contain `&str`, `&[u8]`, lists, view traits, or view enums.

### View schema traits
View traits provide zero copy access to their data. A method that returns an `impl Trait` must be
guarded with
```
where
    Self: Sized
```
which is what keeps `dyn Person`, the name of the schema, usable as one. A nested view trait, a view
enum carrying one, and a list are each written as an `impl Trait`; a primitive or a value is not,
and needs no guard.

For example:
```rust
#[zerializable]
trait Person {
    #[n(0)]
    fn name(&self) -> &str;

    #[n(1)]
    fn children(&self) -> impl List<Item = impl Person + '_> + '_
    where
        Self: Sized;
}
```

View schema traits may contain any primitive, container type, value type, view trait, or view enum.

### View enums
View enums provide zero copy access to view traits behind a generic parameter. Their other fields
are copied, except `&str` and `&[u8]`, which point into the buffer as a view trait's do.
```rust
#[zerializable]
enum Mammal<'a, P: Person> {
    #[variant(0)]
    Cat {
        #[n(0)]
        name: &'a str,
    },
    #[variant(1)]
    Human(#[n(1)] P),
}
```
A variant's fields are its own, so two variants may declare the same slots, and either may declare
one the other does not.

An enum with a borrowed field declares the lifetime it points into, and is named with it wherever it
is named: `Mammal<'_, dyn Person>` is the schema, and decoding it gives
`Mammal<'buf, PersonView<'buf>>`. An enum that borrows nothing declares no lifetime, and is named
without one.

A list is a generic parameter as a nested view trait is, bound by what it holds: an element named
outright as `List<Item = u32>`, or, where the list holds messages, the trait declaring them as
`List<Item: Person>`.
```rust
#[zerializable]
enum Recipients<'a, P: List<Item: Person>, N: List<Item = &'a str>> {
    #[variant(0)]
    Everyone,
    #[variant(1)]
    People(#[n(0)] P),
    #[variant(2)]
    Addresses(#[n(0)] N),
}
```
A list is named with `ListView` wherever the enum is named:
`Recipients<'_, ListView<'_, dyn Person>, ListView<'_, str>>` is the schema, and decoding it gives
`Recipients<'buf, ListView<'buf, dyn Person>, ListView<'buf, str>>`.

A nested view enum is written over the enum's own parameters: an enum with a `P: Person` parameter
carries the one above as `Pet(#[n(0)] Mammal<'a, P>)`.

View enums may contain any primitive, container type, value type, view trait, or view enum, with one
exception: a list a view enum carries may not hold a view enum. A view schema trait's list may.

## License

Licensed under either of

* [Apache License, Version 2.0](LICENSE-APACHE)
* [MIT License](LICENSE-MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
