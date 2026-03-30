// Copyright (c) The buf-list Contributors
// SPDX-License-Identifier: Apache-2.0

// Stateful property-based tests for Cursor.

use crate::BufList;
use anyhow::{Result, bail, ensure};
use bytes::{Buf, Bytes};
use hegel::generators;
use std::{
    fmt,
    io::{self, BufRead, IoSliceMut, Read, Seek, SeekFrom},
};

#[hegel::composite]
fn buf_lists(tc: hegel::TestCase) -> BufList {
    let chunks: Vec<Vec<u8>> = tc.draw(
        generators::vecs(
            generators::vecs(generators::integers::<u8>())
                .min_size(1)
                .max_size(127),
        )
        .max_size(31),
    );
    chunks.into_iter().map(Bytes::from).collect()
}

fn assert_io_result_eq<T: Eq + fmt::Debug>(
    buf_list_res: io::Result<T>,
    oracle_res: io::Result<T>,
) -> Result<()> {
    match (buf_list_res, oracle_res) {
        (Ok(buf_list_value), Ok(oracle_value)) => {
            ensure!(
                buf_list_value == oracle_value,
                "value didn't match: buf_list value {:?} == oracle value {:?}",
                buf_list_value,
                oracle_value
            );
        }
        (Ok(buf_list_value), Err(oracle_err)) => {
            bail!(
                "BufList value Ok({:?}) is not the same as oracle error Err({})",
                buf_list_value,
                oracle_err,
            );
        }
        (Err(buf_list_err), Ok(oracle_value)) => {
            bail!(
                "BufList error ({}) is not the same as oracle value ({:?})",
                buf_list_err,
                oracle_value
            )
        }
        (Err(buf_list_err), Err(oracle_err)) => {
            // The kinds should match.
            ensure!(
                buf_list_err.kind() == oracle_err.kind(),
                "error kind didn't match: buf_list {:?} == oracle {:?}",
                buf_list_err.kind(),
                oracle_err.kind()
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Stateful (model-based) cursor test.
// ---------------------------------------------------------------------------

struct CursorStatefulTest<'a> {
    buf_list_cursor: crate::Cursor<&'a BufList>,
    oracle_cursor: io::Cursor<&'a [u8]>,
    num_bytes: usize,
}

#[hegel::state_machine]
impl CursorStatefulTest<'_> {
    // -- Position / seek rules -----------------------------------------------

    #[rule]
    fn set_position(&mut self, tc: hegel::TestCase) {
        // Allow going past the end of the list a bit.
        let pos = tc.draw(generators::integers::<usize>().max_value(self.num_bytes * 5 / 4)) as u64;
        self.buf_list_cursor.set_position(pos);
        self.oracle_cursor.set_position(pos);
    }

    #[rule]
    fn seek_start(&mut self, tc: hegel::TestCase) {
        let pos = tc.draw(generators::integers::<usize>().max_value(self.num_bytes * 5 / 4)) as u64;
        let style = SeekFrom::Start(pos);
        assert_io_result_eq(
            self.buf_list_cursor.seek(style),
            self.oracle_cursor.seek(style),
        )
        .unwrap();
    }

    #[rule]
    fn seek_end(&mut self, tc: hegel::TestCase) {
        // Allow going past the beginning and end of the list a bit.
        let raw = tc.draw(generators::integers::<usize>().max_value(self.num_bytes * 3 / 2));
        let offset = raw as i64 - (1 + self.num_bytes * 5 / 4) as i64;
        let style = SeekFrom::End(offset);
        assert_io_result_eq(
            self.buf_list_cursor.seek(style),
            self.oracle_cursor.seek(style),
        )
        .unwrap();
    }

    #[rule]
    fn seek_current(&mut self, tc: hegel::TestCase) {
        let raw = tc.draw(generators::integers::<usize>().max_value(self.num_bytes * 3 / 2));
        // Center the index at roughly 0.
        let offset = raw as i64 - (self.num_bytes * 3 / 4) as i64;
        let style = SeekFrom::Current(offset);
        assert_io_result_eq(
            self.buf_list_cursor.seek(style),
            self.oracle_cursor.seek(style),
        )
        .unwrap();
    }

    // -- Read rules ----------------------------------------------------------

    #[rule]
    fn read(&mut self, tc: hegel::TestCase) {
        let buf_size = tc.draw(generators::integers::<usize>().max_value(self.num_bytes * 5 / 4));
        let mut bl_buf = vec![0u8; buf_size];
        let mut o_buf = vec![0u8; buf_size];
        assert_io_result_eq(
            self.buf_list_cursor.read(&mut bl_buf),
            self.oracle_cursor.read(&mut o_buf),
        )
        .unwrap();
        assert_eq!(bl_buf, o_buf, "read buffer mismatch");
    }

    #[rule]
    fn read_vectored(&mut self, tc: hegel::TestCase) {
        let n_bufs = tc.draw(generators::integers::<usize>().max_value(7));
        let mut bl_vecs: Vec<Vec<u8>> = (0..n_bufs)
            .map(|_| vec![0u8; tc.draw(generators::integers::<usize>().max_value(self.num_bytes))])
            .collect();
        let mut o_vecs = bl_vecs.clone();

        let mut bl_slices: Vec<_> = bl_vecs.iter_mut().map(|v| IoSliceMut::new(v)).collect();
        let mut o_slices: Vec<_> = o_vecs.iter_mut().map(|v| IoSliceMut::new(v)).collect();

        assert_io_result_eq(
            self.buf_list_cursor.read_vectored(&mut bl_slices),
            self.oracle_cursor.read_vectored(&mut o_slices),
        )
        .unwrap();
        assert_eq!(bl_vecs, o_vecs, "read_vectored buffer mismatch");
    }

    #[rule]
    fn read_exact(&mut self, tc: hegel::TestCase) {
        let buf_size = tc.draw(generators::integers::<usize>().max_value(self.num_bytes * 5 / 4));
        let mut bl_buf = vec![0u8; buf_size];
        let mut o_buf = vec![0u8; buf_size];
        assert_io_result_eq(
            self.buf_list_cursor.read_exact(&mut bl_buf),
            self.oracle_cursor.read_exact(&mut o_buf),
        )
        .unwrap();
        assert_eq!(bl_buf, o_buf, "read_exact buffer mismatch");
    }

    #[rule]
    fn consume(&mut self, tc: hegel::TestCase) {
        let amt = tc.draw(generators::integers::<usize>().max_value(self.num_bytes * 5 / 4));
        self.buf_list_cursor.consume(amt);
        self.oracle_cursor.consume(amt);
    }

    // -- Buf trait rules -----------------------------------------------------

    #[rule]
    fn buf_chunk(&mut self, _: hegel::TestCase) {
        let bl_chunk = self.buf_list_cursor.chunk();
        let o_chunk = self.oracle_cursor.chunk();
        // BufList returns one segment at a time while oracle returns the entire
        // remaining buffer. Verify emptiness matches and that buf_list's chunk
        // is a prefix of oracle's.
        assert_eq!(
            bl_chunk.is_empty(),
            o_chunk.is_empty(),
            "chunk emptiness mismatch"
        );
        if !bl_chunk.is_empty() {
            assert!(
                o_chunk.starts_with(bl_chunk),
                "buf_list chunk is not a prefix of oracle chunk"
            );
        }
    }

    #[rule]
    fn buf_advance(&mut self, tc: hegel::TestCase) {
        let amt = tc.draw(generators::integers::<usize>().max_value(self.num_bytes * 5 / 4));
        // Skip if already past the end, as the oracle's Buf impl has a debug
        // assertion that checks position even when advancing by 0.
        if self.buf_list_cursor.remaining() > 0 || (amt == 0 && self.oracle_cursor.remaining() > 0)
        {
            let amt = amt.min(self.buf_list_cursor.remaining());
            self.buf_list_cursor.advance(amt);
            self.oracle_cursor.advance(amt);
        }
    }

    #[rule]
    fn buf_chunks_vectored(&mut self, tc: hegel::TestCase) {
        let num_iovs = tc.draw(generators::integers::<usize>().max_value(self.num_bytes));
        let remaining = self.buf_list_cursor.remaining();
        assert_eq!(
            remaining,
            self.oracle_cursor.remaining(),
            "remaining mismatch before chunks_vectored"
        );

        let mut bl_iovs = vec![io::IoSlice::new(&[]); num_iovs];
        let mut o_iovs = vec![io::IoSlice::new(&[]); num_iovs];
        let bl_filled = self.buf_list_cursor.chunks_vectored(&mut bl_iovs);
        let o_filled = self.oracle_cursor.chunks_vectored(&mut o_iovs);

        let bl_bytes: Vec<u8> = bl_iovs[..bl_filled]
            .iter()
            .flat_map(|iov| iov.as_ref().iter().copied())
            .collect();
        let o_bytes: Vec<u8> = o_iovs[..o_filled]
            .iter()
            .flat_map(|iov| iov.as_ref().iter().copied())
            .collect();

        if remaining > 0 && num_iovs > 0 {
            assert!(
                !bl_bytes.is_empty(),
                "should return data when remaining > 0"
            );
            assert!(
                !o_bytes.is_empty(),
                "oracle should return data when remaining > 0"
            );
            assert!(
                o_bytes.starts_with(&bl_bytes),
                "buf_list data should match beginning of oracle data"
            );
            for (i, iov) in bl_iovs[..bl_filled].iter().enumerate() {
                assert!(
                    !iov.is_empty(),
                    "buf_list iov at index {i} should be non-empty"
                );
            }
        } else if remaining == 0 {
            assert!(
                bl_bytes.is_empty() && o_bytes.is_empty(),
                "should return no data when remaining == 0"
            );
        }
    }

    #[rule]
    fn buf_copy_to_bytes(&mut self, tc: hegel::TestCase) {
        let len = tc.draw(generators::integers::<usize>().max_value(self.num_bytes * 5 / 4));
        // copy_to_bytes panics if len > remaining, so guard.
        if len <= self.buf_list_cursor.remaining() && len <= self.oracle_cursor.remaining() {
            let bl_bytes = self.buf_list_cursor.copy_to_bytes(len);
            let o_bytes = self.oracle_cursor.copy_to_bytes(len);
            assert_eq!(bl_bytes, o_bytes, "copy_to_bytes mismatch");
        }
    }

    #[rule]
    fn buf_get_u8(&mut self, _: hegel::TestCase) {
        if self.buf_list_cursor.remaining() >= 1 && self.oracle_cursor.remaining() >= 1 {
            assert_eq!(
                self.buf_list_cursor.get_u8(),
                self.oracle_cursor.get_u8(),
                "get_u8 mismatch"
            );
        }
    }

    #[rule]
    fn buf_get_u64(&mut self, _: hegel::TestCase) {
        if self.buf_list_cursor.remaining() >= 8 && self.oracle_cursor.remaining() >= 8 {
            assert_eq!(
                self.buf_list_cursor.get_u64(),
                self.oracle_cursor.get_u64(),
                "get_u64 mismatch"
            );
        }
    }

    #[rule]
    fn buf_get_u64_le(&mut self, _: hegel::TestCase) {
        if self.buf_list_cursor.remaining() >= 8 && self.oracle_cursor.remaining() >= 8 {
            assert_eq!(
                self.buf_list_cursor.get_u64_le(),
                self.oracle_cursor.get_u64_le(),
                "get_u64_le mismatch"
            );
        }
    }

    // -- Async rules ---------------------------------------------------------

    #[cfg(feature = "tokio1")]
    #[rule]
    fn poll_read(&mut self, tc: hegel::TestCase) {
        use std::{mem::MaybeUninit, pin::Pin, task::Poll};
        use tokio::io::{AsyncRead, ReadBuf};

        let capacity = tc.draw(generators::integers::<usize>().max_value(self.num_bytes * 5 / 4));
        let filled = tc.draw(generators::integers::<usize>().max_value(capacity));

        let mut bl_vec = vec![MaybeUninit::uninit(); capacity];
        let mut o_vec = bl_vec.clone();

        let mut bl_buf = ReadBuf::uninit(&mut bl_vec);
        let fill_vec = vec![0u8; filled];
        bl_buf.put_slice(&fill_vec);

        let mut o_buf = ReadBuf::uninit(&mut o_vec);
        o_buf.put_slice(&fill_vec);

        let waker = dummy_waker::dummy_waker();
        let mut context = std::task::Context::from_waker(&waker);

        let bl_res = match Pin::new(&mut self.buf_list_cursor).poll_read(&mut context, &mut bl_buf)
        {
            Poll::Ready(res) => res,
            Poll::Pending => unreachable!("buf_list never returns pending"),
        };

        let o_res = match Pin::new(&mut self.oracle_cursor).poll_read(&mut context, &mut o_buf) {
            Poll::Ready(res) => res,
            Poll::Pending => unreachable!("oracle cursor never returns pending"),
        };

        assert_io_result_eq(bl_res, o_res).unwrap();
        assert_eq!(bl_buf.filled(), o_buf.filled(), "filled section mismatch");
        assert_eq!(
            bl_buf.remaining(),
            o_buf.remaining(),
            "remaining count mismatch"
        );
    }

    // -- Invariant -----------------------------------------------------------

    #[invariant]
    fn cursors_agree(&mut self, _: hegel::TestCase) {
        assert_eq!(
            self.buf_list_cursor.remaining(),
            self.oracle_cursor.remaining(),
            "remaining mismatch"
        );
        assert_eq!(
            self.buf_list_cursor.has_remaining(),
            self.oracle_cursor.has_remaining(),
            "has_remaining mismatch"
        );

        let bl_position = self.buf_list_cursor.position();
        assert_eq!(
            bl_position,
            self.oracle_cursor.position(),
            "position mismatch"
        );
        assert_io_result_eq(
            self.buf_list_cursor.stream_position(),
            self.oracle_cursor.stream_position(),
        )
        .unwrap();

        // fill_buf returns an empty slice iff we're at or past the end.
        let fill_buf = self
            .buf_list_cursor
            .fill_buf()
            .expect("fill_buf never errors");
        if bl_position < self.num_bytes as u64 {
            assert!(
                !fill_buf.is_empty(),
                "fill_buf cannot be empty since position {} < num_bytes {}",
                bl_position,
                self.num_bytes,
            );
        } else {
            assert!(
                fill_buf.is_empty(),
                "fill_buf must be empty since position {} >= num_bytes {}",
                bl_position,
                self.num_bytes,
            );
        }

        self.buf_list_cursor
            .assert_invariants()
            .expect("internal invariants violated");
    }
}

/// Assert that buf_list's cursor behaves identically to std::io::Cursor.
#[hegel::test(test_cases = 200)]
fn hegel_cursor_stateful(tc: hegel::TestCase) {
    let buf_list = tc.draw(buf_lists());
    let num_bytes = buf_list.num_bytes();
    let oracle_data: Vec<u8> = buf_list
        .clone()
        .copy_to_bytes(buf_list.remaining())
        .to_vec();
    let m = CursorStatefulTest {
        buf_list_cursor: crate::Cursor::new(&buf_list),
        oracle_cursor: io::Cursor::new(&oracle_data),
        num_bytes,
    };
    hegel::stateful::run(m, tc);
}

#[test]
fn test_cursor_buf_trait() {
    // Create a BufList with multiple chunks
    let mut buf_list = BufList::new();
    buf_list.push_chunk(&b"hello "[..]);
    buf_list.push_chunk(&b"world"[..]);
    buf_list.push_chunk(&b"!"[..]);

    let mut cursor = crate::Cursor::new(buf_list.clone());

    // Test remaining()
    assert_eq!(cursor.remaining(), 12);

    // Test chunk()
    assert_eq!(cursor.chunk(), b"hello ");

    // Test advance()
    cursor.advance(6);
    assert_eq!(cursor.remaining(), 6);
    assert_eq!(cursor.chunk(), b"world");

    // Advance within the same chunk
    cursor.advance(3);
    assert_eq!(cursor.remaining(), 3);
    assert_eq!(cursor.chunk(), b"ld");

    // Advance to the next chunk
    cursor.advance(2);
    assert_eq!(cursor.remaining(), 1);
    assert_eq!(cursor.chunk(), b"!");

    // Advance to the end
    cursor.advance(1);
    assert_eq!(cursor.remaining(), 0);
    assert_eq!(cursor.chunk(), b"");

    // Test chunks_vectored
    let mut cursor = crate::Cursor::new(buf_list.clone());
    let mut iovs = [io::IoSlice::new(&[]); 3];
    let filled = cursor.chunks_vectored(&mut iovs);
    assert_eq!(filled, 3);
    assert_eq!(iovs[0].as_ref(), b"hello ");
    assert_eq!(iovs[1].as_ref(), b"world");
    assert_eq!(iovs[2].as_ref(), b"!");

    // Test chunks_vectored after advancing
    cursor.advance(6);
    let mut iovs = [io::IoSlice::new(&[]); 3];
    let filled = cursor.chunks_vectored(&mut iovs);
    assert_eq!(filled, 2);
    assert_eq!(iovs[0].as_ref(), b"world");
    assert_eq!(iovs[1].as_ref(), b"!");

    // Test chunks_vectored with more iovs than remaining chunks
    let cursor2 = crate::Cursor::new(&buf_list);
    let mut iovs2 = [io::IoSlice::new(&[]); 10];
    let filled2 = cursor2.chunks_vectored(&mut iovs2);
    assert_eq!(filled2, 3, "Should only fill 3 iovs for 3 chunks");
    let total_bytes: usize = iovs2[..filled2].iter().map(|iov| iov.len()).sum();
    assert_eq!(total_bytes, 12, "Total bytes should be 12");
}
