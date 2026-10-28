//! The overlay window: always on top, transparent, click through, and never
//! focusable, so dictating never takes focus from the target app.
//!
//! The window stays shown for the life of the app and the page draws nothing
//! when idle. Hiding it would mean showing it again later: WebView2 stops
//! rendering hidden windows, and showing a window through Tauri activates it.
//! A transparent click through window is invisible and can never steal focus.

use std::sync::Mutex;

use anyhow::{Context, Result};
use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
    SWP_NOACTIVATE, SWP_NOOWNERZORDER, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT,
};

pub const LABEL: &str = "overlay";

/// Capsule size from the spec, in logical pixels.
const PILL_W: i32 = 240;
const PILL_H: i32 = 56;
/// Room around the capsule for its shadow and the appear animation.
const MARGIN: i32 = 20;
const WINDOW_W: i32 = PILL_W + 2 * MARGIN;
const WINDOW_H: i32 = PILL_H + 2 * MARGIN;
/// Gap between the capsule's bottom edge and the taskbar.
const ABOVE_TASKBAR: i32 = 48;

pub struct OverlayWindow {
    // HWND is a raw pointer; stored as an integer so the struct is Send.
    hwnd: isize,
    placed: Mutex<Option<RECT>>,
}

impl OverlayWindow {
    pub fn setup(app: &AppHandle) -> Result<Self> {
        let window = app
            .get_webview_window(LABEL)
            .context("overlay window missing")?;
        window
            .set_ignore_cursor_events(true)
            .context("could not make the overlay click through")?;
        let hwnd = window.hwnd().context("overlay has no window handle")?.0 as isize;

        let overlay = Self {
            hwnd,
            placed: Mutex::new(None),
        };
        // SAFETY: hwnd is the overlay's live top level window.
        unsafe {
            let h = overlay.handle();
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            let wanted =
                (WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW).0;
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | wanted as isize);
        }
        overlay.place_on_cursor_monitor()?;
        Ok(overlay)
    }

    fn handle(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    /// Centres the capsule horizontally on the monitor under the mouse, its
    /// bottom edge 48 px above the taskbar. Only moves when the target
    /// rectangle changed, so the webview is not resized on every dictation.
    pub fn place_on_cursor_monitor(&self) -> Result<()> {
        let target = target_rect()?;
        let mut placed = self
            .placed
            .lock()
            .map_err(|_| anyhow::anyhow!("overlay lock poisoned"))?;
        if placed.is_some_and(|r| r == target) {
            return Ok(());
        }
        // Twice: moving onto a monitor with a different DPI makes Windows
        // rescale the window once on arrival; the second call restores the
        // size computed for that monitor.
        for _ in 0..2 {
            // SAFETY: plain SetWindowPos on our own window; no activation.
            unsafe {
                SetWindowPos(
                    self.handle(),
                    Some(HWND_TOPMOST),
                    target.left,
                    target.top,
                    target.right - target.left,
                    target.bottom - target.top,
                    SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                )
            }
            .context("could not move the overlay")?;
        }
        *placed = Some(target);
        Ok(())
    }
}

fn target_rect() -> Result<RECT> {
    // SAFETY: Win32 queries with valid out pointers.
    unsafe {
        let mut cursor = POINT::default();
        GetCursorPos(&mut cursor).context("could not read the cursor position")?;
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            anyhow::bail!("could not read the monitor size");
        }
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        Ok(layout(info.rcWork, dpi_x))
    }
}

/// The overlay rectangle in physical pixels for a monitor work area (the
/// screen minus the taskbar) at the given DPI.
fn layout(work: RECT, dpi: u32) -> RECT {
    let scale = |v: i32| (v as f64 * dpi as f64 / 96.0).round() as i32;
    let (w, h) = (scale(WINDOW_W), scale(WINDOW_H));
    let left = work.left + (work.right - work.left - w) / 2;
    let pill_bottom = work.bottom - scale(ABOVE_TASKBAR);
    let bottom = pill_bottom + scale(MARGIN);
    RECT {
        left,
        top: bottom - h,
        right: left + w,
        bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn centred_and_48_px_above_the_taskbar_at_100_percent() {
        // 1920x1080 with a 48 px taskbar at the bottom.
        let r = layout(work(0, 0, 1920, 1032), 96);
        assert_eq!(r.right - r.left, WINDOW_W);
        assert_eq!(r.bottom - r.top, WINDOW_H);
        assert_eq!(r.left + (r.right - r.left) / 2, 960);
        // Capsule bottom = window bottom minus the margin.
        assert_eq!(r.bottom - MARGIN, 1032 - 48);
    }

    #[test]
    fn scales_with_dpi() {
        // 150% scaling.
        let r = layout(work(0, 0, 2880, 1728), 144);
        assert_eq!(r.right - r.left, 420);
        assert_eq!(r.bottom - r.top, 144);
        assert_eq!(r.bottom - 30, 1728 - 72);
    }

    #[test]
    fn works_on_a_second_monitor_to_the_left() {
        let r = layout(work(-1920, 0, 0, 1040), 96);
        assert_eq!(r.left + (r.right - r.left) / 2, -960);
    }
}
