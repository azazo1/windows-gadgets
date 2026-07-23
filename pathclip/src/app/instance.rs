use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

pub struct InstanceGuard {
    handle: HANDLE,
}

impl InstanceGuard {
    pub fn acquire() -> Result<Self> {
        let handle = unsafe { CreateMutexW(None, true, w!(r"Local\pathclip-single-instance")) }
            .context("failed to create pathclip instance mutex")?;

        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            bail!("another pathclip instance is already running");
        }

        Ok(Self { handle })
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}
