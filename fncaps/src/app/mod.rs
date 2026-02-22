mod hotkey;
mod ime;
mod launch;
mod logging;
mod state;
mod windows_ops;

pub fn run() {
    logging::init_logging();
    tracing::info!(target: "fncaps", "fncaps starting");

    if let Err(err) = hotkey::run() {
        tracing::error!(target: "fncaps", error = %err, "fncaps stopped with error");
    }
}
