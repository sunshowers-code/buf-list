// Copyright (c) 2018 the linkerd2-proxy authors
// Copyright (c) The buf-list Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! A list of [`bytes::Bytes`] chunks.
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
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() {
//!     let mut buf_list = BufList::new();
//!     buf_list.push_chunk(&b"hello"[..]);
//!     buf_list.push_chunk(&b"world"[..]);
//!     buf_list.push_chunk(&b"!"[..]);
//!
//!     let mut stderr = tokio::io::stderr();
//!     stderr.write_all_buf(&mut buf_list).await.unwrap();
//! }
//! ```
//!
//! # Minimum supported Rust version
//!
//! The minimum supported Rust version (MSRV) is **1.39**, same as the `bytes` crate.
//!
//! The MSRV is not expected to change in the future. If it does, it will be done as a breaking
//! change.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::{
    collections::VecDeque,
    io::IoSlice,
    iter::{FromIterator, FusedIterator},
};

/// Data composed of a list of [`Bytes`] chunks.
///
/// For more, see the [crate documentation](crate).
#[derive(Clone, Debug, Default)]
pub struct BufList {
    // Invariant: none of the bufs in this queue are zero-length.
    bufs: VecDeque<Bytes>,
}

impl BufList {
    /// Creates a new, empty, `BufList`.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the total number of chunks in this `BufList`.
    ///
    /// # Examples
    ///
    /// ```
    /// use buf_list::BufList;
    ///
    /// let buf_list = vec![&b"hello"[..], &b"world"[..]].into_iter().collect::<BufList>();
    /// assert_eq!(buf_list.num_chunks(), 2);
    /// ```
    #[inline]
    pub fn num_chunks(&self) -> usize {
        self.bufs.len()
    }

    /// Returns the total number of bytes across all chunks.
    ///
    /// # Examples
    ///
    /// ```
    /// use buf_list::BufList;
    ///
    /// let buf_list = vec![&b"hello"[..], &b"world"[..]].into_iter().collect::<BufList>();
    /// assert_eq!(buf_list.num_bytes(), 10);
    /// ```
    #[inline]
    pub fn num_bytes(&self) -> usize {
        self.remaining()
    }

    /// Iterates over the chunks in this list.
    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            iter: self.bufs.iter(),
        }
    }

    /// Adds a new chunk to this list.
    ///
    /// If the provided [`Buf`] is zero-length, it will not be added to the list.
    ///
    /// # Examples
    ///
    /// ```
    /// use buf_list::BufList;
    /// use bytes::{Buf, Bytes};
    ///
    /// let mut buf_list = BufList::new();
    ///
    /// // &'static [u8] implements Buf.
    /// buf_list.push_chunk(&b"hello"[..]);
    /// assert_eq!(buf_list.chunk(), &b"hello"[..]);
    ///
    /// // Bytes also implements Buf.
    /// buf_list.push_chunk(Bytes::from_static(&b"world"[..]));
    /// assert_eq!(buf_list.num_chunks(), 2);
    ///
    /// // A zero-length `Buf` will not be added to the list.
    /// buf_list.push_chunk(Bytes::new());
    /// assert_eq!(buf_list.num_chunks(), 2);
    /// ```
    pub fn push_chunk(&mut self, mut data: impl Buf) -> Bytes {
        let len = data.remaining();
        // `data` is (almost) certainly a `Bytes`, so `copy_to_bytes` should
        // internally be a cheap refcount bump almost all of the time.
        // But, if it isn't, this will copy it to a `Bytes` that we can
        // now clone.
        let bytes = data.copy_to_bytes(len);

        // Buffer a clone. Don't push zero-length bufs to uphold the invariant.
        if len > 0 {
            self.bufs.push_back(bytes.clone());
        }

        // Return the bytes
        bytes
    }
}

impl<B> FromIterator<B> for BufList
where
    B: Buf,
{
    fn from_iter<T: IntoIterator<Item = B>>(iter: T) -> Self {
        let mut buf_list = BufList::new();
        for buf in iter.into_iter() {
            buf_list.push_chunk(buf);
        }
        buf_list
    }
}

impl IntoIterator for BufList {
    type Item = Bytes;
    type IntoIter = IntoIter;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            iter: self.bufs.into_iter(),
        }
    }
}

