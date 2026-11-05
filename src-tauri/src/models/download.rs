//! Model downloads: the only network traffic Murmur makes, and only after
//! the user clicks Download. Each file downloads to name.part, resumes from
//! where it stopped with an HTTP Range request, and is only renamed into
//! place once its hash matches the catalogue.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use super::catalog::{CatalogModel, ModelFile};

/// Returned (inside anyhow) when the user cancels. Partial files stay so the
/// next attempt resumes.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("download cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub fn is_cancelled(err: &anyhow::Error) -> bool {
    err.downcast_ref::<Cancelled>().is_some()
}

/// Returned (inside anyhow) when the drive cannot hold what is left to
/// download, checked before starting.
#[derive(Debug)]
pub struct NoSpace {
    /// Bytes still missing beyond what the drive has free.
    pub short: u64,
}

impl std::fmt::Display for NoSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not enough disk space ({} bytes short)", self.short)
    }
}

impl std::error::Error for NoSpace {}

/// Kept free after a download, so the drive is never filled to the brim.
const SPARE_BYTES: u64 = 512 * 1024 * 1024;

/// Bytes free for this user on the volume holding `dir`.
#[cfg(target_os = "macos")]
fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: `path` is NUL terminated and `stats` is a valid out pointer.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)]
    Some(stats.f_bavail as u64 * stats.f_frsize as u64)
}

/// Bytes free for this user on the drive holding `dir`.
#[cfg(windows)]
fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is a NUL terminated path that outlives the call.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}

/// Bytes still to download for `model` into `dir`, counting partial files.
fn remaining(model: &CatalogModel, dir: &Path) -> u64 {
    model
        .files
        .iter()
        .map(|file| {
            let target = dir.join(&file.name);
            if file_complete(&target, file) {
                return 0;
            }
            let have = fs::metadata(part_path(&target)).map_or(0, |m| m.len());
            // An archive needs room for itself and what it unpacks to,
            // which for these models is under twice its size.
            let unpacked = if file.unzip { 2 * file.size } else { 0 };
            file.size.saturating_sub(have) + unpacked
        })
        .sum()
}

/// How often progress is reported.
const PROGRESS_EVERY: Duration = Duration::from_millis(150);

/// Downloads every missing file of `model` into `dir`. `on_progress` gets
/// (bytes done, total bytes) across the whole model.
pub fn download_model(
    model: &CatalogModel,
    dir: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    if let Some(free) = free_bytes(dir) {
        let needed = remaining(model, dir) + SPARE_BYTES;
        if needed > free {
            return Err(NoSpace { short: needed - free }.into());
        }
    }
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        // For the blocking client this limits each read or write, not the
        // whole download: big files on slow links are fine, a stall fails.
        .timeout(Duration::from_secs(60))
        .build()
        .context("could not start the downloader")?;

    let total = model.size();
    let mut done_before = 0u64;
    for file in &model.files {
        let target = dir.join(&file.name);
        if file_complete(&target, file) {
            done_before += file.size;
            on_progress(done_before, total);
            continue;
        }
        if !(file.unzip && target.is_file() && !part_path(&target).exists()) {
            download_file(&client, file, &target, cancel, |bytes| {
                on_progress(done_before + bytes, total)
            })?;
        }
        if file.unzip {
            unpack(&target, dir, cancel)?;
        }
        done_before += file.size;
    }
    on_progress(total, total);
    Ok(())
}

/// A finished file: present with the catalogue size, or for an archive,
/// unpacked. Hashes are checked when a download finishes, not on every start.
pub fn file_complete(path: &Path, file: &ModelFile) -> bool {
    if file.unzip {
        return unpacked_marker(path).is_file();
    }
    fs::metadata(path).is_ok_and(|m| m.len() == file.size)
}

fn unpacked_marker(archive: &Path) -> PathBuf {
    let mut name = archive.file_name().unwrap_or_default().to_os_string();
    name.push(".unpacked");
    archive.with_file_name(name)
}

