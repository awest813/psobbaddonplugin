use chrono::Utc;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use zip::ZipArchive;

const RELEASE_OWNER: &str = "awest813";
const RELEASE_REPO: &str = "psobbaddonplugin";
const CONFIG_FILE: &str = "config.json";
const LOG_FILE: &str = "launcher.log";
const INSTALL_MANIFEST_FILE: &str = "bbmod-launcher-manifest.json";
const BACKUP_MANIFEST_FILE: &str = "backup-manifest.json";
const ALLOWED_ROOT_FILES: &[&str] = &["dinput8.dll", "dinput8.pdb", "README.md", "CHANGELOG.md"];

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct AppConfig {
    install_path: Option<String>,
    last_backup_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CheckItem {
    name: String,
    ok: bool,
    message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PreflightResult {
    ok: bool,
    checks: Vec<CheckItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ReleaseInfo {
    tag_name: String,
    name: String,
    published_at: String,
    bbmod_zip_url: String,
    bbmod_zip_size: u64,
    sha256_asset_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LauncherStatus {
    install_path: Option<String>,
    install_path_exists: bool,
    game_executable_exists: bool,
    addon_installed: bool,
    installed_version: Option<String>,
    latest_release: Option<ReleaseInfo>,
    log_path: String,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct InstallRequest {
    zip_path: String,
    install_path: Option<String>,
    version: Option<String>,
    dry_run: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct InstallLatestRequest {
    install_path: Option<String>,
    dry_run: Option<bool>,
    expected_sha256: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct InstallResult {
    installed: bool,
    backup_path: Option<String>,
    version: Option<String>,
    installed_files: usize,
    source_zip: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct InstallManifest {
    installed_version: Option<String>,
    installed_at: String,
    source_zip: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BackupEntry {
    relative_path: String,
    existed: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BackupManifest {
    created_at: String,
    entries: Vec<BackupEntry>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: String,
    published_at: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

fn app_data_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_root(app)?.join("config");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn log_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_root(app)?.join("logs");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn backup_root(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_root(app)?.join("backups");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn downloads_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_root(app)?.join("downloads");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn log_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(log_dir(app)?.join(LOG_FILE))
}

fn log_event(app: &AppHandle, level: &str, action: &str, message: &str) {
    let path = match log_path(app) {
        Ok(p) => p,
        Err(_) => return,
    };

    let event = serde_json::json!({
        "timestamp": Utc::now().to_rfc3339(),
        "level": level,
        "action": action,
        "message": message,
    });

    let line = match serde_json::to_string(&event) {
        Ok(l) => l,
        Err(_) => return,
    };

    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

fn load_config(app: &AppHandle) -> Result<AppConfig, String> {
    let path = config_dir(app)?.join(CONFIG_FILE);
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str::<AppConfig>(&content).map_err(|e| e.to_string())
}

fn save_config(app: &AppHandle, config: &AppConfig) -> Result<(), String> {
    let path = config_dir(app)?.join(CONFIG_FILE);
    let serialized = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, serialized).map_err(|e| e.to_string())
}

fn now_unix_secs() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| e.to_string())
}

fn resolve_install_path(app: &AppHandle, candidate: Option<String>) -> Result<PathBuf, String> {
    if let Some(path) = candidate {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("Install path cannot be empty".to_string());
        }
        let path = PathBuf::from(trimmed);
        if !path.is_absolute() {
            return Err("Install path must be absolute".to_string());
        }
        return Ok(path);
    }

    let config = load_config(app)?;
    if let Some(path) = config.install_path {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("Saved install path is empty".to_string());
        }
        let path = PathBuf::from(trimmed);
        if !path.is_absolute() {
            return Err("Install path must be absolute".to_string());
        }
        return Ok(path);
    }

    Err("No install path configured".to_string())
}

fn is_allowed_archive_path(path: &Path) -> bool {
    let mut components = path.components();
    let first = match components.next() {
        Some(Component::Normal(c)) => c.to_string_lossy().to_string(),
        _ => return false,
    };

    if first.eq_ignore_ascii_case("addons") {
        return true;
    }

    if components.next().is_some() {
        return false;
    }

    ALLOWED_ROOT_FILES
        .iter()
        .any(|f| f.eq_ignore_ascii_case(&first))
}

fn inspect_zip(zip_path: &Path) -> Result<(usize, bool, bool), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| e.to_string())?;

    let mut allowed_files = 0usize;
    let mut has_dinput = false;
    let mut has_addons = false;

    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.name().ends_with('/') {
            continue;
        }

        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };

        if !is_allowed_archive_path(enclosed) {
            continue;
        }

        allowed_files += 1;

        let top = enclosed
            .components()
            .next()
            .and_then(|c| match c {
                Component::Normal(n) => Some(n.to_string_lossy().to_string()),
                _ => None,
            })
            .unwrap_or_default();

        if top.eq_ignore_ascii_case("addons") {
            has_addons = true;
        }
        if top.eq_ignore_ascii_case("dinput8.dll") {
            has_dinput = true;
        }
    }

    Ok((allowed_files, has_dinput, has_addons))
}

fn copy_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(src, dst).map_err(|e| e.to_string())?;
        return Ok(());
    }

    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        copy_recursive(&src_path, &dst_path)?;
    }
    Ok(())
}

