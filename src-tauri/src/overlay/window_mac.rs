//! The overlay window on macOS: always on top at status window level, on
//! every Space and over full screen apps, transparent and click through.
//! Like the Windows overlay (window.rs) it stays shown for the life of the
//! app and the page draws nothing when idle; it is only ever moved, which
//! never activates Murmur or takes focus from the target app.

use std::sync::Mutex;

use anyhow::{Context, Result};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::settings::OverlayPosition;

pub const LABEL: &str = "overlay";

/// The same gaps as window.rs, in points. The window's size comes from
/// tauri.conf.json, which fits the capsule plus `MARGIN` on each side.
const MARGIN: f64 = 16.0;
/// Gap between the capsule and the screen edge it sits against: just above
/// the Dock for the bottom positions, below the menu bar for top centre.
const EDGE_GAP: f64 = 12.0;
/// Gap between the capsule and the right edge in the bottom right position.
const SIDE_GAP: f64 = 16.0;
/// NSStatusWindowLevel: above normal and floating windows, like the menu
/// bar's status items.
const STATUS_WINDOW_LEVEL: isize = 25;

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

pub struct OverlayWindow {
    window: WebviewWindow,
    /// The window size in points, as configured.
    size: (f64, f64),
    placed: Mutex<Option<Rect>>,
    position: Mutex<OverlayPosition>,
}

impl OverlayWindow {
    pub fn setup(app: &AppHandle, position: OverlayPosition) -> Result<Self> {
        let window = app
            .get_webview_window(LABEL)
            .context("overlay window missing")?;
        window
            .set_ignore_cursor_events(true)
            .context("could not make the overlay click through")?;
        let scale = window.scale_factor().unwrap_or(2.0);
        let size = window
            .inner_size()
            .context("could not read the overlay size")?
            .to_logical::<f64>(scale);

        let handle = window.clone();
        window
            .run_on_main_thread(move || raise(&handle))
            .context("could not set up the overlay window")?;

        let overlay = Self {
            window,
            size: (size.width, size.height),
            placed: Mutex::new(None),
            position: Mutex::new(position),
        };
        overlay.place_on_cursor_monitor()?;
        Ok(overlay)
    }

    pub fn set_position(&self, position: OverlayPosition) {
        if let Ok(mut p) = self.position.lock() {
            *p = position;
        }
    }

    /// Places the capsule on the display under the mouse: by default centred,
    /// its bottom edge 12 points above the Dock. Only moves when the target
    /// rectangle changed.
    pub fn place_on_cursor_monitor(&self) -> Result<()> {
        let position = self
            .position
            .lock()
            .map(|p| *p)
            .unwrap_or(OverlayPosition::BottomCentre);
        let (work, scale) = cursor_work_area(&self.window)?;
        let target = layout(work, scale, self.size, position);
        let mut placed = self
            .placed
            .lock()
            .map_err(|_| anyhow::anyhow!("overlay lock poisoned"))?;
        if placed.is_some_and(|r| r == target) {
            return Ok(());
        }
        self.window
            .set_size(PhysicalSize::new(target.w.max(1) as u32, target.h.max(1) as u32))
            .context("could not size the overlay")?;
        self.window
            .set_position(PhysicalPosition::new(target.x, target.y))
            .context("could not move the overlay")?;
        *placed = Some(target);
        Ok(())
    }
}

/// Status window level, on every Space and beside full screen apps, out of
/// the window cycle. Runs on the main thread.
fn raise(window: &WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};

    let Ok(ptr) = window.ns_window() else {
        log::warn!("overlay has no NSWindow");
        return;
    };
    // SAFETY: Tauri hands out the live NSWindow of this window, and this
    // runs on the main thread.
    let ns_window: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
    ns_window.setLevel(STATUS_WINDOW_LEVEL);
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    ns_window.setIgnoresMouseEvents(true);
    ns_window.setHasShadow(false);
}

/// The usable area (without the menu bar and Dock) of the display under
/// the mouse, in physical pixels, and its scale factor.
fn cursor_work_area(window: &WebviewWindow) -> Result<(Rect, f64)> {
    let cursor = window.cursor_position().ok();
    let monitors = window.available_monitors().unwrap_or_default();
    let contains = |m: &&tauri::Monitor| {
        let (p, s) = (m.position(), m.size());
        cursor.is_some_and(|c| {
            c.x >= p.x as f64
                && c.y >= p.y as f64
                && c.x < p.x as f64 + s.width as f64
                && c.y < p.y as f64 + s.height as f64
        })
    };
    let monitor = match monitors.iter().find(contains) {
        Some(m) => m.clone(),
        None => window
            .primary_monitor()
            .ok()
            .flatten()
            .context("could not find a display")?,
    };
    let work = monitor.work_area();
    Ok((
        Rect {
            x: work.position.x,
            y: work.position.y,
            w: work.size.width as i32,
            h: work.size.height as i32,
        },
        monitor.scale_factor(),
    ))
}

/// The overlay rectangle in physical pixels for a display's usable area at
/// the given scale, for a window of `size` points.
fn layout(work: Rect, scale: f64, size: (f64, f64), position: OverlayPosition) -> Rect {
    let px = |v: f64| (v * scale).round() as i32;
    let (w, h) = (px(size.0), px(size.1));
    let x = match position {
        OverlayPosition::BottomRight => work.x + work.w - px(SIDE_GAP) - w + px(MARGIN),
        _ => work.x + (work.w - w) / 2,
    };
    let y = match position {
        OverlayPosition::TopCentre => work.y + px(EDGE_GAP) - px(MARGIN),
        _ => work.y + work.h - px(EDGE_GAP) + px(MARGIN) - h,
    };
    Rect { x, y, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f64, f64) = (208.0, 64.0);

    fn work(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn centred_and_just_above_the_dock_on_a_retina_display() {
        // A 1512 by 982 point display at 2x, menu bar 37 points, Dock 70.
        let area = work(0, 74, 3024, 1750);
        let r = layout(area, 2.0, SIZE, OverlayPosition::BottomCentre);
        assert_eq!((r.w, r.h), (416, 128));
        assert_eq!(r.x + r.w / 2, 1512);
        // Capsule bottom = window bottom minus the margin, 12 points up.
        assert_eq!(r.y + r.h - 32, 74 + 1750 - 24);
    }

    #[test]
    fn top_centre_and_bottom_right() {
        let area = work(0, 25, 1440, 875);
        let top = layout(area, 1.0, SIZE, OverlayPosition::TopCentre);
        assert_eq!(top.y + 16, 25 + 12);
        let corner = layout(area, 1.0, SIZE, OverlayPosition::BottomRight);
        assert_eq!(corner.x + corner.w - 16, 1440 - 16);
        assert_eq!(corner.y + corner.h - 16, 25 + 875 - 12);
    }
}
