use std::path::PathBuf;

pub fn fixture_path(name: &str) -> PathBuf {
    let mut path =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    dbg!(&path);
    path.push("tests/fixtures");
    path.push(name);
    path
}
