// Copyright (c) The buf-list Contributors
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "futures03")]
mod futures_imp;
#[cfg(test)]
mod tests;
#[cfg(feature = "tokio1")]
mod tokio_imp;

use crate::{errors::ReadExactError, BufList};
use bytes::Bytes;
use std::{
    cmp::Ordering,
    io::{self, IoSliceMut, SeekFrom},
};

/// A `Cursor` wraps an in-memory `BufList` and provides it with a [`Seek`] implementation.
///
/// `Cursor`s allow `BufList`s to implement [`Read`] and [`BufRead`], allowing a `BufList` to be
/// used anywhere you might use a reader or writer that does actual I/O.
///
/// The cursor may either own or borrow a `BufList`: both `Cursor<BufList>` and `Cursor<&BufList>`
/// are supported.
///
/// # Optional features
///
/// * `tokio1`: With this feature enabled, [`Cursor`] implements the `tokio` crate's
///   [`AsyncSeek`](tokio::io::AsyncSeek), [`AsyncRead`](tokio::io::AsyncRead) and
///   [`AsyncBufRead`](tokio::io::AsyncBufRead).
/// * `futures03`: With this feature enabled, [`Cursor`] implements the `futures` crate's
///   [`AsyncSeek`](futures_io_03::AsyncSeek), [`AsyncRead`](futures_io_03::AsyncRead) and
///   [`AsyncBufRead`](futures_io_03::AsyncBufRead).
///
/// [`Read`]: std::io::Read
/// [`BufRead`]: std::io::BufRead
/// [`Seek`]: std::io::Seek
pub struct Cursor<T> {
    inner: T,

    /// Data associated with the cursor.
    data: CursorData,
}

impl<T: AsRef<BufList>> Cursor<T> {
    /// Creates a new cursor wrapping the provided `BufList`.
    ///
    /// # Examples
    ///
    /// ```
    /// use buf_list::{BufList, Cursor};
    ///
    /// let cursor = Cursor::new(BufList::new());
    /// ```
    pub fn new(inner: T) -> Cursor<T> {
        let data = CursorData::new(inner.as_ref());
        Cursor { inner, data }
    }

    /// Consumes this cursor, returning the underlying value.
    ///
    /// # Examples
    ///
    /// ```
    /// use buf_list::{BufList, Cursor};
    ///
    /// let cursor = Cursor::new(BufList::new());
    ///
    /// let vec = cursor.into_inner();
    /// ```
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Gets a reference to the underlying value in this cursor.
    ///
    /// # Examples
    ///
    /// ```
    /// use buf_list::{BufList, Cursor};
    ///
    /// let cursor = Cursor::new(BufList::new());
    ///
    /// let reference = cursor.get_ref();
    /// ```
    #[cfg(const_fn_trait_bounds)]
    pub const fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Gets a reference to the underlying value in this cursor.
    ///
    /// # Examples
    ///
    /// ```
    /// use buf_list::{BufList, Cursor};
    ///
    /// let cursor = Cursor::new(BufList::new());
    ///
    /// let reference = cursor.get_ref();
    /// ```
    #[cfg(not(const_fn_trait_bounds))]
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Returns the current position of this cursor.
    ///
    /// # Examples
    ///
    /// ```
    /// use buf_list::{BufList, Cursor};
    /// use std::io::prelude::*;
    /// use std::io::SeekFrom;
    ///
    /// let mut cursor = Cursor::new(BufList::from(&[1, 2, 3, 4, 5][..]));
    ///
    /// assert_eq!(cursor.position(), 0);
    ///
    /// cursor.seek(SeekFrom::Current(2)).unwrap();
    /// assert_eq!(cursor.position(), 2);
    ///
    /// cursor.seek(SeekFrom::Current(-1)).unwrap();
    /// assert_eq!(cursor.position(), 1);
    /// ```
    #[cfg(const_fn_trait_bounds)]
    pub const fn position(&self) -> u64 {
        self.data.pos
    }

    /// Returns the current position of this cursor.
    ///
    /// # Examples
    ///
    /// ```
    /// use buf_list::{BufList, Cursor};
    /// use std::io::prelude::*;
    /// use std::io::SeekFrom;
    ///
    /// let mut cursor = Cursor::new(BufList::from(&[1, 2, 3, 4, 5][..]));
    ///
    /// assert_eq!(cursor.position(), 0);
    ///
    /// cursor.seek(SeekFrom::Current(2)).unwrap();
    /// assert_eq!(cursor.position(), 2);
    ///
    /// cursor.seek(SeekFrom::Current(-1)).unwrap();
    /// assert_eq!(cursor.position(), 1);
    /// ```
    #[cfg(not(const_fn_trait_bounds))]
    pub fn position(&self) -> u64 {
        self.data.pos
    }

