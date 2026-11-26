use eframe::egui;
use semver::Version;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io;
#[cfg(target_os = "windows")]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};
#[cfg(target_os = "macos")]
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

const LATEST: &str = "https://api.github.com/repos/RoboticsY/dbml-studio/releases/latest";
const DOWNLOADS: &str = "https://github.com/RoboticsY/dbml-studio/releases/download";
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(30 * 60);
const MAX_INSTALLER_SIZE: u64 = 100 * 1024 * 1024;
const PLATFORM: Option<&str> = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("macos-arm64.dmg")
} else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
    Some("windows-x64-setup.exe")
} else {
    None
};

#[derive(Clone)]
struct Release {
    version: Version,
    filename: String,
    digest: String,
    size: u64,
}

struct Installer {
    directory: TempDir,
    path: PathBuf,
}

type Task<T> = Receiver<Result<T, String>>;

#[derive(Default)]
pub struct Updates {
    next_check: Option<Instant>,
    check: Option<Task<Option<Release>>>,
    release: Option<Release>,
    download: Option<Task<Installer>>,
    installer: Option<Installer>,
    error: Option<String>,
}

impl Updates {
    pub fn toolbar(&mut self, ui: &mut egui::Ui, can_close: bool) -> bool {
        let Some(platform) = PLATFORM else {
            return false;
        };
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        if let Some(result) = self.check.as_ref().and_then(poll) {
            self.check = None;
            self.next_check = Some(
                now + if result.is_ok() {
                    CHECK_INTERVAL
                } else {
                    RETRY_INTERVAL
                },
            );
            if let Ok(release) = result {
                self.release = release;
            }
        }
        if let Some(result) = self.download.as_ref().and_then(poll) {
            self.download = None;
            match result {
                Ok(installer) => self.installer = Some(installer),
                Err(error) => self.error = Some(error),
            }
        }
        if self.check.is_none() && self.download.is_none() && self.installer.is_none() {
            match self.next_check {
                Some(next) if next > now => ctx.request_repaint_after(next - now),
                _ => self.check = Some(worker(&ctx, move || check_release(platform))),
            }
        }
        let Some(release) = &self.release else {
            return false;
        };
        ui.separator();
        if self.download.is_some() {
            ui.spinner();
            ui.label("アップデートをダウンロード中");
        } else {
            let label = if self.installer.is_some() {
                format!("インストーラーを開く v{}", release.version)
            } else {
                format!("⬆ アップデート v{}", release.version)
            };
            let response = ui
                .add_enabled(can_close, egui::Button::new(label))
                .on_hover_text("インストーラーを開いて DBML Studio を終了します")
                .on_disabled_hover_text("変更を保存してからアップデートしてください");
            if response.clicked() {
                self.error = None;
                self.check = None;
                if let Some(installer) = self.installer.take() {
                    return self.open(installer);
                }
                let release = release.clone();
                self.download = Some(worker(&ctx, move || fetch_installer(&release)));
            }
        }
        if let Some(error) = &self.error {
            ui.label("アップデートできませんでした")
                .on_hover_text(error);
        }
        if can_close && self.error.is_none() {
            if let Some(installer) = self.installer.take() {
                return self.open(installer);
            }
        }
        false
    }

    fn open(&mut self, installer: Installer) -> bool {
        match open_installer(&installer.path) {
            Ok(()) => {
                // The OS opens the installer after this copy of DBML Studio exits.
                let _ = installer.directory.keep();
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                self.installer = Some(installer);
                false
            }
        }
    }
}

fn worker<T: Send + 'static>(
    ctx: &egui::Context,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Task<T> {
    let ctx = ctx.clone();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
        ctx.request_repaint();
    });
    receiver
}

fn poll<T>(task: &Task<T>) -> Option<Result<T, String>> {
    match task.try_recv() {
        Ok(result) => Some(result),
        Err(TryRecvError::Disconnected) => Some(Err("更新処理が終了しました。".into())),
        Err(TryRecvError::Empty) => None,
    }
}

fn curl() -> Command {
    #[cfg(target_os = "windows")]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let mut command = Command::new(PathBuf::from(root).join(r"System32\curl.exe"));
        command.creation_flags(0x0800_0000);
        command
    };
    #[cfg(not(target_os = "windows"))]
    let mut command = Command::new("/usr/bin/curl");

    command.args([
        "-q",
        "--fail",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        "8",
        "--user-agent",
        concat!("DBML-Studio/", env!("CARGO_PKG_VERSION")),
    ]);
    command
}

