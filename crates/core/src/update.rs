//! Self-update shared by the CLI (`mrimport self-update`) and the desktop app.
//!
//! The flow mirrors the installers in `install/`: find the newest release, download each
//! asset and verify its SHA-256 against the release's `SHA256SUMS` *before touching anything
//! installed*, remove the old program files (never settings or credentials), then install the
//! new ones.

use crate::{events::Reporter, Error, Result};
use futures::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub const REPO: &str = "Aoof/modrinth-importer";
const API: &str = "https://api.github.com";

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct Release {
    /// Version without the leading `v`.
    pub version: String,
    pub assets: Vec<Asset>,
}

impl Release {
    fn asset(&self, name: &str) -> Result<&Asset> {
        self.assets
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| Error::Invalid(format!("the release has no asset named {name}")))
    }
}

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(crate::user_agent())
        .build()?)
}

/// The newest release. Releases are pre-releases (developer builds), which GitHub's
/// `/releases/latest` alias skips, so take the newest entry of the list.
pub async fn latest(http: &reqwest::Client) -> Result<Release> {
    latest_from(http, &format!("{API}/repos/{REPO}/releases?per_page=1")).await
}

pub async fn latest_from(http: &reqwest::Client, url: &str) -> Result<Release> {
    #[derive(Deserialize)]
    struct Raw {
        tag_name: String,
        #[serde(default)]
        assets: Vec<Asset>,
    }
    let list: Vec<Raw> = http
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let raw = list
        .into_iter()
        .next()
        .ok_or_else(|| Error::Invalid("no releases found".into()))?;
    Ok(Release {
        version: raw.tag_name.trim_start_matches('v').to_string(),
        assets: raw.assets,
    })
}

fn parse_version(v: &str) -> Option<Vec<u64>> {
    let core = v.trim_start_matches('v').split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// True when `candidate` is a strictly newer version than `current` (never offers a downgrade).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => candidate != current,
    }
}

/// Name of the CLI asset for a build target triple.
pub fn cli_asset(target: &str) -> String {
    let ext = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    format!("mrimport-{target}{ext}")
}

/// Name of the desktop-app installer asset for this platform.
pub fn gui_asset() -> Option<String> {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" if cfg!(windows) => "x64", // runs the x64 build through emulation
        "aarch64" => "aarch64",
        _ => return None,
    };
    let tail = match std::env::consts::OS {
        "windows" => "windows-x64-setup.exe".to_string(),
        "macos" => format!("macos-{arch}.dmg"),
        "linux" => format!("linux-{arch}.AppImage"),
        _ => return None,
    };
    Some(format!("ModrinthImporter-{tail}"))
}

fn expected_hash(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (h, n) = l.trim().split_once(char::is_whitespace)?;
        (n.trim().trim_start_matches('*') == asset).then(|| h.to_ascii_lowercase())
    })
}

/// Windows Defender quarantines unsigned developer builds now and then (a false positive). It
/// shows up as `ERROR_VIRUS_INFECTED` (225) or as the file vanishing right after the download.
fn blocked_by_antivirus(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(225) || e.to_string().to_lowercase().contains("virus")
}

fn blocked_message(asset: &str) -> Error {
    Error::Invalid(format!(
        "Windows Security (or another antivirus) blocked {asset}. That is a false positive on an \
         unsigned developer build. Open Windows Security > Virus & threat protection > Protection \
         history, choose the item and Actions > Allow on device, then try again. Nothing was \
         changed. See https://github.com/{REPO}/blob/master/docs/faq.md"
    ))
}

