//! App icons for History rows. The first dictation into an app extracts its
//! icon from the .exe, and it is saved once as a PNG under
//! %APPDATA%\Murmur\icons\<exe>.png. History reads the saved file.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::Engine;
use windows::core::HSTRING;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

/// File name for an app's icon: the exe name, lowercased, made safe.
pub fn icon_file(dir: &Path, exe: &str) -> Option<PathBuf> {
    let name: String = exe
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    (!name.is_empty() && !name.starts_with('.')).then(|| dir.join(format!("{name}.png")))
}

/// Saves the icon for `exe_path` unless one is already saved.
pub fn ensure_saved(dir: &Path, exe_name: &str, exe_path: &str) -> Result<()> {
    let Some(target) = icon_file(dir, exe_name) else {
        return Ok(());
    };
    if target.exists() || exe_path.is_empty() {
        return Ok(());
    }
    let png = extract_png(exe_path)?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(&target, png).with_context(|| format!("could not save {}", target.display()))
}

/// The saved icon as a data URL for an <img>, if there is one.
pub fn data_url(dir: &Path, exe_name: &str) -> Option<String> {
    let bytes = std::fs::read(icon_file(dir, exe_name)?).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Extracts the exe's large icon as a 32x32 PNG with transparency.
fn extract_png(exe_path: &str) -> Result<Vec<u8>> {
    let mut icon = HICON::default();
    // SAFETY: Win32 calls with valid out pointers; every handle created
    // here is released before returning.
    unsafe {
        let found = ExtractIconExW(&HSTRING::from(exe_path), 0, Some(&mut icon), None, 1);
        if found == 0 || icon.is_invalid() {
            bail!("no icon in {exe_path}");
        }
        let result = icon_to_png(icon);
        let _ = DestroyIcon(icon);
        result
    }
}

unsafe fn icon_to_png(icon: HICON) -> Result<Vec<u8>> {
    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut info) }.context("could not read the icon")?;
    let result = unsafe { bitmaps_to_png(info.hbmColor, info.hbmMask) };
    unsafe {
        let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
    }
    result
}

/// Reads a bitmap as top down 32 bit BGRA.
unsafe fn read_bgra(bitmap: HBITMAP, width: i32, height: i32) -> Result<Vec<u8>> {
    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // top down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let dc = unsafe { GetDC(None) };
    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        )
    };
    unsafe { ReleaseDC(None, dc) };
    if lines == 0 {
        bail!("could not read the icon pixels");
    }
    Ok(pixels)
}

unsafe fn bitmaps_to_png(color: HBITMAP, mask: HBITMAP) -> Result<Vec<u8>> {
    if color.is_invalid() {
        bail!("monochrome icons are not supported");
    }
    let mut bm = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            HGDIOBJ(color.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut bm as *mut BITMAP).cast()),
        )
    };
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        bail!("could not read the icon size");
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    let mut bgra = unsafe { read_bgra(color, w, h)? };

    // Old icons carry no alpha; their transparency is in the mask, where
    // white means transparent.
    if bgra.chunks(4).all(|p| p[3] == 0) {
        let mask = unsafe { read_bgra(mask, w, h)? };
        for (px, m) in bgra.chunks_mut(4).zip(mask.chunks(4)) {
            px[3] = if m[0] == 0 { 255 } else { 0 };
        }
    }
    let rgba: Vec<u8> = bgra
        .chunks(4)
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();
    encode_png(&rgba, w as u32, h as u32)
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .context("could not encode the icon")?;
    writer
        .write_image_data(rgba)
        .context("could not encode the icon")?;
    writer.finish().context("could not encode the icon")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_file_names_are_safe() {
        let dir = Path::new("C:/icons");
        assert_eq!(icon_file(dir, "Code.exe"), Some(dir.join("code.exe.png")));
        assert_eq!(icon_file(dir, "a b/c.exe"), Some(dir.join("a_b_c.exe.png")));
        assert_eq!(icon_file(dir, ""), None);
        assert_eq!(icon_file(dir, "..\\x"), None);
    }

    #[test]
    fn extracts_a_real_icon() {
        let png = extract_png("C:\\Windows\\System32\\notepad.exe")
            .or_else(|_| extract_png("C:\\Windows\\explorer.exe"))
            .unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(png.len() > 100);
    }

    #[test]
    fn saves_once_and_serves_a_data_url() {
        let dir = std::env::temp_dir().join(format!("murmur-icons-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        ensure_saved(&dir, "explorer.exe", "C:\\Windows\\explorer.exe").unwrap();
        let url = data_url(&dir, "Explorer.exe").unwrap();
        assert!(url.starts_with("data:image/png;base64,iVBOR"));
        assert!(data_url(&dir, "missing.exe").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
