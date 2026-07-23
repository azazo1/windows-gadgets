use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use clipboard_rs::{Clipboard, ClipboardContext, ClipboardHandler, ClipboardWatcher, ClipboardWatcherContext, ContentFormat, WatcherShutdown};
use tokio::sync::mpsc::UnboundedSender;

use super::RuntimeEvent;

pub trait ClipboardAccess: Send {
    fn has_files(&self) -> bool;
    fn has_text(&self) -> bool;
    fn get_files(&self) -> Result<Vec<String>>;
    fn get_text(&self) -> Result<String>;
    fn set_text(&self, text: String) -> Result<()>;
}

pub struct SystemClipboard {
    context: ClipboardContext,
}

impl SystemClipboard {
    pub fn new() -> Result<Self> {
        Ok(Self {
            context: ClipboardContext::new().map_err(|err| anyhow!(err.to_string()))?,
        })
    }
}

impl ClipboardAccess for SystemClipboard {
    fn has_files(&self) -> bool {
        self.context.has(ContentFormat::Files)
    }

    fn has_text(&self) -> bool {
        self.context.has(ContentFormat::Text)
    }

    fn get_files(&self) -> Result<Vec<String>> {
        self.context
            .get_files()
            .map_err(|err| anyhow!(err.to_string()))
    }

    fn get_text(&self) -> Result<String> {
        self.context
            .get_text()
            .map_err(|err| anyhow!(err.to_string()))
    }

    fn set_text(&self, text: String) -> Result<()> {
        self.context
            .set_text(text)
            .map_err(|err| anyhow!(err.to_string()))
    }
}

pub struct ClipboardMonitor {
    shutdown: Option<WatcherShutdown>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ClipboardMonitor {
    pub fn spawn(sender: UnboundedSender<RuntimeEvent>) -> Result<Self> {
        let (ready_sender, ready_receiver) = std::sync::mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("pathclip-clipboard".to_string())
            .spawn(move || {
                let mut watcher = match ClipboardWatcherContext::new() {
                    Ok(watcher) => watcher,
                    Err(err) => {
                        let _ = ready_sender.send(Err(err.to_string()));
                        return;
                    }
                };

                let shutdown = watcher
                    .add_handler(ChangeHandler { sender })
                    .get_shutdown_channel();
                if ready_sender.send(Ok(shutdown)).is_err() {
                    return;
                }

                watcher.start_watch();
            })
            .context("failed to start clipboard watcher thread")?;

        let shutdown = ready_receiver
            .recv_timeout(Duration::from_secs(5))
            .context("clipboard watcher did not start in time")?
            .map_err(|err| anyhow!("failed to start clipboard watcher: {err}"))?;

        Ok(Self {
            shutdown: Some(shutdown),
            thread: Some(thread),
        })
    }

    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            shutdown.stop();
        }
        if let Some(thread) = self.thread.take() {
            let _ = tokio::task::spawn_blocking(move || thread.join()).await;
        }
    }
}

struct ChangeHandler {
    sender: UnboundedSender<RuntimeEvent>,
}

impl ClipboardHandler for ChangeHandler {
    fn on_clipboard_change(&mut self) {
        let _ = self.sender.send(RuntimeEvent::ClipboardChanged);
    }
}
