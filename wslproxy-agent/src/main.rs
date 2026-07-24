mod app;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    app::init_logging();

    if let Err(error) = app::run().await {
        tracing::error!(error = ?error, "wslproxy-agent 运行失败");
        std::process::exit(1);
    }
}
