#[cfg(any(windows, test))]
mod descriptor;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    windows::run()
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("storage-query requires Windows.");
    std::process::ExitCode::FAILURE
}
