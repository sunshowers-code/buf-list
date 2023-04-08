// Copyright (c) 2018 the linkerd2-proxy authors
// Copyright (c) The buf-list Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! A segmented list of [`bytes::Bytes`] chunks.
//!
//! # Overview
//!
//! This crate provides a [`BufList`] type that is a list of [`Bytes`] chunks.
//! The type implements [`bytes::Buf`], so it can be used in any APIs that use `Buf`.
//!
//! The main use case for [`BufList`] is to buffer data received as a stream of chunks without
//! having to copy them into a single contiguous chunk of memory. The [`BufList`] can then be passed
//! into any APIs that accept `Buf`.
//!
//! If you've ever wanted a `Vec<Bytes>` or a `VecDeque<Bytes>`, this type is for you.
//!
//! # Examples
//!
//! Gather chunks into a `BufList`, then write them all out to standard error in one go:
//!
//! ```
//! use buf_list::BufList;
//! use tokio::io::AsyncWriteExt;
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), std::io::Error> {
//! let mut buf_list = BufList::new();
//! buf_list.push_chunk(&b"hello "[..]);
//! buf_list.push_chunk(&b"world"[..]);
//! buf_list.push_chunk(&b"!"[..]);
//!
//! let mut stderr = tokio::io::stderr();
//! stderr.write_all_buf(&mut buf_list).await?;
//! # Ok(()) }
//! ```
//!
//! Collect a fallible stream of `Bytes` into a `BufList`:
//!
//! ```
//! use buf_list::BufList;
//! use bytes::Bytes;
//! use futures::TryStreamExt;
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), ()> {
//! // A common example is a stream of bytes read over HTTP.
//! let stream = futures::stream::iter(
//!     vec![
//!         Ok(Bytes::from_static(&b"laputa, "[..])),
//!         Ok(Bytes::from_static(&b"castle "[..])),
//!         Ok(Bytes::from_static(&b"in the sky"[..]))
//!     ],
//! );
//!
//! let buf_list = stream.try_collect::<BufList>().await?;
//! assert_eq!(buf_list.num_chunks(), 3);
//! # Ok(()) }
//! ```
//!
//! # Converting to `Stream`s
//!
//! A `BufList` can be converted into a `futures::Stream`, or a `TryStream`, of `Bytes` chunks. Use
//! this recipe to do so:
//!
//! (This will be exposed as an API on `BufList` once `Stream` and/or `TryStream` become part of
//! stable Rust.)
//!
//! ```rust
//! use buf_list::BufList;
//! use bytes::Bytes;
//! use futures::{Stream, TryStream};
//!
//! fn into_stream(buf_list: BufList) -> impl Stream<Item = Bytes> {
//!     futures::stream::iter(buf_list)
//! }
//!
//! fn into_try_stream<E>(buf_list: BufList) -> impl TryStream<Ok = Bytes, Error = E> {
//!     futures::stream::iter(buf_list.into_iter().map(Ok))
//! }
//! ```
//!
//! # Minimum supported Rust version
//!
//! The minimum supported Rust version (MSRV) is **1.39**, same as the `bytes` crate.
//!
//! The MSRV is not expected to change in the future. If it does, it will be done as a breaking
//! change.

mod imp;

pub use imp::*;
