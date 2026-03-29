// Copyright (c) The buf-list Contributors
// SPDX-License-Identifier: Apache-2.0

// Property-based tests for Cursor.

// The DefaultGenerator derive generates PascalCase variable names for enum variants.
#![expect(non_snake_case)]

use crate::BufList;
use anyhow::{Context, Result, bail, ensure};
use bytes::{Buf, Bytes};
// Import the Generator and DefaultGenerator *traits* (distinct from the DefaultGenerator derive
// macro imported above) so that .map() and .default_generator() are available.
use hegel::generators::DefaultGenerator as _;
use hegel::{DefaultGenerator, Generator, generators};
use std::{
    fmt,
    io::{self, BufRead, IoSliceMut, Read, Seek, SeekFrom},
};

/// Assert that buf_list's cursor behaves identically to std::io::Cursor.
#[hegel::test(test_cases = 200)]
fn hegel_cursor_ops(tc: hegel::TestCase) {
    let buf_list = tc.draw(buf_lists());

    let num_bytes = buf_list.num_bytes();
    let bytes = buf_list.clone().copy_to_bytes(buf_list.remaining());
    let mut buf_list_cursor = crate::Cursor::new(&buf_list);
    let mut oracle_cursor = io::Cursor::new(bytes.as_ref());

    let num_ops = tc.draw(generators::integers::<usize>().max_value(255));

    eprintln!("\n**** start! num_bytes={num_bytes}, num_ops={num_ops}");

    for index in 0..num_ops {
        let cursor_op = tc.draw(cursor_ops(num_bytes));
        // apply_and_compare prints out the rest of the line.
        eprint!("** index {}, operation {:?}: ", index, cursor_op);
        cursor_op
            .apply_and_compare(&mut buf_list_cursor, &mut oracle_cursor)
            .with_context(|| format!("for index {}", index))
            .unwrap();
    }
    eprintln!("**** success");
}

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

#[derive(Clone, Debug, DefaultGenerator)]
enum CursorOp {
    SetPosition(u64),
    SeekStart(u64),
    SeekEnd(i64),
    SeekCurrent(i64),
    Read(usize),
    ReadVectored(Vec<usize>),
    ReadExact(usize),
    // fill_buf can't be tested here because oracle is a contiguous block. Instead, we check its
    // return value separately.
    Consume(usize),
    // Buf trait operations.
    BufChunk,
    BufAdvance(usize),
    BufChunksVectored(usize),
    BufCopyToBytes(usize),
    BufGetU8,
    BufGetU64,
    BufGetU64Le,
    // No need to test futures03 imps since they're simple wrappers around the main imps.
    #[cfg(feature = "tokio1")]
    PollRead {
        capacity: usize,
        filled: usize,
    },
}

/// Build a CursorOp generator with field constraints that depend on the
/// BufList's size.
///
/// Uses `#[derive(DefaultGenerator)]` on `CursorOp` for variant selection,
/// with per-variant field generators configured inline. This removes the need
/// for a separate discriminant enum.
fn cursor_ops(num_bytes: usize) -> impl Generator<CursorOp> {
    // `d` provides access to default_*() methods for building per-variant generators.
    let d = CursorOp::default_generator();

    let gen = CursorOp::default_generator()
        .SetPosition(
            d.default_SetPosition()
                // Allow going past the end of the list a bit.
                .value(
                    generators::integers::<usize>()
                        .max_value(num_bytes * 5 / 4)
                        .map(|v| v as u64),
                ),
        )
        .SeekStart(
            d.default_SeekStart()
                // Allow going past the end of the list a bit.
                .value(
                    generators::integers::<usize>()
                        .max_value(num_bytes * 5 / 4)
                        .map(|v| v as u64),
                ),
        )
        .SeekEnd(
            d.default_SeekEnd()
                // Allow going past the beginning and end of the list a bit.
                .value(
                    generators::integers::<usize>()
                        .max_value(num_bytes * 3 / 2)
                        .map(move |raw| raw as i64 - (1 + num_bytes * 5 / 4) as i64),
                ),
        )
        .SeekCurrent(
            d.default_SeekCurrent()
                // Center the index at roughly 0.
                .value(
                    generators::integers::<usize>()
                        .max_value(num_bytes * 3 / 2)
                        .map(move |raw| raw as i64 - (num_bytes * 3 / 4) as i64),
                ),
        )
        .Read(
            d.default_Read()
                .value(generators::integers::<usize>().max_value(num_bytes * 5 / 4)),
        )
        .ReadVectored(d.default_ReadVectored().value(
            generators::vecs(generators::integers::<usize>().max_value(num_bytes)).max_size(7),
        ))
        .ReadExact(
            d.default_ReadExact()
                .value(generators::integers::<usize>().max_value(num_bytes * 5 / 4)),
        )
        .Consume(
            d.default_Consume()
                .value(generators::integers::<usize>().max_value(num_bytes * 5 / 4)),
        )
        .BufAdvance(
            d.default_BufAdvance()
                .value(generators::integers::<usize>().max_value(num_bytes * 5 / 4)),
        )
        .BufChunksVectored(
            d.default_BufChunksVectored()
                .value(generators::integers::<usize>().max_value(num_bytes)),
        )
        .BufCopyToBytes(
            d.default_BufCopyToBytes()
                .value(generators::integers::<usize>().max_value(num_bytes * 5 / 4)),
        );

    #[cfg(feature = "tokio1")]
    let gen = gen.PollRead(poll_read_op(num_bytes));

    gen
}

