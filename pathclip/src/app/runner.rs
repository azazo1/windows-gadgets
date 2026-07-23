use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{debug, info, warn};

use super::RuntimeEvent;
use super::clipboard::{ClipboardAccess, SystemClipboard};
use super::settings::{Profile, Settings};
use super::transform::{TransformResult, transform_files, transform_text};

pub(super) struct Runner<C = SystemClipboard> {
    settings: Settings,
    clipboard: C,
    receiver: UnboundedReceiver<RuntimeEvent>,
    recent_write: Option<RecentWrite>,
}

impl Runner<SystemClipboard> {
    pub(super) fn new(settings: Settings, receiver: UnboundedReceiver<RuntimeEvent>) -> Result<Self> {
        Ok(Self {
            settings,
            clipboard: SystemClipboard::new()?,
            receiver,
            recent_write: None,
        })
    }
}

impl<C: ClipboardAccess> Runner<C> {
    pub(super) async fn run(&mut self) -> Result<()> {
        while let Some(event) = self.receiver.recv().await {
            match event {
                RuntimeEvent::ClipboardChanged => self.handle_clipboard_change().await,
                RuntimeEvent::HotkeyPressed(id) => self.handle_hotkey(id).await,
            }
        }
        Ok(())
    }

    async fn handle_clipboard_change(&mut self) {
        let Some(profile) = self.settings.auto_profile().cloned() else {
            return;
        };

        if self.clipboard.has_files() {
            self.recent_write = None;
            debug!(profile = %profile.name, "automatic conversion skipped for file clipboard");
            return;
        }
        if !self.clipboard.has_text() {
            self.recent_write = None;
            return;
        }

        let text = match self.read_text_with_retry().await {
            Ok(text) => text,
            Err(err) => {
                warn!(error = ?err, "failed to read clipboard text");
                return;
            }
        };

        if self.is_recent_self_write(&text) {
            return;
        }

        match transform_text(&profile, &text) {
            Ok(Some(result)) if result.output == text => {
                debug!(profile = %profile.name, "conversion output is unchanged");
            }
            Ok(Some(result)) => self.write_result(&profile, result).await,
            Ok(None) => debug!(profile = %profile.name, "clipboard text is not an absolute Windows path list"),
            Err(err) => warn!(profile = %profile.name, error = ?err, "automatic path conversion failed"),
        }
    }

    async fn handle_hotkey(&mut self, hotkey_id: u32) {
        let Some(profile) = self.settings.profile_for_hotkey(hotkey_id).cloned() else {
            warn!(hotkey_id, "received an unknown hotkey event");
            return;
        };

        let result = if self.clipboard.has_files() {
            match self.read_files_with_retry().await {
                Ok(paths) => transform_files(&profile, &paths),
                Err(err) => Err(err),
            }
        } else if self.clipboard.has_text() {
            match self.read_text_with_retry().await {
                Ok(text) => match transform_text(&profile, &text) {
                    Ok(Some(result)) if result.output == text => {
                        debug!(profile = %profile.name, "conversion output is unchanged");
                        return;
                    }
                    Ok(Some(result)) => Ok(result),
                    Ok(None) => Err(anyhow!(
                        "clipboard text is not an absolute Windows path list"
                    )),
                    Err(err) => Err(err),
                },
                Err(err) => Err(err),
            }
        } else {
            Err(anyhow!("clipboard does not contain text or files"))
        };

        match result {
            Ok(result) => self.write_result(&profile, result).await,
            Err(err) => warn!(profile = %profile.name, error = ?err, "hotkey path conversion failed"),
        }
    }

    async fn read_text_with_retry(&self) -> Result<String> {
        retry_clipboard(|| self.clipboard.get_text()).await
    }

    async fn read_files_with_retry(&self) -> Result<Vec<String>> {
        retry_clipboard(|| self.clipboard.get_files()).await
    }

    async fn write_result(&mut self, profile: &Profile, result: TransformResult) {
        if result.output.is_empty() {
            warn!(profile = %profile.name, "conversion produced empty clipboard text");
            return;
        }

        match retry_clipboard(|| self.clipboard.set_text(result.output.clone())).await {
            Ok(()) => {
                self.recent_write = Some(RecentWrite {
                    text: result.output,
                    at: Instant::now(),
                });
                info!(profile = %profile.name, paths = result.path_count, "clipboard paths converted");
            }
            Err(err) => warn!(profile = %profile.name, error = ?err, "failed to write converted clipboard text"),
        }
    }

    fn is_recent_self_write(&mut self, text: &str) -> bool {
        let Some(recent) = &self.recent_write else {
            return false;
        };

        if recent.at.elapsed() > Duration::from_secs(2) {
            self.recent_write = None;
            return false;
        }

        if recent.text == text {
            self.recent_write = None;
            debug!("ignored clipboard notification caused by pathclip");
            return true;
        }

        false
    }
}

struct RecentWrite {
    text: String,
    at: Instant,
}

