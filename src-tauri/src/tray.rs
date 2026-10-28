//! Tray icon (spec 6.10): Open Murmur, Start dictation, Pause hotkeys, a
//! Model submenu with the installed models, and Quit. The icon gets a red dot
//! while recording. The menu is rebuilt whenever models or state change.

use std::sync::Mutex;

use anyhow::{Context, Result};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager, Wry};

use crate::dictation::Dictation;
use crate::models::manager::ModelManager;

const OPEN: &str = "open";
const DICTATE: &str = "dictate";
const PAUSE: &str = "pause";
const QUIT: &str = "quit";
const MODEL_PREFIX: &str = "model:";

pub struct Tray {
    icon: TrayIcon<Wry>,
    idle: Image<'static>,
    recording: Image<'static>,
    /// Whether the recording icon is up, so it only changes on a change.
    showing_recording: Mutex<bool>,
}

/// The app icon with a red dot in the bottom right corner.
pub fn recording_icon(base: &Image<'_>) -> Image<'static> {
    let (w, h) = (base.width() as i64, base.height() as i64);
    let mut rgba = base.rgba().to_vec();
    let r = (w.min(h) as f64 * 0.24).max(2.0);
    let (cx, cy) = (w as f64 - r - 0.5, h as f64 - r - 0.5);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
            // White ring so the dot reads on any taskbar colour.
            let colour = if d <= r {
                [0xEF, 0x44, 0x44, 0xFF]
            } else if d <= r + 1.2 {
                [0xFF, 0xFF, 0xFF, 0xFF]
            } else {
                continue;
            };
            let i = ((y * w + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&colour);
        }
    }
    Image::new_owned(rgba, w as u32, h as u32)
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn menu(app: &AppHandle, recording: bool) -> Result<Menu<Wry>> {
    let dictation = app.state::<Dictation>();
    let models = app.state::<std::sync::Arc<ModelManager>>();
    let active = models.active();
    let installed = models.installed();

    let model_items: Vec<CheckMenuItem<Wry>> = installed
        .iter()
        .map(|(id, name)| {
            CheckMenuItem::with_id(
                app,
                format!("{MODEL_PREFIX}{id}"),
                name,
                true,
                active.as_deref() == Some(id.as_str()),
                None::<&str>,
            )
        })
        .collect::<tauri::Result<_>>()?;
    let model_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = model_items
        .iter()
        .map(|i| i as &dyn tauri::menu::IsMenuItem<Wry>)
        .collect();
    let model_menu = Submenu::with_items(app, "Model", !installed.is_empty(), &model_refs)?;

    let dictate_label = if recording {
        "Stop dictation"
    } else {
        "Start dictation"
    };
    Ok(Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, OPEN, "Open Murmur", true, None::<&str>)?,
            &MenuItem::with_id(app, DICTATE, dictate_label, true, None::<&str>)?,
            &CheckMenuItem::with_id(
                app,
                PAUSE,
                "Pause hotkeys",
                true,
                dictation.hotkeys().paused(),
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &model_menu,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?,
        ],
    )?)
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        OPEN => show_main(app),
        DICTATE => app.state::<Dictation>().toggle(),
        PAUSE => {
            let hotkeys = app.state::<Dictation>().hotkeys().clone();
            hotkeys.set_paused(!hotkeys.paused());
            refresh(app);
        }
        QUIT => app.exit(0),
        other => {
            if let Some(model) = other.strip_prefix(MODEL_PREFIX) {
                let models = app.state::<std::sync::Arc<ModelManager>>();
                if let Err(err) = models.activate(model) {
                    log::warn!("could not switch to {model}: {err:#}");
                }
            }
        }
    }
}

/// Rebuilds the menu and picks the icon for the current state.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    let recording = app.state::<Dictation>().is_recording();
    match menu(app, recording) {
        Ok(m) => {
            let _ = tray.icon.set_menu(Some(m));
        }
        Err(err) => log::warn!("could not build the tray menu: {err:#}"),
    }
    if let Ok(mut showing) = tray.showing_recording.lock() {
        if *showing != recording {
            *showing = recording;
            let image = if recording {
                tray.recording.clone()
            } else {
                tray.idle.clone()
            };
            let _ = tray.icon.set_icon(Some(image));
        }
    };
}

pub fn set_visible(app: &AppHandle, visible: bool) {
    if let Some(tray) = app.try_state::<Tray>() {
        let _ = tray.icon.set_visible(visible);
    }
}

pub fn setup(app: &AppHandle, visible: bool) -> Result<()> {
    let idle = app
        .default_window_icon()
        .context("app has no icon")?
        .clone()
        .to_owned();
    let recording = recording_icon(&idle);
    let icon = TrayIconBuilder::with_id("main")
        .icon(idle.clone())
        .tooltip(app.package_info().name.clone())
        .menu(&menu(app, false)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)
        .context("could not create the tray icon")?;
    icon.set_visible(visible)?;
    app.manage(Tray {
        icon,
        idle,
        recording,
        showing_recording: Mutex::new(false),
    });

    for event in ["models-changed", "dictation-state"] {
        let handle = app.clone();
        app.listen(event, move |_| refresh(&handle));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_icon_adds_a_red_dot_in_the_corner() {
        let base = Image::new_owned(vec![0; 32 * 32 * 4], 32, 32);
        let rec = recording_icon(&base);
        let px = |x: usize, y: usize| &rec.rgba()[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(px(25, 25), &[0xEF, 0x44, 0x44, 0xFF]);
        assert_eq!(px(2, 2), &[0, 0, 0, 0]);
        assert_eq!((rec.width(), rec.height()), (32, 32));
    }
}
