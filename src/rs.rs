mod dual_basis;
mod gf;

use thiserror::Error;

// Symbols per code word
pub const N: u8 = 255;
// Bits per symbol
pub const J: u8 = 8;
// Common irreducible primative polynomial x^8 + x^7 + x^2 + x + 1
pub const PRIM: i32 = 391;
// Primative element: alpha 11
pub const GEN: u8 = 173;
// FIrst consecutive root in g(x): 128-E
pub const FCR: i32 = 112;

pub const PARITY_LEN: usize = 32;

pub enum RSState {
    Ok(i32),
    Uncorrectable,
    NotPerformed,
}

#[derive(Error, Debug)]
pub enum RSError {
    #[error("could not perform RS with provided input")]
    InvalidInput,
    #[error("algorithm failure")]
    AlgorithmFailure(String),
    #[error("uncorrectable")]
    Uncorrectable(String),
}

pub fn deinterlace(data: Vec<u8>, interlacing: i32) -> Result<Vec<[u8; 255]>, RSError> {
    if data.len() % interlacing as usize != 0 {
        return Err(RSError::InvalidInput);
    }
    let mut zult: Vec<[u8; 255]> = Vec::new();
    for _ in 0..interlacing {
        zult.push([0u8; 255]);
    }
    for j in 0..data.len() as usize {
        zult[j % interlacing as usize][j / interlacing as usize] = data[j]
    }
    Ok(zult)
}

fn correct_errata(input: &[u8], synd: &[u8], errpos: &[i32]) -> Result<Vec<u8>, RSError> {
    let mut coef_pos = vec![0i32; errpos.len()];
    for (i, p) in errpos.iter().enumerate() {
        coef_pos[i] = input.len() as i32 - 1 - p;
    }

    let errloc = find_errata_locator(&coef_pos[..]);
    let mut rev_synd = synd.to_owned();
    rev_synd.reverse();
    let erreval = find_error_evaluator(&rev_synd, &errloc, errloc.len() as i32 - 1);

    let mut x = vec![0u8; coef_pos.len()];
    for (i, p) in coef_pos.iter().enumerate() {
        x[i] = gf::pow(GEN as u8, -(N as i32 - p));
    }

    let mut e = vec![0u8; input.len()];
    for (i, xi) in x.iter().enumerate() {
        let xi_inv = gf::inv(*xi);
        let mut errloc_prime_tmp: Vec<u8> = Vec::new();
        for j in 0..x.len() {
            if j != i {
                errloc_prime_tmp.push(1 ^ gf::mult(xi_inv, x[j]));
            }
        }
        let mut errloc_prime = 1u8;
        for c in errloc_prime_tmp.iter() {
            errloc_prime = gf::mult(errloc_prime, *c);
        }

        let mut y = gf::poly_eval(&erreval, xi_inv);
        y = gf::mult(gf::pow(*xi, 1 - FCR), y);

        if errloc_prime == 0 {
            return Err(RSError::AlgorithmFailure("failed to find error magnitude".to_owned()));
        }

        e[errpos[i] as usize] = gf::div(y, errloc_prime);
    }

    let zult = &gf::poly_add(&input, &e);
    Ok(zult.to_vec())
}

fn find_errata_locator(errpos: &[i32]) -> Vec<u8> {
    let mut errloc: Vec<u8> = Vec::with_capacity(errpos.len());
    errloc.push(1);
    for p in errpos.iter() {
        let x = vec![gf::pow(GEN as u8, *p), 0];
        let y = gf::poly_add(&vec![1u8][..], &x[..]);
        errloc = gf::poly_mult(&errloc, &y);
    }
    errloc
}

fn find_error_evaluator(synd: &[u8], errloc: &[u8], n: i32) -> Vec<u8> {
    let mut divisor: Vec<u8> = vec![0u8; n as usize + 2];
    divisor[0] = 1;
    let (_, rem) = gf::poly_div(&gf::poly_mult(&synd, &errloc), &divisor);
    rem
}

fn find_errors(errloc: &[u8]) -> Result<Vec<i32>, RSError> {
    let num_errs = errloc.len() - 1;
    let mut errpos: Vec<i32> = Vec::with_capacity(num_errs);
    let n = N as i32;
    for i in 0..n {
        if gf::poly_eval(errloc, gf::pow(GEN, i as i32)) == 0 {
            errpos.push(n - 1 - i);
        }
    }
    if errpos.len() != num_errs {
        return Err(RSError::AlgorithmFailure(format!(
            "wrong number of errors; expected {:?}, got {:?}",
            num_errs,
            errpos.len()
        )
        .to_owned()));
    }
    Ok(errpos)
}

