//! Reed-Solomon (255, 223) backend using [libcorrect](https://github.com/quiet/libcorrect).
//!
//! Enabled by the `libcorrect` cargo feature. See [LibcorrectReedSolomon].

use std::cell::RefCell;
use std::ptr::NonNull;

use libcorrect_sys as sys;
use rs2::dual_basis::{to_conv, to_dual};

use super::reed_solomon::{deinterleave, Integrity, ReedSolomon};
use crate::{Error, Result};

const N: usize = 255;
const PARITY_LEN: usize = 32;
const MSG_LEN: usize = N - PARITY_LEN;
const FIRST_ROOT: u8 = 112;
const ROOT_GAP: u8 = 11;

/// Owned libcorrect codec for the CCSDS code. Not `Sync`; each thread uses its own.
struct Codec(NonNull<sys::correct_reed_solomon>);

impl Codec {
    fn new() -> Self {
        // SAFETY: plain constructor call, null is checked below.
        let ptr = unsafe {
            sys::correct_reed_solomon_create(
                sys::CORRECT_RS_PRIMITIVE_POLYNOMIAL_CCSDS,
                FIRST_ROOT,
                ROOT_GAP,
                PARITY_LEN,
            )
        };
        Codec(NonNull::new(ptr).expect("libcorrect failed to allocate a Reed-Solomon codec"))
    }

    /// Decode a conventional-basis 255 byte codeblock. Returns the full corrected 255 byte
    /// codeblock (message plus parity) and whether it differed from the input, or `None` if
    /// the block could not be corrected.
    fn correct(&mut self, block: &[u8]) -> Option<([u8; N], bool)> {
        debug_assert_eq!(block.len(), N);
        let mut msg = [0u8; MSG_LEN];
        // SAFETY: `block` is N bytes and `msg` has room for N - PARITY_LEN bytes, which is the
        // most this codec writes. The handle is valid and used by one thread (&mut self).
        let n = unsafe {
            sys::correct_reed_solomon_decode(
                self.0.as_ptr(),
                block.as_ptr(),
                N,
                msg.as_mut_ptr(),
            )
        };
        if n != MSG_LEN as isize {
            return None;
        }
        let mut out = [0u8; N];
        // SAFETY: `msg` is MSG_LEN bytes and `out` has room for N bytes.
        let n = unsafe {
            sys::correct_reed_solomon_encode(
                self.0.as_ptr(),
                msg.as_ptr(),
                MSG_LEN,
                out.as_mut_ptr(),
            )
        };
        if n != N as isize {
            return None;
        }
        // Compare the whole codeblock so errors confined to parity symbols also count.
        let changed = out.as_slice() != block;
        Some((out, changed))
    }
}

impl Drop for Codec {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `correct_reed_solomon_create` and is released once.
        unsafe { sys::correct_reed_solomon_destroy(self.0.as_ptr()) }
    }
}

thread_local! {
    static CODEC: RefCell<Option<Codec>> = const { RefCell::new(None) };
}

/// Run `f` with this thread's codec, creating it on first use.
fn with_codec<R>(f: impl FnOnce(&mut Codec) -> R) -> R {
    CODEC.with(|cell| {
        let mut slot = cell.borrow_mut();
        f(slot.get_or_insert_with(Codec::new))
    })
}

/// CCSDS Reed-Solomon (223/255) using libcorrect, as an alternative to
/// [DefaultReedSolomon](super::DefaultReedSolomon).
///
/// Behavior differences from [DefaultReedSolomon](super::DefaultReedSolomon):
/// * libcorrect does not report an error count or offer a detect-only check. Correction status
///   is found by comparing the corrected codeblock to the input. [Self::with_correction] set to
///   `false` therefore still runs the full decode and gives no CPU saving.
/// * [Self::with_detection] set to `false` skips the algorithm and only removes check symbols.
///
/// The codec holds scratch buffers, so one codec is created per thread on first use.
#[derive(Clone, Debug)]
pub struct LibcorrectReedSolomon {
    correct: bool,
    detect: bool,
    interleave: u8,
    remove_parity: bool,
    virtual_fill: usize,
}

impl LibcorrectReedSolomon {
    /// Create a new instance with the given interleave, no virtual fill, detection and
    /// correction enabled, and check symbols removed.
    pub fn new(interleave: u8) -> Self {
        Self {
            interleave,
            virtual_fill: 0,
            remove_parity: true,
            detect: true,
            correct: true,
        }
    }

    /// See [DefaultReedSolomon::with_virtual_fill](super::DefaultReedSolomon::with_virtual_fill).
    pub fn with_virtual_fill(mut self, num: usize) -> Self {
        self.virtual_fill = num;
        self
    }

    /// If `false`, blocks with errors get [Integrity::NotCorrected] and are returned unchanged.
    pub fn with_correction(mut self, enabled: bool) -> Self {
        self.correct = enabled;
        self
    }

    /// When `false` no RS algorithm is performed, check symbols are still removed.
    pub fn with_detection(mut self, enabled: bool) -> Self {
        self.detect = enabled;
        self
    }

    /// When true, remove the parity bytes from the end of the code block.
    pub fn with_remove_parity(mut self, enabled: bool) -> Self {
        self.remove_parity = enabled;
        self
    }
}

