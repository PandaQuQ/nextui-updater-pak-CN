use crate::{
    app_state::{AppStateManager, Progress},
    config::VersionMode,
    github::{Release, ReleaseAndTag},
    Result, SDCARD_ROOT,
};
use bytes::Bytes;
use fetching::{download, fetch_releases, fetch_tags};
use regex::Regex;

use std::{
    cmp::Ordering,
    collections::HashSet,
    fs::File,
    io::{Cursor, Read, Write},
    path::PathBuf,
    process::exit,
    sync::LazyLock,
    thread,
};

mod fetching;

fn extract_zip<T: Fn(&str) -> bool>(
    bytes: Bytes,
    filter: T,
    progress_cb: impl Fn(f32),
) -> Result<()> {
    pub fn file_write_all_bytes(path: &PathBuf, bytes: &[u8]) -> Result<usize> {
        let mut file = File::create(path)?;
        file.set_len(0)?;
        Ok(file.write(bytes)?)
    }

    // Extract the update package
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let target_directory = PathBuf::from(&*SDCARD_ROOT);
    let archive_len = archive.len();

    for file_number in 0..archive_len {
        let mut next = archive.by_index(file_number)?;

        let sanitized_name = next.mangled_name();

        if !filter(sanitized_name.as_os_str().to_string_lossy().as_ref()) {
            println!("Skipping file: {}", sanitized_name.display());
            continue;
        }

        if next.is_dir() {
            let extracted_folder_path = target_directory.join(sanitized_name);
            std::fs::create_dir_all(&extracted_folder_path)?;
            println!("Created directory: {}", extracted_folder_path.display());
        } else if next.is_file() {
            let mut buffer: Vec<u8> = Vec::new();
            let _bytes_read = next.read_to_end(&mut buffer)?;
            let extracted_file_path = target_directory.join(sanitized_name);
            file_write_all_bytes(&extracted_file_path, buffer.as_ref())?;
            println!("Extracted file: {}", extracted_file_path.display());
        }

        progress_cb((file_number + 1) as f32 / archive_len as f32);
    }

    Ok(())
}

/// Repository the (Chinese) updater updates itself from.
const UPDATER_REPO: &str = "PandaQuQ/nextui-updater-pak-CN";
/// Release asset that contains `Tools/tg5040/Updater.pak/...` (extracted at the SD root).
/// (`Updater.pak.zip` holds the bare pak folder contents and must not be used here.)
const UPDATER_ASSET: &str = "nextui-updater-pak.zip";

/// Version of this build: the release tag injected by the release workflow
/// (`NEXTUI_UPDATER_RELEASE_TAG`, e.g. `v0.5.0-fix3`), otherwise the crate version.
pub fn installed_updater_version() -> &'static str {
    match option_env!("NEXTUI_UPDATER_RELEASE_TAG") {
        Some(tag) if !tag.is_empty() => tag,
        _ => env!("CARGO_PKG_VERSION"),
    }
}

/// Sort key for updater versions like `v0.5.0`, `v0.5.0-fix2`, `v0.6.0-beta1`.
/// `-fixN` is newer than the plain version, `-alpha/-beta/-rcN` are older.
/// Returns `None` if the tag has no `X.Y.Z` core.
fn updater_version_key(tag: &str) -> Option<(semver::Version, u8, u64)> {
    let tag = tag.trim().trim_start_matches(['v', 'V']);
    let (core, suffix) = tag.split_once('-').unwrap_or((tag, ""));
    let core = semver::Version::parse(core).ok()?;
    let mut rank = 1; // plain release
    let mut num = 0;
    for part in suffix.split('-').filter(|p| !p.is_empty()) {
        let lower = part.to_ascii_lowercase();
        let digits_at = lower
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(lower.len());
        let (word, n) = lower.split_at(digits_at);
        let n = n.parse::<u64>().unwrap_or(0);
        match word {
            "fix" | "hotfix" | "patch" => {
                rank = 2;
                num = n.max(1);
            }
            "alpha" | "beta" | "rc" | "pre" => {
                rank = 0;
                num = n;
            }
            _ => {} // e.g. "CN"
        }
    }
    Some((core, rank, num))
}