fn create_backup(install_path: &Path, app: &AppHandle) -> Result<PathBuf, String> {
    let stamp = now_unix_secs()?;
    let backup_dir = backup_root(app)?.join(format!("backup-{stamp}"));
    fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;

    let targets = [
        "addons",
        "dinput8.dll",
        "dinput8.pdb",
        "README.md",
        "CHANGELOG.md",
        INSTALL_MANIFEST_FILE,
    ];

    let mut entries = Vec::with_capacity(targets.len());

    for target in targets {
        let source = install_path.join(target);
        let existed = source.exists();

        entries.push(BackupEntry {
            relative_path: target.to_string(),
            existed,
        });

        if existed {
            let dest = backup_dir.join(target);
            copy_recursive(&source, &dest)?;
        }
    }

    let manifest = BackupManifest {
        created_at: Utc::now().to_rfc3339(),
        entries,
    };
    let manifest_path = backup_dir.join(BACKUP_MANIFEST_FILE);
    let content = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(manifest_path, content).map_err(|e| e.to_string())?;

    Ok(backup_dir)
}

fn rollback_backup(install_path: &Path, backup_path: &Path) -> Result<(), String> {
    let manifest_path = backup_path.join(BACKUP_MANIFEST_FILE);
    let raw = fs::read_to_string(manifest_path).map_err(|e| e.to_string())?;
    let manifest: BackupManifest = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    for entry in manifest.entries {
        let target = install_path.join(&entry.relative_path);
        if target.exists() {
            if target.is_dir() {
                fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
            } else {
                fs::remove_file(&target).map_err(|e| e.to_string())?;
            }
        }

        if entry.existed {
            let source = backup_path.join(&entry.relative_path);
            if source.exists() {
                copy_recursive(&source, &target)?;
            }
        }
    }

    Ok(())
}

fn extract_zip_allowed(zip_path: &Path, install_path: &Path) -> Result<usize, String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| e.to_string())?;

    let mut installed_files = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;

        if entry.name().ends_with('/') {
            continue;
        }

        let Some(enclosed) = entry.enclosed_name().map(|p| p.to_owned()) else {
            continue;
        };

        if !is_allowed_archive_path(&enclosed) {
            continue;
        }

        let out_path = install_path.join(&enclosed);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let mut outfile = fs::File::create(&out_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut outfile).map_err(|e| e.to_string())?;
        installed_files += 1;
    }

    Ok(installed_files)
}

fn read_install_manifest(install_path: &Path) -> Option<InstallManifest> {
    let path = install_path.join(INSTALL_MANIFEST_FILE);
    if !path.exists() {
        return None;
    }

    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str::<InstallManifest>(&content).ok()
}

fn verify_checksum_internal(path: &Path, expected_sha256: &str) -> Result<bool, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let actual = format!("{:x}", hasher.finalize());
    Ok(actual.eq_ignore_ascii_case(expected_sha256.trim()))
}

