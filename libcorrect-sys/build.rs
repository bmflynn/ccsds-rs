fn main() {
    let files = ["decode.c", "encode.c", "polynomial.c", "reed-solomon.c"];
    let mut build = cc::Build::new();
    build
        .include("vendor/include")
        .std("c99")
        .opt_level(2)
        .warnings(false);
    for f in files {
        let path = format!("vendor/src/{f}");
        println!("cargo:rerun-if-changed={path}");
        build.file(path);
    }
    build.compile("correct_reed_solomon");
    println!("cargo:rerun-if-changed=build.rs");
}
