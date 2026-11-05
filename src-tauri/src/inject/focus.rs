//! The window that will receive the paste, and whether it is a terminal.

#[cfg(windows)]
use windows::core::PWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HWND};
#[cfg(windows)]
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusedApp {
    /// Executable file name, e.g. "Code.exe". On macOS the app bundle's
    /// name, e.g. "Slack.app". Empty if it could not be read.
    pub exe: String,
    /// Full path of the executable (the .app bundle on macOS), for its icon.
    pub exe_path: String,
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
    // macOS: terminals are only told apart so their text is never read.
    "terminal.app",
    "iterm.app",
    "warp.app",
    "alacritty.app",
    "wezterm.app",
    "kitty.app",
    "ghostty.app",
    "hyper.app",
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

/// The frontmost app and its focused window's title (the title needs the
/// Accessibility permission and is empty without it).
#[cfg(target_os = "macos")]
pub fn foreground() -> FocusedApp {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let Some(app) = workspace.frontmostApplication() else {
        return FocusedApp::default();
    };
    let bundle = app.bundleURL().and_then(|url| url.path()).map(|p| p.to_string());
    let exe_path = bundle.unwrap_or_default();
    let exe = exe_path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    let title = crate::macos::Element::application(app.processIdentifier())
        .and_then(|a| a.element("AXFocusedWindow"))
        .and_then(|w| w.string("AXTitle"))
        .unwrap_or_default();
    FocusedApp {
        exe,
        exe_path,
        title,
    }
}

#[cfg(windows)]
pub fn foreground() -> FocusedApp {
    // SAFETY: plain Win32 queries on the current foreground window; every
    // buffer is sized and the process handle is closed.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return FocusedApp::default();
        }
        let exe_path = exe_path(hwnd).unwrap_or_default();
        FocusedApp {
            exe: exe_path.rsplit('\\').next().unwrap_or_default().to_string(),
            exe_path,
            title: window_title(hwnd),
        }
    }
}

#[cfg(windows)]
unsafe fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

#[cfg(windows)]
unsafe fn exe_path(hwnd: HWND) -> Option<String> {
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
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exe: &str, title: &str) -> FocusedApp {
        FocusedApp {
            exe: exe.into(),
            exe_path: String::new(),
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
        assert!(app("Terminal.app", "").is_terminal());
        assert!(app("iTerm.app", "").is_terminal());
        assert!(!app("Visual Studio Code.app", "zsh").is_terminal());
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
