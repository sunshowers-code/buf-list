// Copyright (c) The buf-list Contributors
// SPDX-License-Identifier: Apache-2.0

// Property-based tests for Cursor.

use crate::BufList;
use anyhow::{bail, ensure, Context, Result};
use bytes::{Buf, Bytes};
use proptest::prelude::*;
use std::{
    fmt,
    io::{self, BufRead, IoSliceMut, Read, Seek, SeekFrom},
};
use test_strategy::{proptest, Arbitrary};

/// Assert that buf_list's cursor behaves identically to std::io::Cursor.
#[proptest]
fn proptest_cursor_ops(
    #[strategy(buf_list_strategy())] buf_list: BufList,
    #[strategy(cursor_ops_strategy())] ops: Vec<CursorOp>,
) {
    let bytes = buf_list.clone().copy_to_bytes(buf_list.remaining());
    let mut buf_list_cursor = crate::Cursor::new(&buf_list);
    let mut upstream_cursor = io::Cursor::new(bytes.as_ref());

    eprintln!("\n**** start!");

    for (index, cursor_op) in ops.into_iter().enumerate() {
        // apply_and_compare prints out the rest of the line.
        eprint!("** index {}, operation {:?}: ", index, cursor_op);
        cursor_op
            .apply_and_compare(&mut buf_list_cursor, &mut upstream_cursor)
            .with_context(|| format!("for index {}", index))
            .unwrap();
    }
    eprintln!("**** success");
}

fn buf_list_strategy() -> impl Strategy<Value = BufList> {
    prop::collection::vec(prop::collection::vec(any::<u8>(), 1..128), 0..32)
        .prop_map(|chunks| chunks.into_iter().map(Bytes::from).collect())
}

#[derive(Arbitrary, Clone, Debug)]
enum CursorOp {
    SetPosition(prop::sample::Index),
    SeekStart(prop::sample::Index),
    SeekEnd(prop::sample::Index),
    SeekCurrent(prop::sample::Index),
    Read(prop::sample::Index),
    ReadVectored(
        #[strategy(prop::collection::vec(any::<prop::sample::Index>(), 0..8))]
        Vec<prop::sample::Index>,
    ),
    ReadExact(prop::sample::Index),
    // fill_buf can't be tested here because upstream is a contiguous block. Instead, we check its
    // return value separately.
    Consume(prop::sample::Index),
    // No need to test futures03 imps since they're simple wrappers around the main imps.
    #[cfg(feature = "tokio1")]
    PollRead {
        capacity: prop::sample::Index,
        filled: prop::sample::Index,
    },
}