impl ReedSolomon for LibcorrectReedSolomon {
    fn perform(&self, cadu_dat: &[u8]) -> Result<(Integrity, Vec<u8>)> {
        let interleave = self.interleave as usize;
        if cadu_dat.len() + self.virtual_fill != N * interleave {
            return Err(Error::IntegrityAlgorithm(format!(
                "codeblock len={} cannot be corrected by this algorithm with interleave={}",
                cadu_dat.len(),
                self.interleave,
            )));
        }
        let parity_len = interleave * PARITY_LEN;

        // Virtual fill is zeros added to the start of the data
        let mut full = Vec::with_capacity(N * interleave);
        full.resize(self.virtual_fill, 0u8);
        full.extend_from_slice(cadu_dat);

        if !self.detect {
            let end = if self.remove_parity {
                full.len() - parity_len
            } else {
                full.len()
            };
            return Ok((Integrity::Ok, full[self.virtual_fill..end].to_vec()));
        }

        let blocks = deinterleave(&full, self.interleave);
        let mut corrected = vec![0u8; full.len()];
        let mut num_corrected = 0usize;

        let uncorrectable = with_codec(|codec| {
            for (idx, block) in blocks.iter().enumerate() {
                let conv = to_conv(block);
                let Some((fixed, changed)) = codec.correct(&conv) else {
                    return Some(Integrity::Uncorrectable);
                };
                if changed {
                    if !self.correct {
                        return Some(Integrity::NotCorrected);
                    }
                    num_corrected += 1;
                }
                for (j, b) in to_dual(&fixed).iter().enumerate() {
                    corrected[idx + j * interleave] = *b;
                }
            }
            None
        });

        match uncorrectable {
            // Same as DefaultReedSolomon: NotCorrected keeps all bytes, Uncorrectable drops
            // check symbols from the unmodified input.
            Some(Integrity::NotCorrected) => return Ok((Integrity::NotCorrected, full)),
            Some(other) => {
                let end = full.len() - parity_len;
                return Ok((other, full[..end].to_vec()));
            }
            None => {}
        }

        let end = if self.remove_parity {
            corrected.len() - parity_len
        } else {
            corrected.len()
        };
        let data = corrected[self.virtual_fill..end].to_vec();
        match num_corrected {
            0 => Ok((Integrity::Ok, data)),
            _ => Ok((Integrity::Corrected, data)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::DefaultReedSolomon;
    use super::*;

    fn make_cadu(interleave: u8, seed: u32) -> Vec<u8> {
        // Encode `interleave` distinct messages with libcorrect (conventional basis), convert
        // to dual basis, then interleave.
        let mut cadu = vec![0u8; N * interleave as usize];
        for i in 0..interleave as usize {
            let msg: Vec<u8> = (0..MSG_LEN as u32)
                .map(|j| (j.wrapping_mul(31).wrapping_add(seed + i as u32 * 17)) as u8)
                .collect();
            let mut enc = [0u8; N];
            with_codec(|c| unsafe {
                sys::correct_reed_solomon_encode(
                    c.0.as_ptr(),
                    msg.as_ptr(),
                    MSG_LEN,
                    enc.as_mut_ptr(),
                );
            });
            for (j, b) in to_dual(&enc).iter().enumerate() {
                cadu[i + j * interleave as usize] = *b;
            }
        }
        cadu
    }

    #[test]
    fn matches_rs2_clean_and_corrected() {
        for interleave in [4u8, 5] {
            let clean = make_cadu(interleave, 1);
            let lc = LibcorrectReedSolomon::new(interleave);
            let rs2 = DefaultReedSolomon::new(interleave);

            let (st, data) = lc.perform(&clean).unwrap();
            assert_eq!(st, Integrity::Ok);
            assert_eq!(rs2.perform(&clean).unwrap(), (st, data));

            // 16 byte errors in each interleaved codeblock is the maximum.
            let mut bad = clean.clone();
            for blk in 0..interleave as usize {
                for e in 0..16 {
                    bad[e * 15 * interleave as usize + blk] ^= 0x5a;
                }
            }
            let (st, data) = lc.perform(&bad).unwrap();
            assert_eq!(st, Integrity::Corrected);
            assert_eq!(rs2.perform(&bad).unwrap(), (st, data.clone()));
            let (_, clean_data) = lc.perform(&clean).unwrap();
            assert_eq!(data, clean_data);
        }
    }

    #[test]
    fn parity_only_error_is_corrected() {
        let mut cadu = make_cadu(4, 3);
        let last = cadu.len() - 1;
        cadu[last] ^= 0xff;
        let (st, _) = LibcorrectReedSolomon::new(4).perform(&cadu).unwrap();
        assert_eq!(st, Integrity::Corrected);
    }

    #[test]
    fn too_many_errors_is_uncorrectable() {
        let mut cadu = make_cadu(4, 5);
        // 40 errors in codeblock 0
        for e in 0..40 {
            cadu[e * 4] ^= 0xa5;
        }
        let (st, data) = LibcorrectReedSolomon::new(4).perform(&cadu).unwrap();
        assert_eq!(st, Integrity::Uncorrectable);
        assert_eq!(data.len(), cadu.len() - 4 * PARITY_LEN);
    }

    #[test]
    fn correction_disabled_reports_not_corrected() {
        let mut cadu = make_cadu(4, 7);
        cadu[100] ^= 1;
        let (st, data) = LibcorrectReedSolomon::new(4)
            .with_correction(false)
            .perform(&cadu)
            .unwrap();
        assert_eq!(st, Integrity::NotCorrected);
        assert_eq!(data.len(), cadu.len());
    }

    #[test]
    fn bad_length_is_error() {
        assert!(LibcorrectReedSolomon::new(4).perform(&[0u8; 100]).is_err());
    }
}
