mod bytes;
mod pn;
mod rs;
mod synchronizer;

pub use synchronizer::{
    ASM,
    SyncError,
};


pub const TERRA: i32 = 42;
pub const AQUA: i32 = 154;
pub const SNPP: i32 = 157;
pub const NOAA20: i32 = 159;
pub const NOAA21: i32 = 177;
pub const NOAA22: i32 = 178;
pub const NOAA23: i32 = 179;

pub struct RSConfig {
    pub interleave: i32,
    pub correctable: i32,
    pub vfill_length: i32,
}

pub struct Framing {
    pub asm: Vec<u8>,
    /// Length of the frame contained within a CADU, not including the ASM or 
    /// any Reed-solomon parity bytes.
    pub frame_length: i32,
    pub pseudo_randomized: bool,
    pub izone_length: i32,
    pub trailer_length: i32,
    pub rs: Option<RSConfig>,
}

impl Framing {
    /// Returns the expected length of CADU which will include the ASM and the 
    /// length of the Reed-Solomon code block.
    ///
    /// So, for example, with standard RS(223/255) with an interleave of 4 this
    /// will return 1024, which is 4 bytes for the ASM, 128 bits for the Reed-Solomon
    /// code block and the frame bytes.
    pub fn cadu_len(&self) -> i32 {
        let rslen = match self.rs {
            Some(ref rs) => {
                (rs.interleave * rs.correctable) / 2
            },
            None => 0,
        };
        self.asm.len() as i32 + self.frame_length + rslen
    }
}

/// Get spacecraft framing info for a particular spacecraft.
pub fn get_framing(scid: i32) -> Option<Framing> {
    match scid {
        TERRA | AQUA | SNPP | NOAA20 => Some(Framing {
            asm: ASM.to_vec(),
            frame_length: 892,
            pseudo_randomized: true,
            izone_length: 0,
            trailer_length: 0,
            rs: Some(RSConfig {
                interleave: 4,
                correctable: 16,
                vfill_length: 0,
            }),
        }),
        NOAA21 | NOAA22 | NOAA23 => Some(Framing {
            asm: ASM.to_vec(),
            frame_length: 1115,
            pseudo_randomized: true,
            izone_length: 0,
            trailer_length: 0,
            rs: Some(RSConfig {
                interleave: 4,
                correctable: 16,
                vfill_length: 0,
            }),
        }),
        _ => None,
    }
}