async fn retry_clipboard<T, F>(mut operation: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut last_error = None;
    for attempt in 0..5 {
        match operation() {
            Ok(value) => return Ok(value),
            Err(err) => {
                last_error = Some(err);
                if attempt < 4 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        }
    }
    Err(last_error.expect("clipboard retry loop must have an error"))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use anyhow::Result;
    use tokio::sync::mpsc::unbounded_channel;

    use super::{ClipboardAccess, RecentWrite, Runner, Settings};

    #[derive(Default)]
    struct MockState {
        files: Vec<String>,
        text: String,
        writes: Vec<String>,
    }

    #[derive(Default)]
    struct MockClipboard {
        state: Mutex<MockState>,
    }

    impl MockClipboard {
        fn with_text(text: &str) -> Self {
            Self {
                state: Mutex::new(MockState {
                    text: text.to_string(),
                    ..MockState::default()
                }),
            }
        }

        fn with_files(files: &[&str]) -> Self {
            Self {
                state: Mutex::new(MockState {
                    files: files.iter().map(|path| path.to_string()).collect(),
                    ..MockState::default()
                }),
            }
        }

        fn writes(&self) -> Vec<String> {
            self.state.lock().unwrap().writes.clone()
        }
    }

    impl ClipboardAccess for MockClipboard {
        fn has_files(&self) -> bool {
            !self.state.lock().unwrap().files.is_empty()
        }

        fn has_text(&self) -> bool {
            !self.state.lock().unwrap().text.is_empty()
        }

        fn get_files(&self) -> Result<Vec<String>> {
            Ok(self.state.lock().unwrap().files.clone())
        }

        fn get_text(&self) -> Result<String> {
            Ok(self.state.lock().unwrap().text.clone())
        }

        fn set_text(&self, text: String) -> Result<()> {
            let mut state = self.state.lock().unwrap();
            state.text.clone_from(&text);
            state.files.clear();
            state.writes.push(text);
            Ok(())
        }
    }

    fn settings(source: &str) -> Settings {
        Settings::parse(source).unwrap()
    }

    fn runner(settings: Settings, clipboard: MockClipboard) -> Runner<MockClipboard> {
        let (_sender, receiver) = unbounded_channel();
        Runner {
            settings,
            clipboard,
            receiver,
            recent_write: None,
        }
    }

    #[tokio::test]
    async fn automatic_conversion_never_replaces_file_objects() {
        let settings = settings(
            r#"
                auto_profile = "slash"

                [profiles.slash]
                steps = [{ type = "forward-slash" }]
            "#,
        );
        let mut runner = runner(settings, MockClipboard::with_files(&[r"C:\a.txt"]));

        runner.handle_clipboard_change().await;

        assert!(runner.clipboard.writes().is_empty());
    }

    #[tokio::test]
    async fn hotkey_explicitly_converts_file_objects() {
        let settings = settings(
            r#"
                auto_profile = ""

                [profiles.slash]
                hotkey = "Ctrl+Shift+V"
                steps = [{ type = "forward-slash" }]
            "#,
        );
        let hotkey_id = settings.registered_hotkeys()[0].id();
        let mut runner = runner(
            settings,
            MockClipboard::with_files(&[r"C:\a.txt", r"D:\b.txt"]),
        );

        runner.handle_hotkey(hotkey_id).await;

        assert_eq!(runner.clipboard.writes(), vec!["C:/a.txt\r\nD:/b.txt"]);
    }

    #[tokio::test]
    async fn partial_file_conversion_failure_keeps_clipboard() {
        let settings = settings(
            r#"
                auto_profile = ""

                [profiles.wsl]
                hotkey = "Ctrl+Shift+V"
                steps = [{ type = "wsl" }]
            "#,
        );
        let hotkey_id = settings.registered_hotkeys()[0].id();
        let mut runner = runner(
            settings,
            MockClipboard::with_files(&[r"C:\a.txt", r"\\server\share\b.txt"]),
        );

        runner.handle_hotkey(hotkey_id).await;

        assert!(runner.clipboard.writes().is_empty());
    }

    #[tokio::test]
    async fn unchanged_automatic_output_is_not_written() {
        let settings = settings(
            r#"
                auto_profile = "slash"

                [profiles.slash]
                steps = [{ type = "forward-slash" }]
            "#,
        );
        let mut runner = runner(settings, MockClipboard::with_text("C:/a.txt"));

        runner.handle_clipboard_change().await;

        assert!(runner.clipboard.writes().is_empty());
    }

    #[test]
    fn self_write_is_ignored_exactly_once() {
        let settings = settings(
            r#"
                auto_profile = "slash"

                [profiles.slash]
                steps = [{ type = "forward-slash" }]
            "#,
        );
        let mut runner = runner(settings, MockClipboard::default());
        runner.recent_write = Some(RecentWrite {
            text: "C:/a.txt".to_string(),
            at: std::time::Instant::now(),
        });

        assert!(runner.is_recent_self_write("C:/a.txt"));
        assert!(!runner.is_recent_self_write("C:/a.txt"));
    }
}
