#[cfg(target_os = "windows")]
mod app;

#[cfg(target_os = "windows")]
fn main() {
    app::run();
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("fncaps only supports Windows.");
}