    /// Sets the position of this cursor.
    ///
    /// # Examples
    ///
    /// ```
    /// use buf_list::{BufList, Cursor};
    ///
    /// let mut cursor = Cursor::new(BufList::from(&[1, 2, 3, 4, 5][..]));
    ///
    /// assert_eq!(cursor.position(), 0);
    ///
    /// cursor.set_position(2);
    /// assert_eq!(cursor.position(), 2);
    ///
    /// cursor.set_position(4);
    /// assert_eq!(cursor.position(), 4);
    /// ```
    pub fn set_position(&mut self, pos: u64) {
        self.data.set_pos(pos);
    }

    // ---
    // Helper methods
    // ---
    #[cfg(test)]
    fn assert_invariants(&self) -> anyhow::Result<()> {
        self.data.assert_invariants()
    }
}

impl<T> Clone for Cursor<T>
where
    T: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        Cursor {
            inner: self.inner.clone(),
            data: self.data.clone(),
        }
    }

    #[inline]
    fn clone_from(&mut self, other: &Self) {
        self.inner.clone_from(&other.inner);
        self.data = other.data.clone();
    }
}

impl<T: AsRef<BufList>> io::Seek for Cursor<T> {
    fn seek(&mut self, style: SeekFrom) -> io::Result<u64> {
        self.data.seek_impl(style)
    }

    #[cfg(seek_convenience)]
    fn stream_position(&mut self) -> io::Result<u64> {
        Ok(self.data.pos)
    }
}

impl<T: AsRef<BufList>> io::Read for Cursor<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Ok(self.data.read_impl(self.inner.as_ref(), buf))
    }

    fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        Ok(self.data.read_vectored_impl(self.inner.as_ref(), bufs))
    }

    // TODO: is_read_vectored once that's available on stable Rust.

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.data.read_exact_impl(self.inner.as_ref(), buf)
    }
}

impl<T: AsRef<BufList>> io::BufRead for Cursor<T> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Ok(self.data.fill_buf_impl(self.inner.as_ref()))
    }

    fn consume(&mut self, amt: usize) {
        self.data.consume_impl(amt);
    }
}

#[derive(Clone, Debug)]
struct CursorData {
    /// An index of chunks and their start positions. There's an additional index at the end, which
    /// is the length of the list (list.num_bytes()).
    start_pos: Box<[u64]>,

    /// The chunk number the cursor is pointing to. Kept in sync with pos.
    ///
    /// This is within the range [0, self.start_pos.len()). It is self.start_pos.len() - 1 iff pos
    /// is greater than list.num_bytes().
    chunk: usize,

    /// The overall position in the stream. Kept in sync with chunk.
    pos: u64,
}

impl CursorData {
    fn new(inner: &BufList) -> Self {
        let mut start_pos = Vec::with_capacity(inner.num_chunks() + 1);
        let mut next = 0u64;
        for chunk in inner.iter() {
            start_pos.push(next);
            next += chunk.len() as u64;
        }
        // Add the length of the chunk at the end.
        start_pos.push(next);

        Self {
            start_pos: start_pos.into_boxed_slice(),
            chunk: 0,
            pos: 0,
        }
    }

    #[cfg(test)]
    fn assert_invariants(&self) -> anyhow::Result<()> {
        use anyhow::ensure;

        ensure!(
            self.pos >= self.start_pos[self.chunk],
            "invariant failed: current position {} >= start position {} (chunk = {})",
            self.pos,
            self.start_pos[self.chunk],
            self.chunk
        );

        let next_pos = self.start_pos.get(self.chunk + 1).copied().into();
        ensure!(
            Offset::Value(self.pos) < next_pos,
            "invariant failed: next start position {:?} > current position {} (chunk = {})",
            next_pos,
            self.pos,
            self.chunk
        );

        Ok(())
    }

