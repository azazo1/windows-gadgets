use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use global_hotkey::hotkey::HotKey;
use tokio::sync::mpsc::UnboundedSender;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::WindowId;

use super::RuntimeEvent;

enum ControlEvent {
    Stop,
}

pub struct HotkeyMonitor {
    proxy: EventLoopProxy<ControlEvent>,
    thread: Option<thread::JoinHandle<()>>,
}

impl HotkeyMonitor {
    pub fn spawn(
        hotkeys: Vec<HotKey>,
        sender: UnboundedSender<RuntimeEvent>,
    ) -> Result<Option<Self>> {
        if hotkeys.is_empty() {
            return Ok(None);
        }

        let (ready_sender, ready_receiver) = std::sync::mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("pathclip-hotkeys".to_string())
            .spawn(move || {
                let mut builder = EventLoop::<ControlEvent>::with_user_event();
                builder.with_any_thread(true);
                let event_loop = match builder.build() {
                    Ok(event_loop) => event_loop,
                    Err(err) => {
                        let _ = ready_sender.send(Err(err.to_string()));
                        return;
                    }
                };

                let proxy = event_loop.create_proxy();
                let manager = match GlobalHotKeyManager::new() {
                    Ok(manager) => manager,
                    Err(err) => {
                        let _ = ready_sender.send(Err(err.to_string()));
                        return;
                    }
                };

                if let Err(err) = manager.register_all(&hotkeys) {
                    let _ = ready_sender.send(Err(err.to_string()));
                    return;
                }

                GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
                    if event.state == HotKeyState::Pressed {
                        let _ = sender.send(RuntimeEvent::HotkeyPressed(event.id));
                    }
                }));

                if ready_sender.send(Ok(proxy)).is_err() {
                    return;
                }

                let mut app = HotkeyApp { manager, hotkeys };
                if let Err(err) = event_loop.run_app(&mut app) {
                    tracing::error!(error = ?err, "global hotkey event loop exited with an error");
                }
            })
            .context("failed to start global hotkey thread")?;

        let proxy = ready_receiver
            .recv_timeout(Duration::from_secs(5))
            .context("global hotkey thread did not start in time")?
            .map_err(|err| anyhow!("failed to start global hotkeys: {err}"))?;

        Ok(Some(Self {
            proxy,
            thread: Some(thread),
        }))
    }

    pub async fn stop(mut self) {
        let _ = self.proxy.send_event(ControlEvent::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = tokio::task::spawn_blocking(move || thread.join()).await;
        }
    }
}

struct HotkeyApp {
    manager: GlobalHotKeyManager,
    hotkeys: Vec<HotKey>,
}

impl ApplicationHandler<ControlEvent> for HotkeyApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ControlEvent) {
        if matches!(event, ControlEvent::Stop) {
            let _ = self.manager.unregister_all(&self.hotkeys);
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }
}
