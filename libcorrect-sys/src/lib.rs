//! Raw bindings to the Reed-Solomon part of [libcorrect](https://github.com/quiet/libcorrect).
//!
//! Only the Reed-Solomon sources are vendored. See `vendor/` and `LICENSE-libcorrect`.
#![allow(non_camel_case_types)]

/// Opaque libcorrect Reed-Solomon codec. Holds scratch buffers, so one instance must not be used
/// from more than one thread at a time.
#[repr(C)]
pub struct correct_reed_solomon {
    _private: [u8; 0],
}

/// `x^8 + x^7 + x^2 + x + 1`
pub const CORRECT_RS_PRIMITIVE_POLYNOMIAL_CCSDS: u16 = 0x187;

extern "C" {
    pub fn correct_reed_solomon_create(
        primitive_polynomial: u16,
        first_consecutive_root: u8,
        generator_root_gap: u8,
        num_roots: usize,
    ) -> *mut correct_reed_solomon;

    pub fn correct_reed_solomon_encode(
        rs: *mut correct_reed_solomon,
        msg: *const u8,
        msg_length: usize,
        encoded: *mut u8,
    ) -> isize;

    /// Returns the number of bytes written to `msg`, or -1 on failure.
    pub fn correct_reed_solomon_decode(
        rs: *mut correct_reed_solomon,
        encoded: *const u8,
        encoded_length: usize,
        msg: *mut u8,
    ) -> isize;

    pub fn correct_reed_solomon_destroy(rs: *mut correct_reed_solomon);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_errors() {
        unsafe {
            let rs =
                correct_reed_solomon_create(CORRECT_RS_PRIMITIVE_POLYNOMIAL_CCSDS, 112, 11, 32);
            assert!(!rs.is_null());
            let msg: Vec<u8> = (0..223u32).map(|i| (i * 7 + 3) as u8).collect();
            let mut enc = vec![0u8; 255];
            assert_eq!(
                correct_reed_solomon_encode(rs, msg.as_ptr(), 223, enc.as_mut_ptr()),
                255
            );
            for i in 0..16 {
                enc[i * 5] ^= 0xa5;
            }
            let mut out = vec![0u8; 223];
            assert_eq!(
                correct_reed_solomon_decode(rs, enc.as_ptr(), 255, out.as_mut_ptr()),
                223
            );
            assert_eq!(out, msg);
            correct_reed_solomon_destroy(rs);
        }
    }
}