/// Download `asset` into `dir`, check it against `SHA256SUMS`, and return its path. A file that
/// fails the check is deleted; nothing installed is touched.
pub async fn download_verified(
    http: &reqwest::Client,
    rel: &Release,
    asset: &str,
    dir: &Path,
    rep: &Reporter,
) -> Result<PathBuf> {
    let a = rel.asset(asset)?;
    let sums = http
        .get(&rel.asset("SHA256SUMS")?.url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let want = expected_hash(&sums, asset)
        .ok_or_else(|| Error::Invalid(format!("{asset} is not listed in SHA256SUMS")))?;

    rep.stage(format!("Downloading {asset}"));
    let resp = http.get(&a.url).send().await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(0) as usize;
    tokio::fs::create_dir_all(dir).await?;
    let path = dir.join(asset);
    let mut file = tokio::fs::File::create(&path).await?;
    let mut hasher = Sha256::new();
    let mut done = 0usize;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        done += chunk.len();
        rep.progress(done, total.max(done));
    }
    file.flush().await?;
    drop(file);

    rep.stage("Verifying checksum");
    // Re-read from disk: this is what will be installed, and it is where antivirus acts.
    let on_disk = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(e) if blocked_by_antivirus(&e) || e.kind() == std::io::ErrorKind::NotFound => {
            return Err(blocked_message(asset));
        }
        Err(e) => return Err(e.into()),
    };
    if hex::encode(Sha256::digest(&on_disk)) != want {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(Error::Invalid(format!(
            "checksum mismatch for {asset}; refusing to install"
        )));
    }
    Ok(path)
}

fn io_err(what: &str, e: std::io::Error) -> Error {
    if blocked_by_antivirus(&e) {
        return Error::Invalid(format!("Windows Security blocked {what}: {e}"));
    }
    Error::Invalid(format!("{what}: {e}"))
}

/// Replace the CLI executable `exe` with the verified file `new`, removing the old one. A running
/// `.exe` can be renamed but not deleted, so on Windows the old copy is moved aside and removed
/// on a later run ([`cleanup_leftovers`]).
pub fn replace_cli(new: &Path, exe: &Path) -> Result<()> {
    let staged = exe.with_extension("new");
    std::fs::copy(new, &staged).map_err(|e| io_err("could not stage the new executable", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old);
    if cfg!(windows) {
        std::fs::rename(exe, &old).map_err(|e| {
            let _ = std::fs::remove_file(&staged);
            io_err(
                "could not move the old executable aside (try with sufficient permissions)",
                e,
            )
        })?;
    } else {
        std::fs::remove_file(exe).map_err(|e| {
            let _ = std::fs::remove_file(&staged);
            io_err(
                "could not remove the old executable (try with sufficient permissions)",
                e,
            )
        })?;
    }
    if let Err(e) = std::fs::rename(&staged, exe) {
        // Put the old one back rather than leave nothing installed.
        let _ = std::fs::rename(&old, exe);
        let _ = std::fs::remove_file(&staged);
        return Err(io_err("could not install the new executable", e));
    }
    if !cfg!(windows) {
        let _ = std::fs::remove_file(&old);
    }
    Ok(())
}

/// Remove files an earlier update could not delete while the program was running.
pub fn cleanup_leftovers(exe: &Path) {
    let _ = std::fs::remove_file(exe.with_extension("old"));
    let _ = std::fs::remove_file(exe.with_extension("new"));
}

/// Where the desktop app is installed. On Windows the app folder, on macOS the `.app` bundle, on
/// Linux the AppImage file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiInstall(pub PathBuf);

/// The desktop app's install, looked up at the places the installers use.
pub fn installed_gui() -> Option<GuiInstall> {
    let candidates: Vec<PathBuf> = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(|d| vec![PathBuf::from(d).join("Modrinth Importer")])
            .unwrap_or_default()
    } else if cfg!(target_os = "macos") {
        let mut v = vec![PathBuf::from("/Applications/Modrinth Importer.app")];
        if let Some(h) = std::env::var_os("HOME") {
            v.push(PathBuf::from(h).join("Applications/Modrinth Importer.app"));
        }
        v
    } else {
        let mut v = vec![];
        if let Some(h) = std::env::var_os("HOME") {
            v.push(PathBuf::from(h).join(".local/bin/ModrinthImporter.AppImage"));
        }
        v
    };
    candidates
        .into_iter()
        .find(|p| is_gui_install(p))
        .map(GuiInstall)
}