impl<'a> IntoIterator for &'a BufList {
    type Item = &'a Bytes;
    type IntoIter = Iter<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Buf for BufList {
    fn remaining(&self) -> usize {
        self.bufs.iter().map(Buf::remaining).sum()
    }

    fn chunk(&self) -> &[u8] {
        self.bufs.front().map(Buf::chunk).unwrap_or(&[])
    }

    fn chunks_vectored<'iovs>(&'iovs self, iovs: &mut [IoSlice<'iovs>]) -> usize {
        // Are there more than zero iovecs to write to?
        if iovs.is_empty() {
            return 0;
        }

        // Loop over the buffers in the replay buffer list, and try to fill as
        // many iovecs as we can from each buffer.
        let mut filled = 0;
        for buf in &self.bufs {
            filled += buf.chunks_vectored(&mut iovs[filled..]);
            if filled == iovs.len() {
                return filled;
            }
        }

        filled
    }

    fn advance(&mut self, mut amt: usize) {
        while amt > 0 {
            let rem = self.bufs[0].remaining();
            // If the amount to advance by is less than the first buffer in
            // the buffer list, advance that buffer's cursor by `amt`,
            // and we're done.
            if rem > amt {
                self.bufs[0].advance(amt);
                return;
            }

            // Otherwise, advance the first buffer to its end, and
            // continue.
            self.bufs[0].advance(rem);
            amt -= rem;

            self.bufs.pop_front();
        }
    }

    fn copy_to_bytes(&mut self, len: usize) -> Bytes {
        // If the length of the requested `Bytes` is <= the length of the front
        // buffer, we can just use its `copy_to_bytes` implementation (which is
        // just a reference count bump).
        match self.bufs.front_mut() {
            Some(first) if len <= first.remaining() => {
                let buf = first.copy_to_bytes(len);
                // If we consumed the first buffer, also advance our "cursor" by
                // popping it.
                if first.remaining() == 0 {
                    self.bufs.pop_front();
                }

                buf
            }
            _ => {
                assert!(
                    len <= self.remaining(),
                    "`len` ({}) greater than remaining ({})",
                    len,
                    self.remaining()
                );
                let mut buf = BytesMut::with_capacity(len);
                buf.put(self.take(len));
                buf.freeze()
            }
        }
    }
}

/// An owned iterator over chunks in a [`BufList`].
///
/// Returned by the [`IntoIterator`] implementation for [`BufList`].
#[derive(Clone, Debug)]
pub struct IntoIter {
    iter: std::collections::vec_deque::IntoIter<Bytes>,
}

impl Iterator for IntoIter {
    type Item = Bytes;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl DoubleEndedIterator for IntoIter {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }
}

impl ExactSizeIterator for IntoIter {
    #[inline]
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl FusedIterator for IntoIter {}

/// A borrowed iterator over chunks in a [`BufList`].
///
/// Returned by [`BufList::iter`], and by the [`IntoIterator`] implementation for `&'a BufList`.
#[derive(Clone, Debug)]
pub struct Iter<'a> {
    iter: std::collections::vec_deque::Iter<'a, Bytes>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a Bytes;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    // These methods are implemented manually to forward to the underlying
    // iterator.

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }

    // fold has a special implementation, so forward it.
    #[inline]
    fn fold<B, F>(self, init: B, f: F) -> B
    where
        Self: Sized,
        F: FnMut(B, Self::Item) -> B,
    {
        self.iter.fold(init, f)
    }

    // Can't implement try_fold as it uses `std::ops::Try` which isn't stable yet, as of Rust 1.67

    #[inline]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.iter.nth(n)
    }

    #[inline]
    fn last(self) -> Option<Self::Item>
    where
        Self: Sized,
    {
        self.iter.last()
    }
}

impl<'a> DoubleEndedIterator for Iter<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }

    #[inline]
    fn rfold<B, F>(self, init: B, f: F) -> B
    where
        Self: Sized,
        F: FnMut(B, Self::Item) -> B,
    {
        self.iter.rfold(init, f)
    }

    // Can't implement try_rfold as it uses `std::ops::Try` which isn't stable yet, as of Rust 1.67.
}

impl<'a> ExactSizeIterator for Iter<'a> {
    #[inline]
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl<'a> FusedIterator for Iter<'a> {}
