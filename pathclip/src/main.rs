#[cfg(target_os = "windows")]
mod app;

#[cfg(target_os = "windows")]
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(err) = app::run().await {
        tracing::error!(error = ?err, "pathclip exited with an error");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("pathclip only supports Windows.");
}