fn is_gui_install(p: &Path) -> bool {
    if cfg!(windows) {
        p.join("uninstall.exe").is_file()
    } else {
        p.exists()
    }
}

/// The install the running desktop app (`exe`) belongs to, if it was installed by the installer
/// (a development build or an unpacked archive can't update itself).
pub fn running_gui(exe: &Path) -> Option<GuiInstall> {
    if cfg!(windows) {
        let dir = exe.parent()?;
        is_gui_install(dir).then(|| GuiInstall(dir.to_path_buf()))
    } else if cfg!(target_os = "macos") {
        exe.ancestors()
            .find(|a| a.extension().is_some_and(|e| e == "app"))
            .map(|a| GuiInstall(a.to_path_buf()))
    } else {
        std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .map(GuiInstall)
    }
}

/// Uninstall the old desktop app and install the verified `installer` in its place, the way the
/// shell installers do. Settings and credentials are left alone.
///
/// With `relaunch` the new app is started afterwards. On Windows that is a detached helper that
/// waits for `wait_for_pid` (the running app) to exit first, so this returns immediately and the
/// caller must exit; elsewhere it completes before returning and the new app is started.
pub fn install_gui(
    installer: &Path,
    old: &GuiInstall,
    relaunch: bool,
    wait_for_pid: Option<u32>,
) -> Result<()> {
    #[cfg(windows)]
    {
        windows::install(installer, &old.0, relaunch, wait_for_pid)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = wait_for_pid;
        unix::install_macos(installer, &old.0, relaunch)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = wait_for_pid;
        unix::install_appimage(installer, &old.0, relaunch)
    }
}

