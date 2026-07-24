#[cfg(target_os = "windows")]
mod app;

#[cfg(target_os = "windows")]
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(error) = app::run().await {
        tracing::error!(error = ?error, "wslproxy 运行失败");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("wslproxy 仅支持 Windows.");
}
