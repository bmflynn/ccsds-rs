#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Not enough data to decode; wanted={0}, got={1}")]
    TooShort(usize, usize),
}

pub type Result<T> = std::result::Result<T, Error>;
