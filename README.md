# buf-list

[![buf-list on crates.io](https://img.shields.io/crates/v/buf-list)](https://crates.io/crates/buf-list) [![Documentation (latest release)](https://docs.rs/buf-list/badge.svg)](https://docs.rs/buf-list/) [![Documentation (main)](https://img.shields.io/badge/docs-main-brightgreen)](https://sunshowers-code.github.io/buf-list/rustdoc/buf_list/) [![License](https://img.shields.io/badge/license-Apache-green.svg)](LICENSE)

A list of `bytes::Bytes` chunks.

## Overview

This crate provides a `BufList` type that is a list of `Bytes` chunks.
The type implements `bytes::Buf`, so it can be used in any APIs that use `Buf`.

The main use case for `BufList` is to buffer data received as a stream of chunks without
having to copy them into a single contiguous chunk of memory. The `BufList` can then be passed
into any APIs that accept `Buf`.

If you've ever wanted a `Vec<Bytes>` or a `VecDeque<Bytes>`, this type is for you.

## Examples

Gather chunks into a `BufList`, then write them all out to standard error in one go:

```rust
use buf_list::BufList;
use tokio::io::AsyncWriteExt;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut buf_list = BufList::new();
    buf_list.push_chunk(&b"hello"[..]);
    buf_list.push_chunk(&b"world"[..]);
    buf_list.push_chunk(&b"!"[..]);

    let mut stderr = tokio::io::stderr();
    stderr.write_all_buf(&mut buf_list).await.unwrap();
}
```

## Minimum supported Rust version

The minimum supported Rust version (MSRV) is **1.39**, same as the `bytes` crate.

The MSRV is not expected to change in the future. If it does, it will be done as a breaking
change.

## Contributing

Pull requests are welcome! Please follow the
[code of conduct](https://github.com/sunshowers-code/.github/blob/main/CODE_OF_CONDUCT.md).

## License

buf-list is copyright 2022 The buf-list Contributors. All rights reserved.

Copied and adapted from linkerd2-proxy; [original
code](https://github.com/linkerd/linkerd2-proxy/blob/d36e3a75ef428453945eedaa230a32982c17d30d/linkerd/http-retry/src/replay.rs#L421-L492)
written by Eliza Weisman. linkerd2-proxy is copyright 2018 the linkerd2-proxy authors. All rights
reserved.

Licensed under the Apache License, Version 2.0 (the "License"); you may not use
these files except in compliance with the License. You may obtain a copy of the
License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software distributed
under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
CONDITIONS OF ANY KIND, either express or implied. See the License for the
specific language governing permissions and limitations under the License.