fn check_release(platform: &str) -> Result<Option<Release>, String> {
    let output = curl()
        .args(["--max-time", "15", "--max-filesize", "1048576", LATEST])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().into());
    }
    let metadata: Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    Ok(select_release(
        &metadata,
        &Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
        platform,
    ))
}

fn select_release(metadata: &Value, current: &Version, platform: &str) -> Option<Release> {
    if metadata["draft"].as_bool()? || metadata["prerelease"].as_bool()? {
        return None;
    }
    let version = Version::parse(metadata["tag_name"].as_str()?.strip_prefix('v')?).ok()?;
    if &version <= current || !version.pre.is_empty() || !version.build.is_empty() {
        return None;
    }
    let filename = format!("DBML-Studio-{version}-{platform}");
    let asset = metadata["assets"].as_array()?.iter().find(|asset| {
        asset["name"].as_str() == Some(&filename) && asset["state"].as_str() == Some("uploaded")
    })?;
    let digest = asset["digest"].as_str()?.strip_prefix("sha256:")?;
    let size = asset["size"].as_u64()?;
    if digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !(1..=MAX_INSTALLER_SIZE).contains(&size)
    {
        return None;
    }
    Some(Release {
        version,
        filename,
        digest: digest.to_ascii_lowercase(),
        size,
    })
}

fn fetch_installer(release: &Release) -> Result<Installer, String> {
    let directory = tempfile::Builder::new()
        .prefix("dbml-studio-update-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let path = directory.path().join(&release.filename);
    let url = format!("{DOWNLOADS}/v{}/{}", release.version, release.filename);
    let status = curl()
        .args([
            "--location",
            "--max-redirs",
            "4",
            "--max-time",
            "120",
            "--max-filesize",
        ])
        .arg(release.size.to_string())
        .arg("--output")
        .arg(&path)
        .arg(url)
        .stderr(Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("ダウンロードに失敗しました（{status}）。"));
    }
    let mut file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let bytes = io::copy(&mut file, &mut hash).map_err(|error| error.to_string())?;
    if bytes != release.size || format!("{:x}", hash.finalize()) != release.digest {
        return Err("ダウンロードしたファイルの SHA-256 が一致しません。".into());
    }
    mark_download(&path).map_err(|error| error.to_string())?;
    Ok(Installer { directory, path })
}

fn mark_download(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        let mut stream = path.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        std::fs::File::create(stream)?.write_all(b"[ZoneTransfer]\r\nZoneId=3\r\n")
    }
    #[cfg(target_os = "macos")]
    {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let status = Command::new("/usr/bin/xattr")
            .args(["-w", "com.apple.quarantine"])
            .arg(format!("0081;{timestamp:x};DBML Studio;"))
            .arg(path)
            .status()?;
        if !status.success() {
            return Err(io::Error::other(
                "ダウンロード元の情報を設定できませんでした。",
            ));
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn open_installer(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // The path is a NUL-terminated UTF-16 string and lives through the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            null(),
            path.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        return Err(io::Error::other(format!(
            "インストーラーを開けませんでした（{}）。",
            result as isize
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn open_installer(path: &Path) -> io::Result<()> {
    let status = Command::new("/usr/bin/open").arg(path).status()?;
    if !status.success() {
        return Err(io::Error::other("ディスクイメージを開けませんでした。"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selects_only_a_newer_stable_release_with_a_verified_platform_asset() {
        let current = Version::new(0, 3, 9);
        let platform = "macos-arm64.dmg";
        let mut metadata = json!({
            "tag_name": "v0.3.10",
            "draft": false,
            "prerelease": false,
            "assets": [{
                "name": "DBML-Studio-0.3.10-macos-arm64.dmg",
                "state": "uploaded",
                "size": 1234,
                "digest": format!("sha256:{}", "a".repeat(64)),
            }],
        });
        let release = select_release(&metadata, &current, platform).unwrap();
        assert_eq!(release.version, Version::new(0, 3, 10));
        assert!(select_release(&metadata, &release.version, platform).is_none());
        assert!(select_release(&metadata, &current, "windows-x64-setup.exe").is_none());
        metadata["prerelease"] = json!(true);
        assert!(select_release(&metadata, &current, platform).is_none());
        metadata["prerelease"] = json!(false);
        metadata["assets"][0]["digest"] = json!("sha256:missing");
        assert!(select_release(&metadata, &current, platform).is_none());
    }
}
