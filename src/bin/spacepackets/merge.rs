use std::process;

pub(crate) fn do_merge(input: String) -> process::ExitCode {
    println!("merge input={:?}", input);
    return process::ExitCode::FAILURE;
}
