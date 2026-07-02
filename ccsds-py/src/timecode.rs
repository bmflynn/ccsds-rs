use pyo3::prelude::*;

pub use ccsds::timecode::Format as TimecodeFormat;

#[pyclass(frozen)]
pub struct Timecode {
    #[pyo3(get)]
    epoch: hifitime::Epoch,
}

#[pymethods]
impl Timecode {
    pub fn __repr__(&self) -> String {
        self.__str__()
    }

    // str rep that is loadable by datetime.fromisoformat
    pub fn __str__(&self) -> String {
        self.epoch.to_string()
    }

    /// Returns seconds since Jan 1, 1970
    ///
    /// Returns:
    ///     A hifitime.Epoch instance representing this timecode.
    pub fn unix_seconds(&self) -> f64 {
        self.epoch.to_unix_seconds()
    }

    /// Extract timecode as a `datetime.datetime`.
    ///
    /// Returns:
    ///     A datetime with its tzinfo set to `datetime.timezone.utc`.
    ///
    ///     Note, that datetime does not support time anything more than microsecond precision
    ///     and any nanoseconds present are silently dropped.
    pub fn datetime<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let datetime = py.import_bound("datetime")?;
        let utc = datetime.getattr("timezone")?.getattr("utc")?;
        datetime
            .getattr("datetime")?
            .getattr("fromtimestamp")?
            .call1((self.epoch.to_unix_seconds(), utc))
    }
}

/// Decode the provided data into a `Timecode`.
///
/// Args:
///     format:
///         A Format instance specifying the timecode parameters used for decoding
///     buf:
///         Data to decode. Must be at least as long as the format requires. decoding
///         will always start at index 0.
///
/// Returns:
///     Timecode
///
/// Raises:
///     ValueError: If `buf` cannot meet the format requirements
#[pyfunction]
pub fn decode_timecode(format: TimecodeFormat, buf: &[u8]) -> PyResult<Timecode> {
    Ok(Timecode {
        epoch: ccsds::timecode::decode(&format, buf)?,
    })
}

/// Decode NASA EOS telemetry CUC timecode
///
/// See decode_timecode
#[pyfunction(name = "_decode_eos_timecode")]
pub fn decode_eos_timecode(buf: &[u8]) -> PyResult<Timecode> {
    let format = TimecodeFormat::Cuc {
        num_coarse: 2,
        num_fine: 4,
        fine_mult: Some(15200.0),
    };
    Ok(Timecode {
        epoch: ccsds::timecode::decode(&format, buf)?,
    })
}

/// Decode JPSS CDS timecode.
///
/// See decode_timecode
#[pyfunction(name = "_decode_jpss_timecode")]
pub fn decode_jpss_timecode(buf: &[u8]) -> PyResult<Timecode> {
    let format = TimecodeFormat::Cds {
        num_day: 2,
        num_submillis: 2,
    };
    Ok(Timecode {
        epoch: ccsds::timecode::decode(&format, buf)?,
    })
}