fn find_error_locator(synd: &[u8], parity_len: usize) -> Result<Vec<u8>, RSError> {
    let mut errloc = vec![1u8];
    let mut oldloc = vec![1u8];
    let mut synd_shift = 0;
    if synd.len() > parity_len {
        synd_shift = synd.len() - parity_len;
    }
    for i in 0..parity_len {
        let k = i as usize + synd_shift;
        let mut delta = synd[k];
        for j in 1..errloc.len() {
            delta ^= gf::mult(errloc[errloc.len() - j - 1], synd[k - j]);
        }
        oldloc.push(0);
        if delta != 0 {
            if oldloc.len() > errloc.len() {
                let newloc = gf::poly_scale(&oldloc, delta);
                oldloc = gf::poly_scale(&errloc, gf::inv(delta));
                errloc = newloc;
            }
            errloc = gf::poly_add(&errloc, &gf::poly_scale(&oldloc, delta));
        }
    }

    while errloc.len() > 0 && errloc[0] == 0 {
        errloc = errloc[1..].to_vec();
    }
    let num_errs = errloc.len() - 1;
    if num_errs * 2 > parity_len {
        return Err(RSError::Uncorrectable(format!(
            "too many errors; expected no more than {:?}, found {:?}",
            parity_len / 2,
            num_errs
        )));
    }
    Ok(errloc)
}

fn forney_syndromes(synd: &[u8], pos: &[i32], nmess: i32) -> Vec<u8> {
    let mut erase_pos_rev = vec![0i32; pos.len()];
    for (i, p) in pos.iter().enumerate() {
        erase_pos_rev[i] = nmess - 1 - p;
    }
    let mut fsynd: Vec<u8> = Vec::with_capacity(synd.len() - 1);
    fsynd.extend_from_slice(&synd[1..]);
    for i in 0..pos.len() {
        let x = gf::pow(GEN as u8, erase_pos_rev[i]);
        for j in 0..fsynd.len() - 1 {
            fsynd[j] = gf::mult(fsynd[j], x) ^ fsynd[j + 1];
        }
    }
    fsynd
}

fn calc_syndromes(input: &[u8], parity_len: usize) -> Vec<u8> {
    let mut synd: Vec<u8> = vec![0u8; parity_len + 1];
    for i in 0..parity_len {
		let p = gf::pow(GEN, i as i32 + FCR);
        synd[i + 1] = gf::poly_eval(&input, p);
    }
    synd
}

/// Correct a Reed-Solomon code block.
///
/// Decoding is performed according to the CCSDS Reed-Solomon coding standard documented 
/// in CCSDS 131.0-B-4: TM Synchronization and Channel Coding.
///
/// # Errors 
/// * RSError::InvalidInput, if the input code block is not valid for CCSDS standard RS
/// * RSError::Uncorrectable, if the code block contains too many errors to correct
/// * RSError::AlgorithmFailure, if RS could not be performed for an internal reason
pub fn correct_message(input: &[u8]) -> Result<Vec<u8>, RSError> {
    if input.len() != N as usize {
        return Err(RSError::InvalidInput);
    }
    let input = dual_basis::to_conv(input);
    let mut out = input.clone();

    let synd = calc_syndromes(&out, PARITY_LEN);
    let max = synd.iter().max().unwrap();
    // if there are no non-zero elements there are no errors
    if *max == 0 {
        return Ok(out);
    }

    let fsynd = forney_syndromes(&synd, &[], out.len() as i32);
    let errloc = find_error_locator(&fsynd[..], PARITY_LEN)?;

    let mut errloc_rev = errloc.clone();
    errloc_rev.reverse();
    let errpos = find_errors(&errloc_rev[..])?;

    out = correct_errata(&out, &synd, &errpos)?;

    let synd = calc_syndromes(&out, PARITY_LEN);
    if *synd.iter().max().unwrap() > 0 {
        return Err(RSError::Uncorrectable(
            "failed to correct all errors".to_owned(),
        ));
    }

    Ok(dual_basis::to_dual(&out))
}

/// Return true if the input code block contains 1 or more errors.
pub fn has_errors(msg: &[u8]) -> bool {
    let msg = dual_basis::to_conv(msg);
    let mut x = 0;
    for i in calc_syndromes(&msg[..], PARITY_LEN) {
        if i > x {
            x = i;
        }
    }
    x != 0
}

#[cfg(test)]
mod tests {
    use super::*;