/// PowerShell that does the Windows reinstall; written out so it can run after the app exits.
/// Values are passed as single-quoted literals with `'` doubled.
pub fn windows_script(
    installer: &Path,
    app_dir: &Path,
    relaunch: bool,
    wait_for_pid: Option<u32>,
) -> String {
    let q = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "''"));
    let mut s = String::from("$ErrorActionPreference = 'Continue'\n");
    if let Some(pid) = wait_for_pid {
        s += &format!("Wait-Process -Id {pid} -Timeout 60 -ErrorAction SilentlyContinue\n");
    }
    s += &format!(
        "$dir = {dir}\n$setup = {setup}\n\
         $u = Join-Path $dir 'uninstall.exe'\n\
         if (Test-Path -LiteralPath $u) {{ Start-Process $u -ArgumentList '/S', \"_?=$dir\" -Wait }}\n\
         if (Test-Path -LiteralPath $dir) {{ Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue }}\n\
         Start-Process $setup -ArgumentList '/S' -Wait\n\
         Remove-Item -LiteralPath $setup -Force -ErrorAction SilentlyContinue\n",
        dir = q(app_dir),
        setup = q(installer),
    );
    if relaunch {
        s += "$exe = Get-ChildItem -LiteralPath $dir -Filter '*.exe' -ErrorAction SilentlyContinue | Where-Object { $_.Name -notmatch 'uninstall' } | Select-Object -First 1\n\
              if ($exe) { Start-Process $exe.FullName }\n";
    }
    s += "Remove-Item -LiteralPath $MyInvocation.MyCommand.Path -Force -ErrorAction SilentlyContinue\n";
    s
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    pub fn install(
        installer: &Path,
        app_dir: &Path,
        relaunch: bool,
        wait_for_pid: Option<u32>,
    ) -> Result<()> {
        let script = windows_script(installer, app_dir, relaunch, wait_for_pid);
        let path = installer.with_file_name(format!("mrimport-update-{}.ps1", std::process::id()));
        // PowerShell 5 reads a BOM-less file as ANSI; a BOM keeps non-ASCII paths intact.
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(script.as_bytes());
        std::fs::write(&path, bytes).map_err(|e| io_err("could not write the update script", e))?;
        let mut cmd = std::process::Command::new("powershell.exe");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&path)
        .creation_flags(CREATE_NO_WINDOW);
        if wait_for_pid.is_some() {
            // The app is about to exit: let the helper outlive it.
            cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
                .spawn()
                .map_err(|e| io_err("could not start the update helper", e))?;
        } else {
            let st = cmd
                .status()
                .map_err(|e| io_err("could not run the update script", e))?;
            if !st.success() {
                return Err(Error::Invalid(format!("the update script failed ({st})")));
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::process::Command;

    fn run(cmd: &mut Command, what: &str) -> Result<()> {
        let st = cmd.status().map_err(|e| io_err(what, e))?;
        if st.success() {
            Ok(())
        } else {
            Err(Error::Invalid(format!("{what} failed ({st})")))
        }
    }

    #[cfg(target_os = "macos")]
    pub fn install_macos(dmg: &Path, app: &Path, relaunch: bool) -> Result<()> {
        let mnt = dmg.with_extension("mnt");
        std::fs::create_dir_all(&mnt)?;
        run(
            Command::new("hdiutil")
                .args(["attach", "-nobrowse", "-quiet", "-mountpoint"])
                .arg(&mnt)
                .arg(dmg),
            "mounting the disk image",
        )?;
        let copy = (|| -> Result<()> {
            let src = mnt.join("Modrinth Importer.app");
            if !src.exists() {
                return Err(Error::Invalid(
                    "the disk image has no Modrinth Importer.app".into(),
                ));
            }
            // Old app out, new app in (a running app's files stay valid until it exits).
            std::fs::remove_dir_all(app).map_err(|e| io_err("could not remove the old app", e))?;
            run(
                Command::new("cp")
                    .arg("-R")
                    .arg(&src)
                    .arg(app.parent().unwrap_or(Path::new("/"))),
                "copying the new app",
            )
        })();
        let _ = Command::new("hdiutil")
            .args(["detach", "-quiet"])
            .arg(&mnt)
            .status();
        let _ = std::fs::remove_dir_all(&mnt);
        copy?;
        let _ = Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(app)
            .status();
        // Developer build: seal the bundle with a local ad-hoc signature, as install.sh does.
        let _ = Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(app)
            .status();
        let _ = std::fs::remove_file(dmg);
        if relaunch {
            let _ = Command::new("open").arg("-n").arg(app).spawn();
        }
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn install_appimage(new: &Path, old: &Path, relaunch: bool) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let staged = old.with_extension("new");
        std::fs::copy(new, &staged).map_err(|e| io_err("could not stage the new AppImage", e))?;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
        std::fs::remove_file(old).map_err(|e| io_err("could not remove the old AppImage", e))?;
        std::fs::rename(&staged, old)
            .map_err(|e| io_err("could not install the new AppImage", e))?;
        let _ = std::fs::remove_file(new);
        if relaunch {
            let _ = Command::new(old).spawn();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

    #[test]
    fn only_a_strictly_newer_version_is_an_update() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.1.10", "0.1.9"), "numeric, not lexical");
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"), "never a downgrade");
    }

    #[test]
    fn checksum_lines_are_matched_by_exact_name() {
        let sums = "AB12  mrimport-x.exe\ncd34 *other.exe\n";
        assert_eq!(expected_hash(sums, "mrimport-x.exe").unwrap(), "ab12");
        assert_eq!(expected_hash(sums, "other.exe").unwrap(), "cd34");
        assert!(expected_hash(sums, "mrimport").is_none());
    }

    #[test]
    fn asset_names_match_the_release_workflow() {
        assert_eq!(
            cli_asset("x86_64-pc-windows-msvc"),
            "mrimport-x86_64-pc-windows-msvc.exe"
        );
        assert_eq!(
            cli_asset("aarch64-apple-darwin"),
            "mrimport-aarch64-apple-darwin"
        );
    }

    async fn release(server: &MockServer, body: &[u8], sums_for: &[u8]) -> Release {
        let sums = format!("{}  tool.bin\n", hex::encode(Sha256::digest(sums_for)));
        Mock::given(path("/tool.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(server)
            .await;
        Mock::given(path("/SHA256SUMS"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sums))
            .mount(server)
            .await;
        Release {
            version: "9.9.9".into(),
            assets: vec![
                Asset {
                    name: "tool.bin".into(),
                    url: format!("{}/tool.bin", server.uri()),
                },
                Asset {
                    name: "SHA256SUMS".into(),
                    url: format!("{}/SHA256SUMS", server.uri()),
                },
            ],
        }
    }

    #[tokio::test]
    async fn a_download_matching_its_checksum_is_kept() {
        let server = MockServer::start().await;
        let rel = release(&server, b"payload", b"payload").await;
        let dir = tempfile::tempdir().unwrap();
        let p = download_verified(
            &client().unwrap(),
            &rel,
            "tool.bin",
            dir.path(),
            &Reporter::silent(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(p).unwrap(), b"payload");
    }

    #[tokio::test]
    async fn a_tampered_download_is_refused_and_deleted() {
        let server = MockServer::start().await;
        let rel = release(&server, b"evil", b"payload").await;
        let dir = tempfile::tempdir().unwrap();
        let err = download_verified(
            &client().unwrap(),
            &rel,
            "tool.bin",
            dir.path(),
            &Reporter::silent(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"), "{err}");
        assert!(
            !dir.path().join("tool.bin").exists(),
            "bad file must not linger"
        );
    }

    #[tokio::test]
    async fn the_newest_release_is_read_from_the_api() {
        let server = MockServer::start().await;
        Mock::given(path("/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"tag_name": "v0.3.1", "assets": [{"name": "a", "browser_download_url": "http://x/a"}]}
            ])))
            .mount(&server)
            .await;
        let r = latest_from(&client().unwrap(), &format!("{}/releases", server.uri()))
            .await
            .unwrap();
        assert_eq!(r.version, "0.3.1");
        assert_eq!(r.assets[0].name, "a");
    }

    #[test]
    fn replacing_the_cli_swaps_the_binary_and_leaves_no_staging_files() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("mrimport.exe");
        let new = dir.path().join("download.bin");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        replace_cli(&new, &exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert!(!exe.with_extension("new").exists());
        // Windows keeps the old copy aside (it may be running); elsewhere it is deleted outright.
        assert_eq!(exe.with_extension("old").exists(), cfg!(windows));
        cleanup_leftovers(&exe);
        assert!(!exe.with_extension("old").exists());
    }

    #[test]
    fn the_windows_script_uninstalls_then_installs_and_quotes_paths() {
        let s = windows_script(
            Path::new(r"C:\Temp\it's\setup.exe"),
            Path::new(r"C:\Users\a\AppData\Local\Modrinth Importer"),
            true,
            Some(4242),
        );
        let (w, u, i, r) = (
            s.find("Wait-Process -Id 4242").unwrap(),
            s.find("uninstall.exe").unwrap(),
            s.find("-ArgumentList '/S' -Wait").unwrap(),
            s.find("Start-Process $exe.FullName").unwrap(),
        );
        assert!(
            w < u && u < i && i < r,
            "wait, uninstall, install, relaunch:\n{s}"
        );
        assert!(
            s.contains(r"'C:\Temp\it''s\setup.exe'"),
            "single quotes are doubled:\n{s}"
        );
        assert!(
            !windows_script(Path::new("s"), Path::new("d"), false, None).contains("Wait-Process")
        );
    }

    /// The script is only ever run after the app has exited, so a syntax error would fail silently.
    #[cfg(windows)]
    #[test]
    fn the_windows_script_is_valid_powershell() {
        let s = windows_script(
            Path::new(r"C: b\it's.exe"),
            Path::new(r"C:\Modrinth Importer"),
            true,
            Some(1),
        );
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("s.ps1");
        std::fs::write(&f, s).unwrap();
        let out = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command"])
            .arg(format!(
                "$e=$null; [void][System.Management.Automation.Language.Parser]::ParseFile('{}',[ref]$null,[ref]$e); if($e){{$e|Out-String; exit 1}}",
                f.display()
            ))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}
