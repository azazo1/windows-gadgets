use std::{
    io,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::Command,
};

fn open_with_default_app(path: &str) -> io::Result<()> {
    let mut path = PathBuf::from(path);
    if !path.is_absolute() {
        path = which::which(path)
            .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?
            .into();
    }
    let Some(path) = path.to_str().map(str::to_string) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidFilename,
            anyhow::anyhow!("{:?}", path),
        ));
    };
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    Command::new("C:\\Windows\\system32\\cmd.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args(&["/C", "start", "", &path])
        .spawn()?;
    Ok(())
}

/// 打开指定的程序或可执行文件
pub fn open_program(program: &str) {
    tracing::info!(target: "fncaps::launch", program, "launching program");

    // 尝试直接运行 (可能是 PATH 中的可执行文件或完整路径)
    match open_with_default_app(program) {
        Ok(_) => {
            tracing::debug!(target: "fncaps::launch", program, "program spawned successfully");
        }
        Err(e) => {
            tracing::error!(target: "fncaps::launch", program, error = %e, "failed to launch program");
        }
    }
}
