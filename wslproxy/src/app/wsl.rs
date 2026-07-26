use tokio::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(super) fn command() -> Command {
    let mut command = Command::new("wsl.exe");
    command.creation_flags(CREATE_NO_WINDOW);
    command
}