	const FIXTURE_MSG: &[u8; 255] = &[
		0x67, 0xc4, 0x6b, 0xa7, 0x3e, 0xbe, 0x4c, 0x33, 0x6c, 0xb2, 0x23, 0x3a, 0x74,
		0x06, 0x2b, 0x18, 0xab, 0xb8, 0x09, 0xe6, 0x7d, 0xaf, 0x5d, 0xe5, 0xdf, 0x76,
		0x25, 0x3f, 0xb9, 0x14, 0xee, 0xec, 0xd1, 0xa3, 0x39, 0x5f, 0x38, 0x68, 0xf0,
		0x26, 0xa6, 0x8a, 0xcb, 0x09, 0xaf, 0x4e, 0xf8, 0x93, 0xf7, 0x45, 0x4b, 0x0d,
		0xa9, 0xb8, 0x74, 0x0e, 0xf3, 0xc7, 0xed, 0x6e, 0xa3, 0x0f, 0xf6, 0x79, 0x94,
		0x16, 0xe2, 0x7f, 0xad, 0x91, 0x91, 0x04, 0xac, 0xa4, 0xae, 0xb4, 0x51, 0x76,
		0x2f, 0x62, 0x03, 0x5e, 0xa1, 0xe5, 0x5c, 0x45, 0xf8, 0x1f, 0x7a, 0x7b, 0xe8,
		0x35, 0xd8, 0xcc, 0x51, 0x0e, 0xae, 0x3a, 0x2a, 0x64, 0x1d, 0x03, 0x10, 0xcd,
		0x18, 0xe6, 0x7f, 0xef, 0xba, 0xd9, 0xe8, 0x98, 0x47, 0x82, 0x9c, 0xa1, 0x58,
		0x47, 0x25, 0xdf, 0x41, 0xd2, 0x01, 0x62, 0x3c, 0x24, 0x88, 0x90, 0xe9, 0xd7,
		0x38, 0x1b, 0xa0, 0xa2, 0xb4, 0x23, 0xea, 0x7e, 0x58, 0x0d, 0xf4, 0x61, 0x24,
		0x14, 0xb0, 0x41, 0x90, 0x0c, 0xb7, 0xbb, 0x5c, 0x59, 0x1b, 0xc6, 0x69, 0x24,
		0x0f, 0xb6, 0x0e, 0x14, 0xa1, 0xb1, 0x8e, 0x48, 0x0f, 0x17, 0x1d, 0xfb, 0x0f,
		0x38, 0x42, 0xe3, 0x24, 0x58, 0xab, 0x82, 0xa8, 0xfd, 0xdf, 0xac, 0x68, 0x93,
		0x3d, 0x0d, 0x8f, 0x50, 0x52, 0x44, 0x6c, 0xba, 0xd3, 0x51, 0x99, 0x9c, 0x3e,
		0xad, 0xd5, 0xa8, 0xd7, 0x9d, 0xc7, 0x7f, 0x9f, 0xc9, 0x2a, 0xac, 0xe5, 0xc2,
		0xcd, 0x9a, 0x9b, 0xfa, 0x2d, 0x72, 0xab, 0x6b, 0xa4, 0x6b, 0x8b, 0x7d, 0xfa,
		0x6c, 0x83, 0x63, 0x77, 0x9f, 0x4e, 0x9a, 0x20, 0x35, 0xd2, 0x91, 0xce, 0xf4,
		0x21, 0x1a, 0x97, 0x3c, 0x1a, 0x15, 0x9d, 0xfc, 0x98, 0xba, 0x72, 0x1b, 0x9a,
		0xa2, 0xe9, 0xc9, 0x46, 0x68, 0xce, 0xad, 0x27,
    ];

    #[test]
    fn test_calc_syndromes() {
        const EXPECTED: &[u8] = &[
            0x00, 
            0xb7, 0xd5, 0x62, 0x7b, 0xf5, 0xa0, 0x52, 0x91, 0xc1, 0xd2, 0x97, 0xd0, 0x40, 0x68, 0x59, 0x0d, 
            0xcb, 0xc0, 0x84, 0x84, 0x68, 0xa6, 0xd9, 0x79, 0xf9, 0xad, 0x4c, 0x81, 0x9f, 0x14, 0x2f, 0x78, 
        ];

        let zult = calc_syndromes(FIXTURE_MSG, PARITY_LEN);

        for ((i, z), e) in zult.iter().enumerate().zip(EXPECTED.iter()) {
            assert_eq!(z, e, "not all elements equal: expected {}, got {} at index {}\n{:?}", e, z, i, zult); 
        }
    }

    #[test]
    fn test_correct_message() {
        let mut msg = FIXTURE_MSG.clone();

        // corrupt the message
        msg[0] = 0;
        msg[2] = 2;
        msg[4] = 2;
        msg[6] = 2;

        assert!(has_errors(&msg), "expected message to have errors"); 

        correct_message(&msg).expect("expected no error");
    }   

    #[bench]
    fn bench_correct_message(b: &mut test::Bencher) {

        b.iter(|| {
            let mut msg = test::black_box(FIXTURE_MSG.clone());

            // corrupt the message
            msg[0] = 0;
            msg[2] = 2;
            msg[4] = 2;
            msg[6] = 2;
            correct_message(&msg).expect("decode failed")
        });
    }
}