/// Unpacks `archive` into `dir`, dropping a top folder every entry shares,
/// then deletes it and leaves the marker. Entries that would land outside
/// `dir` are refused.
fn unpack(archive: &Path, dir: &Path, cancel: &AtomicBool) -> Result<()> {
    let file = File::open(archive).with_context(|| format!("could not open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("not a zip archive")?;
    let names: Vec<PathBuf> = (0..zip.len())
        .map(|i| {
            zip.by_index(i)
                .ok()
                .and_then(|e| e.enclosed_name())
                .context("archive has an unsafe path")
        })
        .collect::<Result<_>>()?;
    let top = names
        .first()
        .and_then(|p| p.components().next())
        .filter(|first| names.iter().all(|p| p.components().count() > 1 && p.components().next() == Some(*first)))
        .map(|c| PathBuf::from(c.as_os_str()));
    for (i, name) in names.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Err(Cancelled.into());
        }
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let relative = top.as_ref().map_or(name.as_path(), |t| name.strip_prefix(t).unwrap_or(name));
        let out = dir.join(relative);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut writer = File::create(&out).with_context(|| format!("could not write {}", out.display()))?;
        std::io::copy(&mut entry, &mut writer).with_context(|| format!("could not unpack {}", name.display()))?;
    }
    fs::remove_file(archive).ok();
    fs::write(unpacked_marker(archive), b"").context("could not finish unpacking")?;
    Ok(())
}

fn part_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    target.with_file_name(name)
}

/// Hard failures (refused, reset, timed out) allowed per source before the
/// download gives up.
const TRIES_PER_SOURCE: usize = 3;
/// With a mirror to fall back on, a connection slower than this after
/// SLOW_AFTER moves on to the next source.
const SLOW_BYTES_PER_SEC: u64 = 100 * 1024;
const SLOW_AFTER: Duration = Duration::from_secs(8);

/// Moved on from a source that was too slow; not counted as a failure.
#[derive(Debug)]
struct TooSlow;

impl std::fmt::Display for TooSlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("source too slow")
    }
}

impl std::error::Error for TooSlow {}

fn download_file(
    client: &reqwest::blocking::Client,
    file: &ModelFile,
    target: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64),
) -> Result<()> {
    let part = part_path(target);
    let sources: Vec<&str> = std::iter::once(file.url.as_str())
        .chain(file.mirrors.iter().map(String::as_str))
        .collect();
    let mut failures = 0;
    let mut current = 0;
    loop {
        let url = sources[current];
        match fetch(client, url, file, &part, cancel, sources.len() > 1, &mut on_progress) {
            Ok(()) => break,
            Err(err) if is_cancelled(&err) => return Err(err),
            Err(err) => {
                if err.downcast_ref::<TooSlow>().is_none() {
                    failures += 1;
                    if failures >= TRIES_PER_SOURCE * sources.len() {
                        return Err(err);
                    }
                }
                log::warn!("{} from {}: {err:#}; trying again", file.name, host(url));
                current = (current + 1) % sources.len();
            }
        }
    }

    let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have != file.size {
        bail!(
            "{} is {} bytes, expected {}; try again",
            file.name,
            have,
            file.size
        );
    }
    if !hash_matches(&part, file)? {
        fs::remove_file(&part).ok();
        bail!("{} failed its integrity check; try again", file.name);
    }
    fs::rename(&part, target).with_context(|| format!("could not finish {}", file.name))?;
    Ok(())
}

