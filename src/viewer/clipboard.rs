//! Copies through a platform helper when a display session is available, and through OSC 52 for SSH.
use std::io::Write;
use std::process::{Command, Stdio};

/// Returns the OSC 52 sequence immediately and copies through the native helper off the viewer thread.
pub fn copy(text: &str) -> String {
    let text = pasteable(text);
    let seq = osc52(&text);
    std::thread::spawn(move || native_copy(&text));
    seq
}

fn pasteable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t' | '\r')).collect()
}

/// OSC 52 puts `text` on the clipboard of the terminal the user sits at, also over SSH.
fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x1b\\", crate::term::kitty::base64(text.as_bytes()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    #[cfg(target_os = "macos")]
    Pbcopy,
    #[cfg(any(test, not(target_os = "macos")))]
    Wayland,
    #[cfg(any(test, not(target_os = "macos")))]
    Xclip,
    #[cfg(any(test, not(target_os = "macos")))]
    Xsel,
}

impl Backend {
    fn command(self) -> Command {
        let mut command = match self {
            #[cfg(target_os = "macos")]
            Backend::Pbcopy => {
                let mut command = Command::new("pbcopy");
                command.env("LC_CTYPE", "UTF-8");
                command
            }
            #[cfg(any(test, not(target_os = "macos")))]
            Backend::Wayland => {
                let mut command = Command::new("wl-copy");
                command.args(["--type", "text/plain;charset=utf-8"]);
                command
            }
            #[cfg(any(test, not(target_os = "macos")))]
            Backend::Xclip => {
                let mut command = Command::new("xclip");
                command.args(["-selection", "clipboard", "-in"]);
                command
            }
            #[cfg(any(test, not(target_os = "macos")))]
            Backend::Xsel => {
                let mut command = Command::new("xsel");
                command.args(["--clipboard", "--input"]);
                command
            }
        };
        command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        command
    }
}

/// Prefer the native Wayland clipboard when both sessions exist; X11 is a fallback only with DISPLAY.
#[cfg(any(test, not(target_os = "macos")))]
fn backends(wayland: Option<&str>, display: Option<&str>) -> Vec<Backend> {
    let mut out = Vec::new();
    if wayland.is_some_and(|s| !s.is_empty()) {
        out.push(Backend::Wayland);
    }
    if display.is_some_and(|s| !s.is_empty()) {
        out.extend([Backend::Xclip, Backend::Xsel]);
    }
    out
}

fn first_working(backends: &[Backend], mut copy: impl FnMut(Backend) -> bool) -> Option<Backend> {
    backends.iter().copied().find(|&backend| copy(backend))
}

fn copy_with(backend: Backend, text: &str) -> bool {
    let Ok(mut child) = backend.command().spawn() else { return false };
    // Dropping stdin before wait delivers EOF, which the helpers need before taking ownership.
    let written = child.stdin.take().is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    if !written {
        let _ = child.kill();
    }
    let succeeded = child.wait().is_ok_and(|status| status.success());
    written && succeeded
}

fn native_copy(text: &str) {
    #[cfg(target_os = "macos")]
    let choices = [Backend::Pbcopy];
    #[cfg(not(target_os = "macos"))]
    let choices = {
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        let display = std::env::var("DISPLAY").ok();
        backends(wayland.as_deref(), display.as_deref())
    };
    let _ = first_working(&choices, |backend| copy_with(backend, text));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_text_drops_control_characters() {
        assert_eq!(pasteable("ls\x1b[201~echo\u{9b}hi\x07\n\tok\r\n"), "ls[201~echohi\n\tok\r\n");
    }

    #[test]
    fn clipboard_backends_follow_available_sessions() {
        assert_eq!(backends(Some("wayland-1"), Some(":0")), [Backend::Wayland, Backend::Xclip, Backend::Xsel]);
        assert_eq!(backends(Some("wayland-1"), None), [Backend::Wayland]);
        assert_eq!(backends(None, Some(":0")), [Backend::Xclip, Backend::Xsel]);
        assert!(backends(None, None).is_empty());
        assert!(backends(Some(""), Some("")).is_empty());
    }

    #[test]
    fn clipboard_commands_preserve_utf8_and_target_clipboard_not_primary() {
        let args = |backend: Backend| backend.command().get_args().map(|s| s.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(Backend::Wayland.command().get_program(), "wl-copy");
        assert_eq!(args(Backend::Wayland), ["--type", "text/plain;charset=utf-8"]);
        assert_eq!(Backend::Xclip.command().get_program(), "xclip");
        assert_eq!(args(Backend::Xclip), ["-selection", "clipboard", "-in"]);
        assert_eq!(Backend::Xsel.command().get_program(), "xsel");
        assert_eq!(args(Backend::Xsel), ["--clipboard", "--input"]);
    }

    #[test]
    fn clipboard_failure_tries_the_next_backend_without_real_commands() {
        let choices = [Backend::Wayland, Backend::Xclip, Backend::Xsel];
        let mut tried = Vec::new();
        let selected = first_working(&choices, |backend| {
            tried.push(backend);
            backend == Backend::Xclip
        });
        assert_eq!(selected, Some(Backend::Xclip));
        assert_eq!(tried, [Backend::Wayland, Backend::Xclip]);
        assert_eq!(first_working(&choices, |_| false), None);
        assert_eq!(first_working(&[], |_| panic!("no backend should run")), None);
    }

    #[test]
    fn osc52_carries_the_text_in_base64() {
        assert_eq!(osc52("é✓"), "\x1b]52;c;w6ninJM=\x1b\\");
    }
}