/// Is `candidate` strictly newer than `installed`? Unparseable tags never are.
fn is_newer_updater(candidate: &str, installed: &str) -> bool {
    if candidate.trim() == installed.trim() {
        return false;
    }
    match (
        updater_version_key(candidate),
        updater_version_key(installed),
    ) {
        (Some(c), Some(i)) => c.cmp(&i) == Ordering::Greater,
        _ => false,
    }
}

/// Newest updater release allowed by the version mode (by version, not date).
fn pick_updater_release(releases: Vec<Release>, mode: VersionMode) -> Option<Release> {
    filter_releases(releases, mode)
        .into_iter()
        .filter(|r| updater_version_key(&r.tag_name).is_some())
        .max_by(|a, b| updater_version_key(&a.tag_name).cmp(&updater_version_key(&b.tag_name)))
}

pub fn self_update(app_state: &AppStateManager) -> Result<()> {
    // Fetch latest release information
    app_state.start_operation("正在检查更新器更新...");

    println!("正在检查更新器更新...");

    let installed = installed_updater_version();
    let Some(release) =
        pick_updater_release(fetch_releases(UPDATER_REPO)?, app_state.version_mode())
    else {
        println!("No updater release found in {UPDATER_REPO}");
        return Ok(());
    };

    println!(
        "Latest updater release: {} (installed: {installed})",
        release.tag_name
    );

    if is_newer_updater(&release.tag_name, installed) {
        println!(
            "New version available: {} (current: {installed})",
            release.tag_name
        );
    } else {
        println!("No updates available");
        return Ok(());
    }

    let Some(asset) = release.assets.iter().find(|a| a.name == UPDATER_ASSET) else {
        println!(
            "Updater release {} has no {UPDATER_ASSET}",
            release.tag_name
        );
        return Ok(());
    };
    app_state.set_current_operation(Some("正在下载更新器...".to_string()));

    let bytes = download(&asset.url, |pr| {
        app_state.update_progress(pr);
    })?;

    app_state
        .set_current_operation(format!("正在解压 NextUI 更新器 {}...", release.tag_name).into());
    app_state.set_progress(Some(Progress::Indeterminate));

    // Move the current binary to a backup location
    let current_binary = std::env::current_exe()?;
    std::fs::rename(&current_binary, current_binary.with_extension("bak"))?;

    // Extract the update package
    let result = extract_zip(
        bytes,
        |_| true,
        |pr| {
            app_state.update_progress(pr);
        },
    );

    println!("Extraction complete!");
    app_state.set_progress(Some(Progress::Indeterminate));

    if result.is_err() {
        // Move the backup back
        std::fs::rename(current_binary.with_extension("bak"), current_binary)?;

        return Err("解压更新包失败".into());
    }

    app_state.set_current_operation(Some("自更新成功！正在重启更新器...".to_string()));

    // Give the user a moment to see the completion message
    thread::sleep(std::time::Duration::from_secs(1));

    // "5" is the exit code for "restart required"
    exit(5);
}

static EMU_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\((?<emu>\w+)\)").expect("Failed to compile regex"));

/// Emulator tags (`GB` for `Game Boy (GB)`) of the Roms folders on the SD card.
fn existing_emu_tags(roms_dir: &std::path::Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(roms_dir) else {
        return HashSet::new();
    };
    entries
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            EMU_TAG_RE
                .captures(&e.file_name().to_string_lossy())
                .and_then(|c| c.name("emu").map(|m| m.as_str().to_string()))
        })
        .collect()
}

/// Full update filter: skip anything inside a Roms folder whose emulator tag
/// was already present before the update started.
fn full_update_filter(file: &str, existing_emu_tags: &HashSet<String>) -> bool {
    let Some(rest) = file.strip_prefix("Roms/") else {
        return true;
    };
    // Only the top-level folder name under Roms/ decides
    let folder = rest.split('/').next().unwrap_or_default();
    match EMU_TAG_RE
        .captures(folder)
        .and_then(|c| c.name("emu").map(|m| m.as_str().to_string()))
    {
        Some(emu) => !existing_emu_tags.contains(&emu),
        None => true,
    }
}