/// Appends to `part` from `url` until the file is complete, resuming with
/// a Range request. Every source serves the same bytes (the hash checks it),
/// so a download can switch sources part way.
fn fetch(
    client: &reqwest::blocking::Client,
    url: &str,
    file: &ModelFile,
    part: &Path,
    cancel: &AtomicBool,
    give_up_when_slow: bool,
    on_progress: &mut impl FnMut(u64),
) -> Result<()> {
    let mut have = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if have > file.size {
        fs::remove_file(part).ok();
        have = 0;
    }
    if have == file.size {
        return Ok(());
    }

    let mut request = client.get(url);
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut response = request
        .send()
        .with_context(|| format!("could not reach {}", host(url)))?;
    let status = response.status();
    let append = match status.as_u16() {
        206 => true,
        200 => {
            // The server ignored the range: start over.
            have = 0;
            false
        }
        416 => bail!("the server refused to resume {}", file.name),
        _ => bail!(
            "download of {} failed with HTTP {}",
            file.name,
            status.as_u16()
        ),
    };

    let mut out = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(part)
        .with_context(|| format!("could not write {}", part.display()))?;

    let mut buf = vec![0u8; 256 * 1024];
    let mut last_report = Instant::now();
    let started = Instant::now();
    let mut fetched = 0u64;
    let mut checked_speed = false;
    on_progress(have);
    loop {
        if cancel.load(Ordering::SeqCst) {
            out.flush().ok();
            return Err(Cancelled.into());
        }
        let n = response
            .read(&mut buf)
            .with_context(|| format!("connection lost while downloading {}", file.name))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .with_context(|| format!("could not write {}", part.display()))?;
        have += n as u64;
        fetched += n as u64;
        if last_report.elapsed() >= PROGRESS_EVERY {
            on_progress(have);
            last_report = Instant::now();
        }
        if give_up_when_slow && !checked_speed && started.elapsed() >= SLOW_AFTER {
            checked_speed = true;
            if fetched < SLOW_BYTES_PER_SEC * started.elapsed().as_secs() {
                out.flush().ok();
                on_progress(have);
                return Err(TooSlow.into());
            }
        }
    }
    out.flush()?;
    on_progress(have);
    Ok(())
}

fn hash_matches(path: &Path, file: &ModelFile) -> Result<bool> {
    let mut f = File::open(path)?;
    let mut buf = vec![0u8; 1024 * 1024];
    if let Some(expected) = &file.sha256 {
        let mut h = Sha256::new();
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        return Ok(hex::encode(h.finalize()) == expected.to_lowercase());
    }
    if let Some(expected) = &file.git_sha1 {
        // git hashes "blob <size>\0" followed by the content.
        let mut h = Sha1::new();
        h.update(format!("blob {}\0", file.size).as_bytes());
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        return Ok(hex::encode(h.finalize()) == expected.to_lowercase());
    }
    Err(anyhow!("{} has no hash in the catalogue", file.name))
}