    fn seek_impl(&mut self, style: SeekFrom) -> io::Result<u64> {
        let (base_pos, offset) = match style {
            SeekFrom::Start(n) => {
                self.set_pos(n);
                return Ok(n);
            }
            SeekFrom::End(n) => (self.num_bytes(), n),
            SeekFrom::Current(n) => (self.pos, n),
        };
        // Can't use checked_add_signed since it was only stabilized in Rust 1.66. This is adapted
        // from
        // https://github.com/rust-lang/rust/blame/ed937594d3/library/std/src/io/cursor.rs#L295-L299.
        let new_pos = if offset >= 0 {
            base_pos.checked_add(offset as u64)
        } else {
            base_pos.checked_sub(offset.wrapping_neg() as u64)
        };
        match new_pos {
            Some(n) => {
                self.set_pos(n);
                Ok(self.pos)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid seek to a negative or overflowing position",
            )),
        }
    }

    fn read_impl(&mut self, list: &BufList, buf: &mut [u8]) -> usize {
        // Read as much as possible until we fill up the buffer.
        let mut buf_pos = 0;
        while buf_pos < buf.len() {
            let (chunk, chunk_pos) = match self.get_chunk_and_pos(list) {
                Some(value) => value,
                None => break,
            };
            // The number of bytes to copy is the smaller of the two:
            // - the length of the chunk - the position in it.
            // - the number of bytes remaining, which is buf.len() - buf_pos.
            let n_to_copy = (chunk.len() - chunk_pos).min(buf.len() - buf_pos);
            let chunk_bytes = chunk.as_ref();

            let bytes_to_copy = &chunk_bytes[chunk_pos..(chunk_pos + n_to_copy)];
            let dest = &mut buf[buf_pos..(buf_pos + n_to_copy)];
            dest.copy_from_slice(bytes_to_copy);
            buf_pos += n_to_copy;

            // Increment the position.
            self.pos += n_to_copy as u64;
            // If we've finished reading through the chunk, move to the next chunk.
            if n_to_copy == chunk.len() - chunk_pos {
                self.chunk += 1;
            }
        }

        buf_pos
    }

    fn read_vectored_impl(&mut self, list: &BufList, bufs: &mut [IoSliceMut<'_>]) -> usize {
        let mut nread = 0;
        for buf in bufs {
            // Copy data from the buffer until we run out of bytes to copy.
            let n = self.read_impl(list, buf);
            nread += n;
            if n < buf.len() {
                break;
            }
        }
        nread
    }

    fn read_exact_impl(&mut self, list: &BufList, buf: &mut [u8]) -> io::Result<()> {
        // This is the same as read_impl as long as there's enough space.
        let remaining = self.num_bytes().saturating_sub(self.pos);
        let buf_len = buf.len();
        if remaining < buf_len as u64 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                ReadExactError { remaining, buf_len },
            ));
        }

        self.read_impl(list, buf);
        Ok(())
    }

    fn fill_buf_impl<'a>(&self, list: &'a BufList) -> &'a [u8] {
        const EMPTY_SLICE: &[u8] = &[];
        match self.get_chunk_and_pos(list) {
            Some((chunk, chunk_pos)) => &chunk.as_ref()[chunk_pos..],
            // An empty return value means the end of the buffer has been reached.
            None => EMPTY_SLICE,
        }
    }

    fn consume_impl(&mut self, amt: usize) {
        self.set_pos(self.pos + amt as u64);
    }

    fn set_pos(&mut self, new_pos: u64) {
        match new_pos.cmp(&self.pos) {
            Ordering::Greater => {
                let next_start = self.start_pos.get(self.chunk + 1).copied().into();
                if Offset::Value(new_pos) < next_start {
                    // Within the same chunk.
                } else {
                    // The above check ensures that we're not currently pointing to the last index
                    // (since it would have returned Eof, which is greater than Offset(n) for any
                    // n).
                    //
                    // Do a binary search for this element.
                    match self.start_pos[self.chunk + 1..].binary_search(&new_pos) {
                        // We're starting the search from self.chunk + 1, which means that the value
                        // returned from binary_search is 1 less than the actual delta.
                        Ok(delta_minus_one) => {
                            // Exactly at the start point of a chunk.
                            self.chunk += 1 + delta_minus_one;
                        }
                        // The value returned in the error case (not at the start point of a chunk)
                        // is (delta - 1) + 1, so just delta.
                        Err(delta) => {
                            debug_assert!(
                                delta > 0,
                                "delta must be at least 1 since we already \
                                checked the same chunk (self.chunk = {})",
                                self.chunk,
                            );
                            self.chunk += delta;
                        }
                    }
                }
            }
            Ordering::Equal => {}
            Ordering::Less => {
                if self.start_pos.get(self.chunk).copied() <= Some(new_pos) {
                    // Within the same chunk.
                } else {
                    match self.start_pos[..self.chunk].binary_search(&new_pos) {
                        Ok(chunk) => {
                            // Exactly at the start point of a chunk.
                            self.chunk = chunk;
                        }
                        Err(chunk_plus_1) => {
                            debug_assert!(
                                chunk_plus_1 > 0,
                                "chunk_plus_1 must be at least 1 since self.start_pos[0] is 0 \
                                 (self.chunk = {})",
                                self.chunk,
                            );
                            self.chunk = chunk_plus_1 - 1;
                        }
                    }
                }
            }
        }
        self.pos = new_pos;
    }

    #[inline]
    fn get_chunk_and_pos<'a>(&self, list: &'a BufList) -> Option<(&'a Bytes, usize)> {
        match list.get_chunk(self.chunk) {
            Some(chunk) => {
                // This guarantees that pos is not past the end of the list.
                debug_assert!(
                    self.pos < self.num_bytes(),
                    "self.pos ({}) is less than num_bytes ({})",
                    self.pos,
                    self.num_bytes()
                );
                Some((chunk, (self.pos - self.start_pos[self.chunk]) as usize))
            }
            None => {
                // pos is past the end of the list.
                None
            }
        }
    }

    fn num_bytes(&self) -> u64 {
        *self
            .start_pos
            .last()
            .expect("start_pos always has at least one element")
    }
}

/// This is the same as Option<T> except Offset and Eof are reversed in ordering, i.e. Eof >
/// Offset(T) for any T.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
enum Offset<T> {
    Value(T),
    Eof,
}

impl<T> From<Option<T>> for Offset<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(v) => Self::Value(v),
            None => Self::Eof,
        }
    }
}
