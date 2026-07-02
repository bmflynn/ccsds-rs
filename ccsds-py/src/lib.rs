mod frame;
mod packet;
mod timecode;

pub use frame::*;
pub use packet::*;
pub use timecode::*;

use pyo3::prelude::*;

#[pymodule]
#[pyo3(name = "ccsds")]
#[pyo3(module = "ccsds")]
fn ccsdspy(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py_decode_packets, m)?)?;
    m.add_function(wrap_pyfunction!(decode_packet_groups, m)?)?;
    m.add_function(wrap_pyfunction!(decode_timecode, m)?)?;
    m.add_function(wrap_pyfunction!(decode_eos_timecode, m)?)?;
    m.add_function(wrap_pyfunction!(decode_jpss_timecode, m)?)?;
    m.add_class::<Packet>()?;
    m.add_class::<PrimaryHeader>()?;
    m.add_class::<PacketGroup>()?;
    m.add_class::<Timecode>()?;
    m.add_class::<TimecodeFormat>()?;

    m.add_function(wrap_pyfunction!(derandomize, m)?)?;
    m.add_function(wrap_pyfunction!(decode_frames, m)?)?;
    m.add_class::<VCDUHeader>()?;
    m.add_class::<Frame>()?;
    m.add_class::<FramingConfig>()?;
    m.add_class::<Integrity>()?;

    Ok(())
}