fn fetch_latest_release_internal() -> Result<ReleaseInfo, String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        RELEASE_OWNER, RELEASE_REPO
    );

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let release = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "psobbaddonplugin-launcher")
        .send()
        .and_then(|resp| resp.error_for_status())
        .map_err(|e| e.to_string())?
        .json::<GithubRelease>()
        .map_err(|e| e.to_string())?;

    let zip_asset = release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("bbmod.zip"))
        .ok_or_else(|| "Latest release is missing bbmod.zip".to_string())?;

    let sha_asset = release.assets.iter().find(|a| {
        let lower = a.name.to_lowercase();
        lower.contains("sha256") || lower.ends_with(".sha256")
    });

    Ok(ReleaseInfo {
        tag_name: release.tag_name,
        name: release.name,
        published_at: release.published_at,
        bbmod_zip_url: zip_asset.browser_download_url.clone(),
        bbmod_zip_size: zip_asset.size,
        sha256_asset_url: sha_asset.map(|a| a.browser_download_url.clone()),
    })
}

fn download_to_path(url: &str, destination: &Path) -> Result<(), String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let bytes = client
        .get(url)
        .header("User-Agent", "psobbaddonplugin-launcher")
        .send()
        .and_then(|resp| resp.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .map_err(|e| e.to_string())?;

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    fs::write(destination, &bytes).map_err(|e| e.to_string())
}

fn install_from_zip_internal(
    app: &AppHandle,
    zip_path: &Path,
    install_path: &Path,
    version: Option<String>,
    dry_run: bool,
) -> Result<InstallResult, String> {
    if !install_path.exists() {
        return Err(format!(
            "Install path does not exist: {}",
            install_path.display()
        ));
    }

    let (allowed_files, has_dinput, has_addons) = inspect_zip(zip_path)?;
    if allowed_files == 0 {
        return Err("Archive has no allowed addon files to install".to_string());
    }
    if !has_dinput {
        return Err("Archive does not contain dinput8.dll".to_string());
    }
    if !has_addons {
        return Err("Archive does not contain addons directory contents".to_string());
    }

    if dry_run {
        return Ok(InstallResult {
            installed: false,
            backup_path: None,
            version,
            installed_files: allowed_files,
            source_zip: zip_path.display().to_string(),
        });
    }

    let backup_path = create_backup(install_path, app)?;
    let apply_result = (|| -> Result<InstallResult, String> {
        let installed_files = extract_zip_allowed(zip_path, install_path)?;

        let manifest = InstallManifest {
            installed_version: version.clone(),
            installed_at: Utc::now().to_rfc3339(),
            source_zip: Some(zip_path.display().to_string()),
        };
        let manifest_content =
            serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
        fs::write(install_path.join(INSTALL_MANIFEST_FILE), manifest_content)
            .map_err(|e| e.to_string())?;

        if !install_path.join("dinput8.dll").exists() {
            return Err("Post-install verification failed: dinput8.dll missing".to_string());
        }
        if !install_path.join("addons").exists() {
            return Err("Post-install verification failed: addons directory missing".to_string());
        }

        Ok(InstallResult {
            installed: true,
            backup_path: Some(backup_path.display().to_string()),
            version,
            installed_files,
            source_zip: zip_path.display().to_string(),
        })
    })();

    match apply_result {
        Ok(result) => {
            let mut config = load_config(app)?;
            config.last_backup_path = Some(backup_path.display().to_string());
            config.install_path = Some(install_path.display().to_string());
            save_config(app, &config)?;
            Ok(result)
        }
        Err(err) => {
            let rollback_err = rollback_backup(install_path, &backup_path).err();
            if let Some(re) = rollback_err {
                Err(format!("{err}. Rollback failed: {re}"))
            } else {
                Err(format!("{err}. Rollback completed."))
            }
        }
    }
}

#[tauri::command]
fn get_status(app: AppHandle) -> Result<LauncherStatus, String> {
    let config = load_config(&app)?;
    let install_path = config.install_path.as_ref().map(PathBuf::from);

    let install_path_exists = install_path.as_ref().is_some_and(|p| p.exists());

    let game_executable_exists = install_path.as_ref().is_some_and(|p| {
        p.join("online.exe").exists() || p.join("pso.exe").exists() || p.join("psobb.exe").exists()
    });

    let addon_installed = install_path
        .as_ref()
        .is_some_and(|p| p.join("dinput8.dll").exists() && p.join("addons").exists());

    let installed_version = install_path
        .as_ref()
        .and_then(|p| read_install_manifest(p))
        .and_then(|m| m.installed_version);

    let latest_release = fetch_latest_release_internal().ok();

    let mut warnings = Vec::new();
    if !install_path_exists {
        warnings.push("Install path is not configured or does not exist".to_string());
    }
    if install_path_exists && !game_executable_exists {
        warnings.push(
            "No expected game executable (online.exe/pso.exe/psobb.exe) found in selected folder"
                .to_string(),
        );
    }
    if !addon_installed {
        warnings.push("Addon files are not fully installed in selected folder".to_string());
    }

    let log_path = log_path(&app)?.display().to_string();

    Ok(LauncherStatus {
        install_path: config.install_path,
        install_path_exists,
        game_executable_exists,
        addon_installed,
        installed_version,
        latest_release,
        log_path,
        warnings,
    })
}

