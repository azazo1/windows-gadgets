use std::process::Command;

/// 打开指定的程序或可执行文件
pub fn open_program(program: &str) {
    tracing::info!(target: "fncaps::launch", program, "launching program");

    // 尝试直接运行 (可能是 PATH 中的可执行文件或完整路径)
    match Command::new(program).spawn() {
        Ok(_) => {
            tracing::debug!(target: "fncaps::launch", program, "program spawned successfully");
        }
        Err(e) => {
            // 如果直接运行失败，尝试通过 which 查找
            if let Ok(path) = which::which(program) {
                tracing::debug!(target: "fncaps::launch", program, resolved = %path.display(), "resolved via which");
                let _ = Command::new(path).spawn();
            } else {
                tracing::error!(target: "fncaps::launch", program, error = %e, "failed to launch program");
            }
        }
    }
}
