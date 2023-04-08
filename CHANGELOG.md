# Changelog

All notable changes to this project will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org).

## Unreleased

### Added

- A new type `Cursor` which wraps a `BufList` or `&BufList`, and implements `Seek`, `Read` and `BufRead`.
- `BufList` implements `From<T>` for any `T` that can be converted to `Bytes`. This creates a
  `BufList` with a single chunk.
- `BufList::get_chunk` returns the chunk at the provided index.
- New optional features:
  - `tokio1`: makes `Cursor` implement tokio's `AsyncSeek`, `AsyncRead` and `AsyncBufRead`
  - `futures03`: makes `Cursor` implement futures's `AsyncSeek`, `AsyncRead` and `AsyncBufRead`.

## [1.0.1] - 2023-02-16

### Added

- Add recipes for converting a `BufList` into a `Stream` or a `TryStream`.

## [1.0.0] - 2023-01-06

### Added

- `BufList` now implements `Extend<B: Buf>`. This means you can now collect a stream of `Bytes`, or other `Buf` chunks, directly into a `BufList` via [`StreamExt::collect`](https://docs.rs/futures/latest/futures/stream/trait.StreamExt.html#method.collect).
  - Collecting a fallible stream is also possible, via [`TryStreamExt::try_collect`](https://docs.rs/futures/latest/futures/stream/trait.TryStreamExt.html#method.try_collect).

### Changed

- `push_chunk` now has a type parameter `B: Buf` rather than `impl Buf`.

## [0.1.3] - 2022-12-11

- Fix license indication in README: this crate is Apache-2.0 only, not MIT OR Apache-2.0.

## [0.1.2] - 2022-12-10

- Fix intradoc links.

## [0.1.1] - 2022-12-10

- Fixes to README.
- Add MSRV policy.

## [0.1.0] - 2022-12-10

- Initial release.

[1.0.1]: https://github.com/sunshowers-code/buf-list/releases/tag/1.0.1
[1.0.0]: https://github.com/sunshowers-code/buf-list/releases/tag/1.0.0
[0.1.3]: https://github.com/sunshowers-code/buf-list/releases/tag/0.1.3
[0.1.2]: https://github.com/sunshowers-code/buf-list/releases/tag/0.1.2
[0.1.1]: https://github.com/sunshowers-code/buf-list/releases/tag/0.1.1
[0.1.0]: https://github.com/sunshowers-code/buf-list/releases/tag/0.1.0