#[tauri::command]
fn set_install_path(app: AppHandle, install_path: String) -> Result<(), String> {
    if install_path.trim().is_empty() {
        return Err("Install path cannot be empty".to_string());
    }
    if !Path::new(install_path.trim()).is_absolute() {
        return Err("Install path must be absolute".to_string());
    }

    let mut config = load_config(&app)?;
    config.install_path = Some(install_path.clone());
    save_config(&app, &config)?;
    log_event(&app, "info", "set_install_path", &install_path);
    Ok(())
}

#[tauri::command]
fn preflight_install(
    app: AppHandle,
    zip_path: String,
    install_path: Option<String>,
) -> Result<PreflightResult, String> {
    let zip = PathBuf::from(zip_path.clone());
    if !zip.is_absolute() {
        return Err("Zip path must be absolute".to_string());
    }
    let target = resolve_install_path(&app, install_path)?;

    let mut checks = Vec::new();

    checks.push(CheckItem {
        name: "zip_exists".to_string(),
        ok: zip.exists(),
        message: if zip.exists() {
            format!("Found archive: {}", zip.display())
        } else {
            format!("Archive not found: {}", zip.display())
        },
    });

    checks.push(CheckItem {
        name: "install_path_exists".to_string(),
        ok: target.exists(),
        message: if target.exists() {
            format!("Install path exists: {}", target.display())
        } else {
            format!("Install path does not exist: {}", target.display())
        },
    });

    if zip.exists() {
        match inspect_zip(&zip) {
            Ok((allowed_files, has_dinput, has_addons)) => {
                checks.push(CheckItem {
                    name: "allowed_files".to_string(),
                    ok: allowed_files > 0,
                    message: format!("Allowed files in archive: {allowed_files}"),
                });
                checks.push(CheckItem {
                    name: "has_dinput".to_string(),
                    ok: has_dinput,
                    message: if has_dinput {
                        "Archive includes dinput8.dll".to_string()
                    } else {
                        "Archive is missing dinput8.dll".to_string()
                    },
                });
                checks.push(CheckItem {
                    name: "has_addons".to_string(),
                    ok: has_addons,
                    message: if has_addons {
                        "Archive includes addons files".to_string()
                    } else {
                        "Archive is missing addons files".to_string()
                    },
                });
            }
            Err(err) => checks.push(CheckItem {
                name: "zip_readable".to_string(),
                ok: false,
                message: format!("Unable to inspect zip: {err}"),
            }),
        }
    }

    let ok = checks.iter().all(|c| c.ok);
    log_event(
        &app,
        "info",
        "preflight_install",
        &format!("zip={zip_path} ok={ok}"),
    );
    Ok(PreflightResult { ok, checks })
}

#[tauri::command]
fn install_from_zip(app: AppHandle, request: InstallRequest) -> Result<InstallResult, String> {
    let target = resolve_install_path(&app, request.install_path)?;
    let zip_path = PathBuf::from(request.zip_path.clone());
    if !zip_path.is_absolute() {
        return Err("Zip path must be absolute".to_string());
    }
    let dry_run = request.dry_run.unwrap_or(false);

    log_event(
        &app,
        "info",
        "install_from_zip_start",
        &format!(
            "zip={} target={} dry_run={dry_run}",
            zip_path.display(),
            target.display()
        ),
    );

    let result =
        install_from_zip_internal(&app, &zip_path, &target, request.version.clone(), dry_run)?;

    log_event(
        &app,
        "info",
        "install_from_zip_success",
        &format!(
            "installed={} files={} backup={}",
            result.installed,
            result.installed_files,
            result.backup_path.clone().unwrap_or_default()
        ),
    );

    Ok(result)
}

