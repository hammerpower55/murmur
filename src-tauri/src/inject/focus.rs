//! The window that will receive the paste, and whether it is a terminal.

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusedApp {
    /// Executable file name, e.g. "Code.exe". Empty if it could not be read.
    pub exe: String,
    pub title: String,
}

/// Terminals paste with Ctrl+Shift+V. Matched on the executable first:
/// titles are unreliable ("deploy.powershell.md - Visual Studio Code").
const TERMINAL_EXES: &[&str] = &[
    "windowsterminal.exe",
    "openconsole.exe",
    "conhost.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "mintty.exe",
    "alacritty.exe",
    "wezterm-gui.exe",
    "wezterm.exe",
];

/// Only used when the executable could not be read (for example an
/// elevated window).
const TERMINAL_TITLES: &[&str] = &[
    "windows terminal",
    "command prompt",
    "cmd.exe",
    "powershell",
    "mintty",
    "alacritty",
    "wezterm",
];

impl FocusedApp {
    pub fn is_terminal(&self) -> bool {
        if !self.exe.is_empty() {
            return TERMINAL_EXES.contains(&self.exe.to_lowercase().as_str());
        }
        let title = self.title.to_lowercase();
        TERMINAL_TITLES.iter().any(|t| title.contains(t))
    }
}

pub fn foreground() -> FocusedApp {
    // SAFETY: plain Win32 queries on the current foreground window; every
    // buffer is sized and the process handle is closed.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return FocusedApp::default();
        }
        FocusedApp {
            exe: exe_name(hwnd).unwrap_or_default(),
            title: window_title(hwnd),
        }
    }
}

unsafe fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

unsafe fn exe_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    unsafe {
        let _ = CloseHandle(process);
    }
    result.ok()?;
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit('\\').next().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exe: &str, title: &str) -> FocusedApp {
        FocusedApp {
            exe: exe.into(),
            title: title.into(),
        }
    }

    #[test]
    fn terminals_by_executable() {
        assert!(app("WindowsTerminal.exe", "~").is_terminal());
        assert!(app("pwsh.exe", "").is_terminal());
        assert!(app("cmd.exe", "").is_terminal());
        assert!(app("mintty.exe", "").is_terminal());
        assert!(app("alacritty.exe", "").is_terminal());
        assert!(app("wezterm-gui.exe", "").is_terminal());
    }

    #[test]
    fn editors_are_not_terminals_even_with_terminal_titles() {
        assert!(!app("Code.exe", "deploy.powershell.md - Visual Studio Code").is_terminal());
        assert!(!app("notepad.exe", "cmd.exe notes.txt - Notepad").is_terminal());
    }

    #[test]
    fn title_fallback_when_executable_unknown() {
        assert!(app("", "Administrator: Windows PowerShell").is_terminal());
        assert!(app("", "Command Prompt").is_terminal());
        assert!(!app("", "Untitled - Notepad").is_terminal());
    }

    #[test]
    fn reads_the_real_foreground_window_without_failing() {
        // Whatever is in front during the test run, this must not panic.
        let _ = foreground();
    }
}