/// Drop drafts, drop prereleases unless the user opted in, newest first.
pub fn filter_releases(releases: Vec<Release>, mode: VersionMode) -> Vec<Release> {
    let mut releases: Vec<Release> = releases
        .into_iter()
        .filter(|r| !r.draft && (mode == VersionMode::Prerelease || !r.prerelease))
        .collect();
    // GitHub does not strictly order /releases by publish time; keep the
    // newest published release first so it becomes the default choice.
    // (stable sort: releases without a date keep their API order at the end)
    releases.sort_by(|a, b| match (&a.published_at, &b.published_at) {
        (Some(x), Some(y)) => y.cmp(x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    releases
}

pub fn do_nextui_release_check(app_state: &AppStateManager) {
    // Forget results of a previous check (e.g. made in another version mode)
    app_state.set_nextui_release(None);
    app_state.set_nextui_tag(None);
    app_state.set_nextui_releases_and_tags(None);
    app_state.set_nextui_releases_and_tags_index(None);

    // Fetch latest release information
    app_state.start_operation("正在检查 NextUI 更新...");
    let repo = "PandaQuQ/NextUI-CN";

    // Fetch latest releases information
    app_state.start_operation("正在获取 NextUI Release 列表...");
    let latest_releases = match fetch_releases(repo) {
        Ok(releases) => filter_releases(releases, app_state.version_mode()),
        Err(err) => {
            // Failed connection
            println!("Releases fetch failed: {:?}", err.source());
            app_state.set_operation_failed(&format!("获取 Release 失败：{err}"));
            return;
        }
    };
    if latest_releases.is_empty() {
        // Connected, but no results (or none left after the version-mode filter)
        println!("Releases fetch returned 0 releases");
        app_state.set_operation_failed(match app_state.version_mode() {
            VersionMode::Stable => "未获取到任何稳定版 Release",
            VersionMode::Prerelease => "未获取到任何 Release",
        });
        return;
    }

    // Fetch latest tag information
    app_state.start_operation("正在获取 NextUI Tag 列表...");
    let mut latest_tags = match fetch_tags(repo) {
        Ok(tags) => tags,
        Err(err) => {
            // Failed connection
            println!("Tags fetch failed: {:?}", err.source());
            app_state.set_operation_failed(&format!("获取 Tag 失败：{err}"));
            return;
        }
    };
    if latest_tags.is_empty() {
        // Connected, but no results
        println!("Tags fetch returned 0 tags");
        app_state.set_operation_failed("未获取到任何 Tag");
        return;
    }

    // Build ReleaseAndTag list for app state
    let mut releases_and_tags: Vec<ReleaseAndTag> = vec![];
    let mut check_latest_release = true;
    let current_tag = app_state.current_version().unwrap_or_default();
    let mut current_tag_found = false;
    for release in &latest_releases {
        if let Some(tag_index) = latest_tags
            .iter()
            .position(|tag| tag.name == release.tag_name)
        {
            releases_and_tags.push(ReleaseAndTag {
                release: (release.clone()),
                tag: (latest_tags[tag_index].clone()),
            });
            if latest_tags[tag_index].commit.sha.starts_with(&current_tag) {
                // set release selector starting index
                app_state.set_nextui_releases_and_tags_index(Some(releases_and_tags.len() - 1));
                current_tag_found = true;
            }
            latest_tags.remove(tag_index);
            if check_latest_release {
                check_latest_release = false;
            }
            continue;
        }
        if check_latest_release {
            // Failed to find a match for the first release
            println!("Latest release has no matching tag: {:?}", release.tag_name);
            app_state.set_operation_failed(&format!(
                "最新 Release 找不到对应的 Tag：{:?}",
                release.tag_name
            ));
            return;
        }
    }

    // Save collected values to app state
    app_state.set_nextui_release(Some(releases_and_tags[0].release.clone()));
    app_state.set_nextui_tag(Some(releases_and_tags[0].tag.clone()));
    app_state.set_nextui_releases_and_tags(Some(releases_and_tags));
    if !current_tag_found {
        app_state.set_nextui_releases_and_tags_index(Some(0_usize));
    }

    app_state.finish_operation();
}

pub fn do_self_update(app_state: &AppStateManager) {
    // Do self-update
    let result = self_update(app_state);
    match result {
        Ok(()) => {
            app_state.finish_operation();
        }
        Err(err) => {
            println!("Self-update failed: {:?}", err.source());
            app_state.set_operation_failed(&format!("自更新失败：{err}"));
        }
    }
}

pub fn do_update(app_state: &'static AppStateManager, full: bool) {
    thread::spawn(move || {
        if let Err(err) = update_nextui(app_state, full) {
            println!("Update failed: {:?}", err.source());

            app_state.set_operation_failed(&format!("更新失败：{err}"));

            // Try to fetch latest release information again
            do_nextui_release_check(app_state);
        }
    });
}

pub fn update_nextui(app_state: &AppStateManager, full: bool) -> Result<()> {
    let mut release = {
        app_state.start_operation("正在下载更新包...");

        app_state.nextui_release().clone().ok_or("未找到可用版本")?
    };
    if app_state.release_selection_menu() {
        let index = app_state.nextui_releases_and_tags_index().unwrap_or(0);
        let relase_and_tag_vector = app_state.nextui_releases_and_tags().unwrap_or_default();
        release = relase_and_tag_vector
            .get(index)
            .ok_or("未找到可用版本")?
            .release
            .clone();
    }

    let assets = release.assets;
    let asset = assets
        .iter()
        .find(|a| a.name.contains(if full { "all" } else { "base" }))
        .or(assets.first())
        .ok_or("未找到可下载的资源文件")?;

    // Download the asset
    app_state.start_determinate_operation(&format!("正在下载 {}...", asset.name));
    println!("Downloading from {}", asset.url);

    let bytes = download(&asset.url, |pr| app_state.update_progress(pr))?;

    app_state.set_current_operation(format!("正在解压 {}...\n请稍候...", asset.name).into());
    app_state.set_progress(Some(Progress::Indeterminate));

    // Extract the update package
    if full {
        // Full update, extract all files, except for Roms folders which already
        // existed *before* this update (folders created by the update itself
        // must still get their contents).
        let existing = existing_emu_tags(&PathBuf::from(&*SDCARD_ROOT).join("Roms"));
        println!("已有的 Roms 模拟器标签：{existing:?}");
        extract_zip(
            bytes,
            |file| full_update_filter(file, &existing),
            |pr| app_state.update_progress(pr),
        )?;
    } else {
        // "Quick" update, just extract MinUI.zip and trimui folder
        extract_zip(
            bytes,
            |file| {
                ["MinUI.zip", "trimui"]
                    .iter()
                    .any(|prefix| file.starts_with(prefix))
            },
            |pr| app_state.update_progress(pr),
        )?;
    }

    println!("Extraction complete!");
    app_state.set_progress(Some(Progress::Indeterminate));

    app_state.set_current_operation(Some("更新完成，准备重启...".to_string()));

    // Give the user a moment to see the completion message
    thread::sleep(std::time::Duration::from_secs(2));

    app_state.set_current_operation(Some("正在重启系统...".to_string()));

    // Reboot the system
    match std::process::Command::new("reboot").output() {
        Ok(_) => Ok(()),
        Err(e) => Err(Box::new(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, prerelease: bool, published_at: &str) -> Release {
        Release {
            tag_name: tag.to_string(),
            assets: vec![],
            prerelease,
            draft: false,
            published_at: Some(published_at.to_string()),
        }
    }

    // GitHub's API order for NextUI-CN at the time of writing
    fn sample() -> Vec<Release> {
        vec![
            rel("v6.15.0-CN-fix1", false, "2026-11-13T10:11:52Z"),
            rel("v6.15.0-CN", false, "2026-11-13T01:26:49Z"),
            rel("v6.15.0-CN-beta1", true, "2026-11-13T11:38:59Z"),
            rel("v6.6.1-CN", false, "2026-02-27T17:41:41Z"),
        ]
    }

    fn tags(v: &[Release]) -> Vec<&str> {
        v.iter().map(|r| r.tag_name.as_str()).collect()
    }

    #[test]
    fn stable_mode_skips_prereleases() {
        let r = filter_releases(sample(), VersionMode::Stable);
        assert_eq!(tags(&r), ["v6.15.0-CN-fix1", "v6.15.0-CN", "v6.6.1-CN"]);
    }

    #[test]
    fn prerelease_mode_includes_them_newest_first() {
        let r = filter_releases(sample(), VersionMode::Prerelease);
        assert_eq!(
            tags(&r),
            [
                "v6.15.0-CN-beta1",
                "v6.15.0-CN-fix1",
                "v6.15.0-CN",
                "v6.6.1-CN"
            ]
        );
    }

    fn urel(tag: &str, prerelease: bool) -> Release {
        let mut r = rel(tag, prerelease, "2026-01-01T00:00:00Z");
        r.assets = ["Updater.pak.zip", "nextui-updater-pak.zip"]
            .iter()
            .map(|n| crate::github::Asset {
                name: (*n).to_string(),
                url: format!("http://x/{n}"),
            })
            .collect();
        r
    }

    #[test]
    fn updater_version_ordering() {
        assert!(is_newer_updater("v0.5.0-fix", "v0.5.0"));
        assert!(is_newer_updater("v0.5.0-fix2", "v0.5.0-fix"));
        assert!(is_newer_updater("v0.5.0-fix3", "0.5.0"));
        assert!(is_newer_updater("v0.6.0", "v0.5.0-fix3"));
        assert!(is_newer_updater("v0.6.0", "v0.6.0-beta1"));
        assert!(is_newer_updater("v0.6.0-beta2", "v0.6.0-beta1"));
        assert!(is_newer_updater("v0.6.0-beta1", "v0.5.0-fix3"));
        assert!(!is_newer_updater("v0.5.0-fix3", "v0.5.0-fix3"));
        assert!(!is_newer_updater("v0.5.0", "v0.5.0-fix3"));
        assert!(!is_newer_updater("0.0.1", "v0.5.0"));
        assert!(!is_newer_updater("garbage", "v0.5.0"));
        assert!(!is_newer_updater("v9.9.9", "garbage"));
    }

    #[test]
    fn updater_release_respects_version_mode() {
        let releases = vec![
            urel("v0.5.0-fix3", false),
            urel("v0.6.0-beta1", true),
            urel("v0.5.0-fix2", false),
        ];
        let stable = pick_updater_release(releases.clone(), VersionMode::Stable).unwrap();
        assert_eq!(stable.tag_name, "v0.5.0-fix3");
        let pre = pick_updater_release(releases, VersionMode::Prerelease).unwrap();
        assert_eq!(pre.tag_name, "v0.6.0-beta1");
    }

    #[test]
    fn full_update_keeps_existing_roms_and_fills_new_ones() {
        let existing: HashSet<String> = ["GB".to_string()].into();
        assert!(!full_update_filter("Roms/Game Boy (GB)/", &existing));
        assert!(!full_update_filter("Roms/Game Boy (GB)/x.txt", &existing));
        // A user folder with a different name but the same tag is respected too
        assert!(!full_update_filter("Roms/GB Games (GB)/x.txt", &existing));
        assert!(full_update_filter("Roms/Sega Genesis (MD)/", &existing));
        assert!(full_update_filter(
            "Roms/Sega Genesis (MD)/from_zip.txt",
            &existing
        ));
        assert!(full_update_filter(
            "Roms/Sega Genesis (MD)/game (GB).md",
            &existing
        ));
        assert!(full_update_filter("Roms/", &existing));
        assert!(full_update_filter("MinUI.zip", &existing));
    }

    #[test]
    fn existing_tags_are_snapshotted_from_disk() {
        let dir = std::env::temp_dir().join(format!("nui-roms-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Game Boy (GB)")).unwrap();
        std::fs::write(dir.join("notes (MD).txt"), "file, not a folder").unwrap();
        let tags = existing_emu_tags(&dir);
        // Creating a folder afterwards (as extraction does) must not change the snapshot
        std::fs::create_dir_all(dir.join("Sega Genesis (MD)")).unwrap();
        assert_eq!(tags, HashSet::from(["GB".to_string()]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn drafts_are_always_skipped() {
        let mut v = sample();
        v[0].draft = true;
        let r = filter_releases(v, VersionMode::Prerelease);
        assert!(!tags(&r).contains(&"v6.15.0-CN-fix1"));
    }
}
