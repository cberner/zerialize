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
* `Option<T>`
* `List<T>`

### Value types
Value types are copied, and may be structs or enums, without generic parameters. They must implement
`Copy`.
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

### View schema traits
View traits provide zero copy access to their data. Methods that return views must be guarded with
```
where
    Self: Sized
```
to keep the compiler happy.

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

### View enums
View enums provide zero copy access to view traits behind a generic parameter. All other fields
are copied.
```rust
#[zerializable]
enum Mammal<P: Person> {
    #[variant(0)]
    Cat {
        #[n(0)]
        name: &str,
    },
    #[variant(1)]
    Human(#[n(1)] P),
}
```

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
