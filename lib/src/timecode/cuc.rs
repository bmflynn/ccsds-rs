use super::error::{Error, Result};

/// Deocde a CCSDS Unsegmented Time Code.
///
/// `coarse` is the number of bytes to use for the coarse time component, `fine` is the number of
/// bytes used for the fine time component. Both values support up to 8 bytes.
///
/// `mult` is an optional multiplier to convert the decoded fine
///
/// # Errors
/// [Error::Unsupported] if `coarse` or `fine` are >= 8.
/// [Error::Other] if the `buf` does not contain enough bytes to decode timecode.
pub fn decode(
    coarse: usize,
    fine: usize,
    mult: Option<u64>,
    buf: &[u8],
) -> Result<super::Timecode> {
    if coarse > 8 {
        return Err(Error::Invalid("CUC coarse must be < 8".to_string()));
    }
    if fine > 8 {
        return Err(Error::Invalid("CUC fine must be < 8".to_string()));
    }
    if buf.len() < coarse + fine {
        return Err(Error::Other(crate::Error::TooShort(
            coarse + fine,
            buf.len(),
        )));
    }
    let (x, rest) = buf.split_at(coarse);
    let mut coarse_bytes = vec![0u8; 8 - coarse];
    coarse_bytes.extend(x);
    let (x, _) = rest.split_at(fine);
    let mut fine_bytes = vec![0u8; 8 - fine];
    fine_bytes.extend(x);

    let secs = u64::from_be_bytes(coarse_bytes.try_into().unwrap());
    let days = u32::try_from(secs / 86400).unwrap();

    let mut picos = u64::from_be_bytes(fine_bytes.try_into().unwrap());
    if let Some(mult) = mult {
        picos *= mult;
    }
    let picos = picos + (secs % 86400) * 10u64.pow(12);

    Ok(super::Timecode { days, picos })
}