impl CursorOp {
    fn apply_and_compare(
        self,
        #[allow(unused_mut)] mut buf_list: &mut crate::Cursor<&BufList>,
        #[allow(unused_mut)] mut upstream: &mut io::Cursor<&[u8]>,
    ) -> Result<()> {
        let num_bytes = buf_list.get_ref().num_bytes();
        match self {
            Self::SetPosition(index) => {
                // Allow going past the end of the list a bit.
                let index = index.index(1 + num_bytes * 5 / 4) as u64;
                eprintln!("set position: {}", index);

                buf_list.set_position(index);
                upstream.set_position(index);
            }
            Self::SeekStart(index) => {
                // Allow going past the end of the list a bit.
                let style = SeekFrom::Start(index.index(1 + num_bytes * 5 / 4) as u64);
                eprintln!("style: {:?}", style);

                let buf_list_pos = buf_list.seek(style);
                let upstream_pos = upstream.seek(style);
                Self::assert_io_error_eq(buf_list_pos, upstream_pos)
                    .context("operation result didn't match")?;
            }
            Self::SeekEnd(index) => {
                // Allow going past the beginning and end of the list a bit.
                let index = index.index(1 + num_bytes * 3 / 2) as i64;
                let style = SeekFrom::End(index - (1 + num_bytes * 5 / 4) as i64);
                eprintln!("style: {:?}", style);

                let buf_list_pos = buf_list.seek(style);
                let upstream_pos = upstream.seek(style);
                Self::assert_io_error_eq(buf_list_pos, upstream_pos)
                    .context("operation result didn't match")?;
            }
            Self::SeekCurrent(index) => {
                let index = index.index(1 + num_bytes * 3 / 2) as i64;
                // Center the index at roughly 0.
                let style = SeekFrom::Current(index - (num_bytes * 3 / 4) as i64);
                eprintln!("style: {:?}", style);

                let buf_list_pos = buf_list.seek(style);
                let upstream_pos = upstream.seek(style);
                Self::assert_io_error_eq(buf_list_pos, upstream_pos)
                    .context("operation result didn't match")?;
            }
            Self::Read(index) => {
                let buf_size = index.index(1 + num_bytes * 5 / 4);
                eprintln!("buf_size: {}", buf_size);

                // Must initialize the whole vec here so &mut returns the whole buffer -- can't use
                // with_capacity!
                let mut buf_list_buf = vec![0u8; buf_size];
                let mut upstream_buf = vec![0u8; buf_size];

                let buf_list_nread = buf_list.read(&mut buf_list_buf);
                let upstream_nread = upstream.read(&mut upstream_buf);
                Self::assert_io_error_eq(buf_list_nread, upstream_nread)
                    .context("operation result didn't match")?;
                ensure!(buf_list_buf == upstream_buf, "read buffer matches");
            }
            Self::ReadVectored(indexes) => {
                // Build a bunch of IoSliceMuts.
                let mut buf_list_vecs: Vec<_> = indexes
                    .into_iter()
                    .map(|index| {
                        // Must initialize the whole vec here so &mut returns the whole buffer -- can't
                        // use with_capacity!
                        let buf_size = index.index(1 + num_bytes);
                        vec![0u8; buf_size]
                    })
                    .collect();
                let mut upstream_vecs = buf_list_vecs.clone();

                let mut buf_list_slices: Vec<_> = buf_list_vecs
                    .iter_mut()
                    .map(|v| IoSliceMut::new(v))
                    .collect();
                let mut upstream_slices: Vec<_> = upstream_vecs
                    .iter_mut()
                    .map(|v| IoSliceMut::new(v))
                    .collect();

                let buf_list_res = buf_list.read_vectored(&mut buf_list_slices);
                let upstream_res = upstream.read_vectored(&mut upstream_slices);
                Self::assert_io_error_eq(buf_list_res, upstream_res)
                    .context("operation result didn't match")?;

                // Also check that the slices read match exactly.
                ensure!(
                    buf_list_vecs == upstream_vecs,
                    "read vecs didn't match: buf_list: {:?} == upstream: {:?}",
                    buf_list_vecs,
                    upstream_vecs
                );
            }
            Self::ReadExact(index) => {
                let buf_size = index.index(1 + num_bytes * 5 / 4);
                eprintln!("buf_size: {}", buf_size);

                // Must initialize the whole vec here so &mut returns the whole buffer -- can't use
                // with_capacity!
                let mut buf_list_buf = vec![0u8; buf_size];
                let mut upstream_buf = vec![0u8; buf_size];

                let buf_list_res = buf_list.read_exact(&mut buf_list_buf);
                let upstream_res = upstream.read_exact(&mut upstream_buf);
                Self::assert_io_error_eq(buf_list_res, upstream_res)
                    .context("operation result didn't match")?;
                ensure!(buf_list_buf == upstream_buf, "read buffer matches");
            }
            Self::Consume(index) => {
                let amt = index.index(1 + num_bytes * 5 / 4);
                eprintln!("amt: {}", amt);

                buf_list.consume(amt);
                upstream.consume(amt);
            }
            #[cfg(feature = "tokio1")]
            Self::PollRead { capacity, filled } => {
                use std::{mem::MaybeUninit, pin::Pin, task::Poll};
                use tokio::io::{AsyncRead, ReadBuf};

                let capacity = capacity.index(1 + num_bytes * 5 / 4);
                let mut buf_list_vec = vec![MaybeUninit::uninit(); capacity];
                let mut upstream_vec = buf_list_vec.clone();

                let mut buf_list_buf = ReadBuf::uninit(&mut buf_list_vec);

                // Fill up the first bytes of the vector. This uses capacity + 1 so that we can
                // sometimes fill up the whole buffer.
                let filled_index = filled.index(capacity + 1);
                let fill_vec = vec![0u8; filled_index];
                buf_list_buf.put_slice(&fill_vec);

                let mut upstream_buf = ReadBuf::uninit(&mut upstream_vec);
                upstream_buf.put_slice(&fill_vec);

                eprintln!("capacity: {}, filled_index: {}", capacity, filled_index);

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

                let mut upstream_pinned = Pin::new(upstream);
                let upstream_res = match upstream_pinned
                    .as_mut()
                    .poll_read(&mut context, &mut upstream_buf)
                {
                    Poll::Ready(res) => res,
                    Poll::Pending => unreachable!("upstream cursor never returns pending"),
                };

                Self::assert_io_error_eq(buf_list_res, upstream_res)
                    .context("result didn't match")?;
                ensure!(
                    buf_list_buf.filled() == upstream_buf.filled(),
                    "filled section didn't match"
                );
                ensure!(
                    buf_list_buf.remaining() == upstream_buf.remaining(),
                    "remaining count didn't match"
                );

                // Put buf_list and upstream back into their original places.
                buf_list = buf_list_pinned.get_mut();
                upstream = upstream_pinned.get_mut();
            }
        }

        // Also check that the position is the same.
        let buf_list_position = buf_list.position();
        ensure!(
            buf_list_position == upstream.position(),
            "position didn't match: buf_list position {} == upstream position {}",
            buf_list_position,
            upstream.position(),
        );
        Self::assert_io_error_eq(buf_list.stream_position(), upstream.stream_position())
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

    fn assert_io_error_eq<T: Eq + fmt::Debug>(
        buf_list_res: io::Result<T>,
        upstream_res: io::Result<T>,
    ) -> Result<()> {
        match (buf_list_res, upstream_res) {
            (Ok(buf_value), Ok(upstream_value)) => {
                ensure!(
                    buf_value == upstream_value,
                    "value didn't match: buf_list value {:?} == upstream value {:?}",
                    buf_value,
                    upstream_value
                );
            }
            (Ok(buf_value), Err(upstream_err)) => {
                bail!(
                    "BufList value Ok({:?}) is not the same as upstream error Err({})",
                    buf_value,
                    upstream_err,
                );
            }
            (Err(buf_err), Ok(upstream_value)) => {
                bail!(
                    "BufList error ({}) is not the same as upstream value ({:?})",
                    buf_err,
                    upstream_value
                )
            }
            (Err(buf_err), Err(upstream_err)) => {
                // The kinds should match.
                ensure!(
                    buf_err.kind() == upstream_err.kind(),
                    "error kind didn't match: buf_list {:?} == upstream {:?}",
                    buf_err.kind(),
                    upstream_err.kind()
                );
            }
        }

        Ok(())
    }
}

fn cursor_ops_strategy() -> impl Strategy<Value = Vec<CursorOp>> {
    prop::collection::vec(any::<CursorOp>(), 0..256)
}
