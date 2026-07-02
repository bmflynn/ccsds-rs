use std::{
    fs::File,
    io::{stdin, BufReader, Read},
    net::TcpStream,
    os::unix::net::UnixStream,
};

use ccsds::framing::{DefaultDerandomizer, Derandomizer, RsOpts};
pub use ccsds::framing::{Frame, Integrity, VCDUHeader};

use pyo3::prelude::*;

#[pyclass(get_all)]
#[derive(Debug, Clone, Default)]
pub struct FramingConfig {
    pub cadu_length: usize,
    pub without_pn: bool,
    pub rs: Option<u32>,
    pub rs_correct: bool,
    pub rs_detect: bool,
    pub rs_virtualfill: usize,
}

#[pymethods]
impl FramingConfig {
    /// Create a new FramingConfig.
    ///
    /// Arguments:
    ///   cadu_length: Length of a CADU, without any ASM bytes.
    #[new]
    pub fn new(cadu_length: usize) -> Self {
        let mut f = FramingConfig::default();
        f.cadu_length = cadu_length;
        f
    }
}

impl std::fmt::Display for FramingConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FramingConfig{{cadu_length={}, without_pn={}, rs={:?}, rs_correct={}, rs_detect={}, rs_virtualfill={}}}",
            self.cadu_length, self.without_pn, self.rs, self.rs_correct, self.rs_detect, self.rs_virtualfill, 
        )
    }
}

#[pymethods]
impl FramingConfig {
    pub fn __repr__(&self) -> String {
        format!("{self}")
    }
    pub fn without_pn(&self) -> Self {
        let mut c = self.clone();
        c.without_pn = false;
        c
    }
    pub fn with_rs(&self, interleave: u32) -> Self {
        let mut c = self.clone();
        c.rs = Some(interleave);
        c
    }

    /// Enabled RS correction. Implies `rs_detect`.
    pub fn with_rs_correct(&self) -> Self {
        let mut c = self.clone();
        c.rs_correct = true;
        c.rs_detect = true;
        c
    }
    pub fn with_rs_detect(&self) -> Self {
        let mut c = self.clone();
        c.rs_detect = true;
        c
    }
    pub fn with_rs_virtualfill(&self, num: usize) -> Self {
        let mut c = self.clone();
        c.rs_virtualfill = num;
        c
    }
}

#[derive(Debug)]
pub enum InputReader {
    Stdin(BufReader<std::io::Stdin>),
    File(BufReader<File>),
    TCP(BufReader<TcpStream>),
    IPC(BufReader<UnixStream>),
}

impl InputReader {
    fn from_str(s: &str) -> Result<InputReader, std::io::Error> {
        let reader = if s.starts_with("tcp://") {
            let addr = s.replace("tcp://", "");
            InputReader::TCP(BufReader::new(TcpStream::connect(addr)?))
        } else if s.starts_with("ipc://") {
            let path = s.replace("ipc://", "");
            InputReader::IPC(BufReader::new(UnixStream::connect(path)?))
        } else if s == "-" {
            InputReader::Stdin(BufReader::new(stdin()))
        } else {
            InputReader::File(BufReader::new(File::open(s)?))
        };
        Ok(reader)
    }
}

impl Read for InputReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            InputReader::Stdin(r) => r.read(buf),
            InputReader::File(r) => r.read(buf),
            InputReader::TCP(r) => r.read(buf),
            InputReader::IPC(r) => r.read(buf),
        }
    }
}

// FIXME: Remove "unsendable"
//        This may require some usage of Arc or something
#[pyclass(unsendable)]
pub struct FrameIter {
    frames: Box<dyn Iterator<Item = Frame> + Send + 'static>,
}

#[pymethods]
impl FrameIter {
    pub fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    pub fn __next__(mut slf: PyRefMut<Self>) -> Option<Frame> {
        slf.frames.next()
    }
}

#[pyfunction]
pub fn decode_frames(input: &str, config: FramingConfig) -> Result<FrameIter, std::io::Error> {
    let reader = InputReader::from_str(input)?;

    let mut pipeline = ccsds::framing::Pipeline::new(config.cadu_length);
    if config.without_pn {
        pipeline = pipeline.without_derandomization()
    }
    if let Some(i) = config.rs {
        let mut opts = RsOpts::new(i as u8);
        if config.rs_correct {
            opts = opts.with_correction(true);
        }
        if config.rs_detect {
            opts = opts.with_detection(true);
        }
        if config.rs_virtualfill > 0 {
            opts = opts.with_virtual_fill(config.rs_virtualfill)
        }
        pipeline = pipeline.with_rs(opts);
    }

    let frames = pipeline.start(reader);

    Ok(FrameIter {
        frames: Box::new(frames),
    })
}

#[pyfunction]
pub fn derandomize(dat: &[u8]) -> Vec<u8> {
    let derandomizer = DefaultDerandomizer::default();
    let dat = derandomizer.derandomize(dat);
    dat
}
