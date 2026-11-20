//! Spawning helper tools (ffmpeg, ffprobe, the AI sidecar) consistently across platforms.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

/// A `Command` that never opens a console window on Windows (PhotoHop is a GUI app).
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Resolve a bundled tool: `$ENV_VAR` if set, else next to the PhotoHop executable, else `PATH`.
pub fn tool_path(name: &str, env_var: &str) -> PathBuf {
    if let Some(p) = std::env::var_os(env_var) {
        return PathBuf::from(p);
    }
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() };
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) {
        let local = dir.join(&file);
        if local.is_file() {
            return local;
        }
    }
    PathBuf::from(name)
}
