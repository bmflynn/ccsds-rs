pub struct CADU {
    pub asm: Vec<u8>,
    pub data: Vec<u8>,
}

impl CADU {
    /// Standard CCSDS attached synchronization marker
    pub const ASM: [u8; 4] = [0x1a, 0xcf, 0xfc, 0x1d];

    pub fn new(asm: Vec<u8>, data: Vec<u8>) -> Self {
        CADU { asm, data }
    }
}