fn host(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::catalog::Family;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::Arc;

    /// A tiny HTTP server for one body that honours Range requests. With
    /// `slow`, it streams in small chunks like a real connection.
    fn serve(body: Vec<u8>, honour_range: bool) -> String {
        serve_with(body, honour_range, false)
    }

    fn serve_with(body: Vec<u8>, honour_range: bool, slow: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body = Arc::new(body);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let body = body.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut start = 0usize;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(r) = line.to_lowercase().strip_prefix("range: bytes=") {
                            start = r.trim().trim_end_matches('-').parse().unwrap();
                        }
                    }
                    let mut stream = stream;
                    let (status, slice) = if honour_range && start > 0 {
                        ("206 Partial Content", &body[start..])
                    } else {
                        ("200 OK", &body[..])
                    };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        slice.len()
                    );
                    if slow {
                        for chunk in slice.chunks(64 * 1024) {
                            if stream.write_all(chunk).is_err() {
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(15));
                        }
                    } else {
                        let _ = stream.write_all(slice);
                    }
                });
            }
        });
        format!("http://{addr}/model.bin")
    }

    fn model_for(url: &str, body: &[u8]) -> CatalogModel {
        CatalogModel {
            id: "test".into(),
            name: "Test".into(),
            family: Family::Whisper,
            accuracy: 1,
            speed: 1,
            languages: vec!["*".into()],
            languages_label: String::new(),
            ram_mb: 1,
            default: false,
            note: None,
            thinking: false,
            npu: false,
            open_gain: None,
            files: vec![ModelFile {
                name: "model.bin".into(),
                url: url.into(),
                size: body.len() as u64,
                sha256: Some(hex::encode(Sha256::digest(body))),
                git_sha1: None,
                mirrors: Vec::new(),
                unzip: false,
            }],
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("murmur-dl-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        dir
    }

    fn body(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    #[test]
    fn a_model_bigger_than_the_free_space_is_refused_before_starting() {
        let dir = temp_dir("space");
        fs::create_dir_all(&dir).unwrap();
        let free = free_bytes(&dir).unwrap();
        assert!(free > 0);
        let mut model = model_for("http://127.0.0.1:9/never", b"x");
        model.files[0].size = free + 1;
        let err = download_model(&model, &dir, &AtomicBool::new(false), |_, _| {}).unwrap_err();
        let short = err.downcast_ref::<NoSpace>().unwrap().short;
        assert!(short > SPARE_BYTES, "{short}");
        assert!(!dir.join("model.bin.part").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remaining_counts_partial_files() {
        let data = body(1000);
        let model = model_for("http://127.0.0.1:9/never", &data);
        let dir = temp_dir("remaining");
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(remaining(&model, &dir), 1000);
        fs::write(dir.join("model.bin.part"), &data[..300]).unwrap();
        assert_eq!(remaining(&model, &dir), 700);
        fs::write(dir.join("model.bin"), &data).unwrap();
        assert_eq!(remaining(&model, &dir), 0);
        fs::remove_dir_all(&dir).ok();
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write as _;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn an_archive_is_unpacked_without_its_top_folder_and_then_removed() {
        let data = zip_of(&[("pkg/encoder.onnx", b"enc"), ("pkg/sub/decoder.data", b"dec")]);
        let mut model = model_for(&serve(data.clone(), true), &data);
        model.files[0].name = "model.zip".into();
        model.files[0].unzip = true;
        let dir = temp_dir("unzip");
        assert!(!file_complete(&dir.join("model.zip"), &model.files[0]));
        download_model(&model, &dir, &AtomicBool::new(false), |_, _| {}).unwrap();
        assert_eq!(fs::read(dir.join("encoder.onnx")).unwrap(), b"enc");
        assert_eq!(fs::read(dir.join("sub").join("decoder.data")).unwrap(), b"dec");
        assert!(!dir.join("model.zip").exists());
        assert!(file_complete(&dir.join("model.zip"), &model.files[0]));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_archive_reaching_outside_the_folder_is_refused() {
        let dir = temp_dir("zipslip");
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("bad.zip");
        fs::write(&archive, zip_of(&[("../escaped.txt", b"x")])).unwrap();
        assert!(unpack(&archive, &dir.join("model"), &AtomicBool::new(false)).is_err());
        assert!(!dir.join("escaped.txt").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn downloads_and_verifies() {
        let data = body(3_000_000);
        let model = model_for(&serve(data.clone(), true), &data);
        let dir = temp_dir("ok");
        let mut last = (0, 0);
        download_model(&model, &dir, &AtomicBool::new(false), |d, t| last = (d, t)).unwrap();
        assert_eq!(fs::read(dir.join("model.bin")).unwrap(), data);
        assert!(!dir.join("model.bin.part").exists());
        assert_eq!(last, (data.len() as u64, data.len() as u64));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cancel_keeps_the_part_file_and_the_next_run_resumes() {
        let data = body(4_000_000);
        let model = model_for(&serve_with(data.clone(), true, true), &data);
        let dir = temp_dir("resume");
        let cancel = AtomicBool::new(false);
        let err = download_model(&model, &dir, &cancel, |done, _| {
            if done > 1_000_000 {
                cancel.store(true, Ordering::SeqCst);
            }
        })
        .unwrap_err();
        assert!(is_cancelled(&err));
        let partial = fs::metadata(dir.join("model.bin.part")).unwrap().len();
        assert!(partial > 0 && partial < data.len() as u64, "{partial}");

        let mut first_report = None;
        download_model(&model, &dir, &AtomicBool::new(false), |d, _| {
            first_report.get_or_insert(d);
        })
        .unwrap();
        // Resumed from the partial file, not from zero.
        assert_eq!(first_report, Some(partial));
        assert_eq!(fs::read(dir.join("model.bin")).unwrap(), data);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn server_without_range_support_restarts_cleanly() {
        let data = body(500_000);
        let model = model_for(&serve(data.clone(), false), &data);
        let dir = temp_dir("norange");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("model.bin.part"), &data[..1000]).unwrap();
        download_model(&model, &dir, &AtomicBool::new(false), |_, _| {}).unwrap();
        assert_eq!(fs::read(dir.join("model.bin")).unwrap(), data);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_download_is_rejected_and_removed() {
        let data = body(200_000);
        let mut model = model_for(&serve(data.clone(), true), &data);
        model.files[0].sha256 = Some("0".repeat(64));
        let dir = temp_dir("bad");
        let err = download_model(&model, &dir, &AtomicBool::new(false), |_, _| {}).unwrap_err();
        assert!(err.to_string().contains("integrity"), "{err}");
        assert!(!dir.join("model.bin").exists());
        assert!(!dir.join("model.bin.part").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn falls_back_to_a_mirror_when_the_main_source_is_down() {
        let data = body(300_000);
        let mirror = serve(data.clone(), true);
        // Nothing listens on port 1.
        let mut model = model_for("http://127.0.0.1:1/model.bin", &data);
        model.files[0].mirrors = vec![mirror];
        let dir = temp_dir("mirror");
        download_model(&model, &dir, &AtomicBool::new(false), |_, _| {}).unwrap();
        assert_eq!(fs::read(dir.join("model.bin")).unwrap(), data);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn complete_files_are_skipped() {
        let data = body(1000);
        // Nothing listens here: any request would fail.
        let model = model_for("http://127.0.0.1:9/model.bin", &data);
        let dir = temp_dir("skip");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("model.bin"), &data).unwrap();
        download_model(&model, &dir, &AtomicBool::new(false), |_, _| {}).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    /// Downloads a real catalogue model into the user's models folder with
    /// the real downloader. Skipped by default:
    ///   MURMUR_FETCH=parakeet-tdt-0.6b-v2 cargo test fetch_real_model -- --ignored --nocapture
    #[test]
    #[ignore = "downloads hundreds of MB from Hugging Face"]
    fn fetch_real_model() {
        let id = std::env::var("MURMUR_FETCH").expect("set MURMUR_FETCH to a model id");
        let model = crate::models::catalog::find(&id).expect("unknown model id");
        let dir = crate::paths::data_dir_named(&crate::paths::product_name())
            .unwrap()
            .join("models")
            .join(&id);
        let started = Instant::now();
        let mut last = Instant::now();
        download_model(&model, &dir, &AtomicBool::new(false), |done, total| {
            if last.elapsed() > Duration::from_secs(5) || done == total {
                println!(
                    "{:>6.1}% of {} MB",
                    done as f64 * 100.0 / total as f64,
                    total / 1_000_000
                );
                last = Instant::now();
            }
        })
        .unwrap();
        println!(
            "done in {} s: {}",
            started.elapsed().as_secs(),
            dir.display()
        );
    }

    #[test]
    fn git_sha1_matches_git() {
        // `git hash-object` of "hello\n" is ce013625030ba8dba906f756967f9e9ca394464a.
        let dir = temp_dir("gitsha");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v.txt");
        fs::write(&path, b"hello\n").unwrap();
        let file = ModelFile {
            name: "v.txt".into(),
            url: String::new(),
            size: 6,
            sha256: None,
            git_sha1: Some("ce013625030ba8dba906f756967f9e9ca394464a".into()),
            mirrors: Vec::new(),
            unzip: false,
        };
        assert!(hash_matches(&path, &file).unwrap());
        fs::remove_dir_all(&dir).ok();
    }
}
