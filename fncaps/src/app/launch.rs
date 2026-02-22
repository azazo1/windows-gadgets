use std::path::PathBuf;
use std::process::Command;

const TEXT_EDITOR_EXE_PATH: &str = "subl.exe";

pub fn open_text_editor() {
    tracing::info!(target: "fncaps::launch", exe = TEXT_EDITOR_EXE_PATH, "launching text editor");
    let _ = Command::new(TEXT_EDITOR_EXE_PATH).spawn();
}

pub fn open_vscode() {
    if let Some(path) = find_vscode_executable() {
        tracing::info!(target: "fncaps::launch", exe = %path.display(), "launching vscode");
        let _ = Command::new(path).spawn();
    } else {
        tracing::warn!(target: "fncaps::launch", "vscode executable not found");
    }
}

pub fn open_pwsh() {
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("C:\\"));

    let Ok(pwsh) = which::which("pwsh.exe") else {
        tracing::warn!(target:"fncaps::launch", "pwsh.exe not found");
        return;
    };
    tracing::info!(target: "fncaps::launch", exe = %pwsh.display(), cwd = %home.display(), "launching shell");
    let _ = Command::new(pwsh).current_dir(home).spawn();
}

fn find_vscode_executable() -> Option<PathBuf> {
    which::which("Code.exe").ok()
}