/// Generates `CursorOp::PollRead` with the constraint that `filled <= capacity`.
#[cfg(feature = "tokio1")]
#[hegel::composite]
fn poll_read_op(tc: hegel::TestCase, num_bytes: usize) -> CursorOp {
    let capacity = tc.draw(generators::integers::<usize>().max_value(num_bytes * 5 / 4));
    // filled is in 0..=capacity, to sometimes fill the whole buffer.
    let filled = tc.draw(generators::integers::<usize>().max_value(capacity));
    CursorOp::PollRead { capacity, filled }
}

impl CursorOp {
    fn apply_and_compare(
        self,
        // The "mut" here is used in the branches corresponding to optional features.
        #[allow(unused_mut)] mut buf_list: &mut crate::Cursor<&BufList>,
        #[allow(unused_mut)] mut oracle: &mut io::Cursor<&[u8]>,
    ) -> Result<()> {
        let num_bytes = buf_list.get_ref().num_bytes();
        match self {
            Self::SetPosition(pos) => {
                eprintln!("set position: {}", pos);

                buf_list.set_position(pos);
                oracle.set_position(pos);
            }
            Self::SeekStart(pos) => {
                let style = SeekFrom::Start(pos);
                eprintln!("style: {:?}", style);

                let buf_list_res = buf_list.seek(style);
                let oracle_res = oracle.seek(style);
                Self::assert_io_result_eq(buf_list_res, oracle_res)
                    .context("operation result didn't match")?;
            }
            Self::SeekEnd(offset) => {
                let style = SeekFrom::End(offset);
                eprintln!("style: {:?}", style);

                let buf_list_res = buf_list.seek(style);
                let oracle_res = oracle.seek(style);
                Self::assert_io_result_eq(buf_list_res, oracle_res)
                    .context("operation result didn't match")?;
            }
            Self::SeekCurrent(offset) => {
                let style = SeekFrom::Current(offset);
                eprintln!("style: {:?}", style);

                let buf_list_res = buf_list.seek(style);
                let oracle_res = oracle.seek(style);
                Self::assert_io_result_eq(buf_list_res, oracle_res)
                    .context("operation result didn't match")?;
            }
            Self::Read(buf_size) => {
                eprintln!("buf_size: {}", buf_size);

                // Must initialize the whole vec here so &mut returns the whole buffer -- can't use
                // with_capacity!
                let mut buf_list_buf = vec![0u8; buf_size];
                let mut oracle_buf = vec![0u8; buf_size];

                let buf_list_res = buf_list.read(&mut buf_list_buf);
                let oracle_res = oracle.read(&mut oracle_buf);
                Self::assert_io_result_eq(buf_list_res, oracle_res)
                    .context("operation result didn't match")?;
                ensure!(buf_list_buf == oracle_buf, "read buffer matches");
            }
            Self::ReadVectored(sizes) => {
                // Build a bunch of IoSliceMuts.
                let mut buf_list_vecs: Vec<_> = sizes
                    .into_iter()
                    .map(|size| {
                        // Must initialize the whole vec here so &mut returns the whole buffer --
                        // can't use with_capacity!
                        vec![0u8; size]
                    })
                    .collect();
                let mut oracle_vecs = buf_list_vecs.clone();

                let mut buf_list_slices: Vec<_> = buf_list_vecs
                    .iter_mut()
                    .map(|v| IoSliceMut::new(v))
                    .collect();
                let mut oracle_slices: Vec<_> =
                    oracle_vecs.iter_mut().map(|v| IoSliceMut::new(v)).collect();

                let buf_list_res = buf_list.read_vectored(&mut buf_list_slices);
                let oracle_res = oracle.read_vectored(&mut oracle_slices);
                Self::assert_io_result_eq(buf_list_res, oracle_res)
                    .context("operation result didn't match")?;

                // Also check that the slices read match exactly.
                ensure!(
                    buf_list_vecs == oracle_vecs,
                    "read vecs didn't match: buf_list: {:?} == oracle: {:?}",
                    buf_list_vecs,
                    oracle_vecs
                );
            }
            Self::ReadExact(buf_size) => {
                eprintln!("buf_size: {}", buf_size);

                // Must initialize the whole vec here so &mut returns the whole buffer -- can't use
                // with_capacity!
                let mut buf_list_buf = vec![0u8; buf_size];
                let mut oracle_buf = vec![0u8; buf_size];

                let buf_list_res = buf_list.read_exact(&mut buf_list_buf);
                let oracle_res = oracle.read_exact(&mut oracle_buf);
                Self::assert_io_result_eq(buf_list_res, oracle_res)
                    .context("operation result didn't match")?;
                ensure!(buf_list_buf == oracle_buf, "read buffer matches");
            }
            Self::Consume(amt) => {
                eprintln!("amt: {}", amt);

                buf_list.consume(amt);
                oracle.consume(amt);
            }
            Self::BufChunk => {
                eprintln!("buf_chunk");

                let buf_list_chunk = buf_list.chunk();
                let oracle_chunk = oracle.chunk();

                // We can't directly compare chunks because BufList returns one
                // segment at a time while oracle returns the entire remaining
                // buffer. Instead, verify that:
                //
                // 1. is_empty matches for both chunks.
                // 2. Both start with the same data (buf_list's chunk is a prefix of oracle's)
                ensure!(
                    buf_list_chunk.is_empty() == oracle_chunk.is_empty(),
                    "chunk emptiness didn't match: buf_list is_empty {} == oracle is_empty {}",
                    buf_list_chunk.is_empty(),
                    oracle_chunk.is_empty()
                );

                if !buf_list_chunk.is_empty() {
                    // Verify buf_list's chunk is a prefix of oracle's chunk.
                    ensure!(
                        oracle_chunk.starts_with(buf_list_chunk),
                        "buf_list chunk is not a prefix of oracle chunk"
                    );
                }
            }
            Self::BufAdvance(amt) => {
                eprintln!("buf_advance: {}", amt);

                // Skip if already past the end, as the oracle's Buf impl has a debug assertion
                // that checks position even when advancing by 0.
                if buf_list.remaining() > 0 || amt == 0 && oracle.remaining() > 0 {
                    // Cap the advance amount to the remaining bytes to avoid
                    // hitting the debug assertion in std::io::Cursor's Buf
                    // impl. While the Buf trait doesn't require this, the
                    // oracle has a debug_assert that panics if we try to
                    // advance past the end.
                    let amt = amt.min(buf_list.remaining());
                    buf_list.advance(amt);
                    oracle.advance(amt);
                } else {
                    eprintln!("  skipping: cursor past end");
                }
            }
            Self::BufChunksVectored(num_iovs) => {
                eprintln!("buf_chunks_vectored: {} iovs", num_iovs);

                // First verify remaining() matches.
                let buf_list_remaining = buf_list.remaining();
                let oracle_remaining = oracle.remaining();
                ensure!(
                    buf_list_remaining == oracle_remaining,
                    "chunks_vectored: remaining didn't match before \
                     calling chunks_vectored: buf_list {} == oracle {}",
                    buf_list_remaining,
                    oracle_remaining
                );

                let mut buf_list_iovs = vec![io::IoSlice::new(&[]); num_iovs];
                let mut oracle_iovs = vec![io::IoSlice::new(&[]); num_iovs];

                let buf_list_filled = buf_list.chunks_vectored(&mut buf_list_iovs);
                let oracle_filled = oracle.chunks_vectored(&mut oracle_iovs);

                // We can't directly compare filled counts or total bytes
                // because BufList may have multiple chunks while the oracle
                // (std::io::Cursor) is contiguous. When there are fewer iovs
                // than chunks, BufList will only fill what it can, while oracle
                // fills everything into one iov.
                //
                // Instead, we verify that:
                // 1. Both returned at least some data if there are bytes
                //    remaining.
                // 2. The data that was returned matches (buf_list's data is a
                //    prefix of oracle's data).
                let buf_list_bytes: Vec<u8> = buf_list_iovs[..buf_list_filled]
                    .iter()
                    .flat_map(|iov| iov.as_ref().iter().copied())
                    .collect();
                let oracle_bytes: Vec<u8> = oracle_iovs[..oracle_filled]
                    .iter()
                    .flat_map(|iov| iov.as_ref().iter().copied())
                    .collect();

                if buf_list_remaining > 0 && num_iovs > 0 {
                    // If there are bytes remaining and iovs available, should
                    // return some data.
                    ensure!(
                        !buf_list_bytes.is_empty(),
                        "chunks_vectored should return some data \
                         when remaining = {buf_list_remaining} > 0 \
                         and num_iovs = {num_iovs} > 0"
                    );
                    ensure!(
                        !oracle_bytes.is_empty(),
                        "oracle chunks_vectored should return some data \
                         when remaining > 0 and num_iovs > 0"
                    );

                    // Verify that buf_list's data matches the beginning of
                    // oracle's data.
                    ensure!(
                        oracle_bytes.starts_with(&buf_list_bytes),
                        "buf_list chunks_vectored data should match beginning \
                         of oracle data"
                    );

                    // Verify that all iovs up to buf_list_filled are non-empty.
                    for (i, iov) in buf_list_iovs[..buf_list_filled].iter().enumerate() {
                        ensure!(
                            !iov.is_empty(),
                            "buf_list iov at index {i} should be non-empty",
                        );
                    }
                } else if buf_list_remaining == 0 {
                    // If no bytes remaining, should return no data.
                    ensure!(
                        buf_list_bytes.is_empty() && oracle_bytes.is_empty(),
                        "chunks_vectored should return no data when \
                         remaining == 0"
                    );
                }
                // If num_iovs == 0, we can't check anything since no iovs were
                // provided. All we're doing is ensuring that buf_list doesn't
                // panic.
            }
            Self::BufCopyToBytes(len) => {
                eprintln!("buf_copy_to_bytes: {}", len);

                // copy_to_bytes can panic if len > remaining, so check first.
                let buf_list_remaining = buf_list.remaining();
                let oracle_remaining = oracle.remaining();

                if len <= buf_list_remaining && len <= oracle_remaining {
                    let buf_list_bytes = buf_list.copy_to_bytes(len);
                    let oracle_bytes = oracle.copy_to_bytes(len);

                    ensure!(buf_list_bytes == oracle_bytes, "copy_to_bytes didn't match");
                } else {
                    // Both should panic, so skip this operation.
                    eprintln!("  skipping: len {} > remaining {}", len, buf_list_remaining);
                }
            }
            Self::BufGetU8 => {
                eprintln!("buf_get_u8");

                if buf_list.remaining() >= 1 && oracle.remaining() >= 1 {
                    let buf_list_val = buf_list.get_u8();
                    let oracle_val = oracle.get_u8();
                    ensure!(
                        buf_list_val == oracle_val,
                        "get_u8 didn't match: buf_list {} == oracle {}",
                        buf_list_val,
                        oracle_val
                    );
                } else {
                    eprintln!("  skipping: not enough bytes remaining");
                }
            }
            Self::BufGetU64 => {
                eprintln!("buf_get_u64");

                if buf_list.remaining() >= 8 && oracle.remaining() >= 8 {
                    let buf_list_val = buf_list.get_u64();
                    let oracle_val = oracle.get_u64();
                    ensure!(
                        buf_list_val == oracle_val,
                        "get_u64 didn't match: buf_list {} == oracle {}",
                        buf_list_val,
                        oracle_val
                    );
                } else {
                    eprintln!("  skipping: not enough bytes remaining");
                }
            }
            Self::BufGetU64Le => {
                eprintln!("buf_get_u64_le");

                if buf_list.remaining() >= 8 && oracle.remaining() >= 8 {
                    let buf_list_val = buf_list.get_u64_le();
                    let oracle_val = oracle.get_u64_le();
                    ensure!(
                        buf_list_val == oracle_val,
                        "get_u64_le didn't match: buf_list {} == oracle {}",
                        buf_list_val,
                        oracle_val
                    );
                } else {
                    eprintln!("  skipping: not enough bytes remaining");
                }
            }
            #[cfg(feature = "tokio1")]
            Self::PollRead { capacity, filled } => {
                use std::{mem::MaybeUninit, pin::Pin, task::Poll};
                use tokio::io::{AsyncRead, ReadBuf};

                let mut buf_list_vec = vec![MaybeUninit::uninit(); capacity];
                let mut oracle_vec = buf_list_vec.clone();

                let mut buf_list_buf = ReadBuf::uninit(&mut buf_list_vec);

                let fill_vec = vec![0u8; filled];
                buf_list_buf.put_slice(&fill_vec);

                let mut oracle_buf = ReadBuf::uninit(&mut oracle_vec);
                oracle_buf.put_slice(&fill_vec);

                eprintln!("capacity: {}, filled: {}", capacity, filled);

                let waker = dummy_waker::dummy_waker();
                let mut context = std::task::Context::from_waker(&waker);
                let mut buf_list_pinned = Pin::new(buf_list);
                let buf_list_res = match buf_list_pinned
                    .as_mut()
                    .poll_read(&mut context, &mut buf_list_buf)
                {
                    Poll::Ready(res) => res,
                    Poll::Pending => unreachable!("buf_list never returns pending"),
                };

                let mut oracle_pinned = Pin::new(oracle);
                let oracle_res = match oracle_pinned
                    .as_mut()
                    .poll_read(&mut context, &mut oracle_buf)
                {
                    Poll::Ready(res) => res,
                    Poll::Pending => unreachable!("oracle cursor never returns pending"),
                };

                Self::assert_io_result_eq(buf_list_res, oracle_res)
                    .context("result didn't match")?;
                ensure!(
                    buf_list_buf.filled() == oracle_buf.filled(),
                    "filled section didn't match"
                );
                ensure!(
                    buf_list_buf.remaining() == oracle_buf.remaining(),
                    "remaining count didn't match"
                );

                // Put buf_list and oracle back into their original places.
                buf_list = buf_list_pinned.get_mut();
                oracle = oracle_pinned.get_mut();
            }
        }

        // Check general properties: remaining and has_remaining are the same.
        let buf_list_remaining = buf_list.remaining();
        let oracle_remaining = oracle.remaining();
        ensure!(
            buf_list_remaining == oracle_remaining,
            "remaining didn't match: buf_list {} == oracle {}",
            buf_list_remaining,
            oracle_remaining
        );

        let buf_list_has_remaining = buf_list.has_remaining();
        let oracle_has_remaining = oracle.has_remaining();
        ensure!(
            buf_list_has_remaining == oracle_has_remaining,
            "has_remaining didn't match: buf_list {} == oracle {}",
            buf_list_has_remaining,
            oracle_has_remaining
        );

        // Also check that the position is the same.
        let buf_list_position = buf_list.position();
        ensure!(
            buf_list_position == oracle.position(),
            "position didn't match: buf_list position {} == oracle position {}",
            buf_list_position,
            oracle.position(),
        );
        Self::assert_io_result_eq(buf_list.stream_position(), oracle.stream_position())
            .context("stream position didn't match")?;

        // Check that fill_buf returns an empty slice iff it is actually empty.
        let fill_buf = buf_list.fill_buf().expect("fill_buf never errors");
        if buf_list_position < num_bytes as u64 {
            ensure!(
                !fill_buf.is_empty(),
                "fill_buf cannot be empty since buf_list.position {} < num_bytes {}",
                buf_list_position,
                num_bytes,
            );
        } else {
            ensure!(
                fill_buf.is_empty(),
                "fill_buf must be empty since buf_list.position {} >= num_bytes {}",
                buf_list_position,
                num_bytes,
            )
        }

        // Finally, check that the internal invariants are upheld.
        buf_list.assert_invariants()?;

        Ok(())
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