#[tauri::command]
fn verify_zip_checksum(zip_path: String, expected_sha256: String) -> Result<bool, String> {
    verify_checksum_internal(Path::new(&zip_path), &expected_sha256)
}

#[tauri::command]
fn fetch_latest_release(app: AppHandle) -> Result<ReleaseInfo, String> {
    let release = fetch_latest_release_internal()?;
    log_event(
        &app,
        "info",
        "fetch_latest_release",
        &format!("latest_tag={}", release.tag_name),
    );
    Ok(release)
}

#[tauri::command]
fn install_latest_release(
    app: AppHandle,
    request: InstallLatestRequest,
) -> Result<InstallResult, String> {
    let release = fetch_latest_release_internal()?;
    let zip_destination = downloads_dir(&app)?.join(format!("bbmod-{}.zip", release.tag_name));
    download_to_path(&release.bbmod_zip_url, &zip_destination)?;

    let mut checksum_to_use = request.expected_sha256.clone();
    if checksum_to_use.is_none() {
        if let Some(sha_url) = release.sha256_asset_url.as_deref() {
            let downloaded = reqwest::blocking::get(sha_url)
                .and_then(|resp| resp.error_for_status())
                .and_then(|resp| resp.text())
                .map_err(|e| e.to_string())?;
            let first = downloaded
                .split_whitespace()
                .find(|part| part.len() == 64 && part.chars().all(|c| c.is_ascii_hexdigit()))
                .map(|s| s.to_string());
            checksum_to_use = first;
        }
    }

    if let Some(expected) = checksum_to_use {
        let ok = verify_checksum_internal(&zip_destination, &expected)?;
        if !ok {
            return Err("Downloaded archive checksum verification failed".to_string());
        }
    }

    let target = resolve_install_path(&app, request.install_path)?;
    let result = install_from_zip_internal(
        &app,
        &zip_destination,
        &target,
        Some(release.tag_name.clone()),
        request.dry_run.unwrap_or(false),
    )?;

    log_event(
        &app,
        "info",
        "install_latest_release",
        &format!(
            "installed_version={} source={}",
            release.tag_name,
            zip_destination.display()
        ),
    );

    Ok(result)
}

#[tauri::command]
fn rollback_last_install(app: AppHandle) -> Result<String, String> {
    let config = load_config(&app)?;
    let install_path = resolve_install_path(&app, None)?;
    let backup = config
        .last_backup_path
        .ok_or_else(|| "No previous backup is available".to_string())?;

    let backup_path = PathBuf::from(&backup);
    if !backup_path.exists() {
        return Err(format!("Backup path does not exist: {backup}"));
    }

    rollback_backup(&install_path, &backup_path)?;
    log_event(
        &app,
        "warn",
        "rollback_last_install",
        &format!("restored_from={backup}"),
    );
    Ok(format!("Rollback completed from {backup}"))
}

#[tauri::command]
fn launch_game(app: AppHandle) -> Result<String, String> {
    let install = resolve_install_path(&app, None)?;
    let candidates = ["online.exe", "pso.exe", "psobb.exe"];

    let executable = candidates
        .iter()
        .map(|name| install.join(name))
        .find(|path| path.exists())
        .ok_or_else(|| {
            format!(
                "No game executable found in {} (looked for online.exe, pso.exe, psobb.exe)",
                install.display()
            )
        })?;

    Command::new(&executable)
        .current_dir(&install)
        .spawn()
        .map_err(|e| e.to_string())?;

    log_event(
        &app,
        "info",
        "launch_game",
        &format!("launched={}", executable.display()),
    );

    Ok(format!("Launched {}", executable.display()))
}

#[tauri::command]
fn read_logs(app: AppHandle, tail_lines: Option<usize>) -> Result<String, String> {
    let path = log_path(&app)?;
    if !path.exists() {
        return Ok(String::new());
    }

    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = content.lines().collect();
    let tail = tail_lines.unwrap_or(200);

    if lines.len() <= tail {
        return Ok(content);
    }

    Ok(lines[lines.len() - tail..].join("\n"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            fetch_latest_release,
            get_status,
            install_from_zip,
            install_latest_release,
            launch_game,
            preflight_install,
            read_logs,
            rollback_last_install,
            set_install_path,
            verify_zip_checksum
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
