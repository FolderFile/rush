#![cfg(target_os = "linux")]

use std::process::Command;

use crate::crypto::{hex, sha256};
use crate::pty;

const DEFAULT_REPO: &str = "FolderFile/rush";
const INSTALL_PATH: &str = "/usr/bin/rush";

fn repo() -> String {
    std::env::var("RUSH_REPO")
        .ok()
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| DEFAULT_REPO.to_string())
}

fn download_asset(asset: &str, dest: &str) -> Result<(), String> {
    let url = format!(
        "https://github.com/{}/releases/latest/download/{}",
        repo(),
        asset
    );
    let mut ok = Command::new("curl")
        .args(["-fsSL", &url, "-o", dest])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        ok = Command::new("wget")
            .args(["-qO", dest, &url])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
    if !ok {
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.is_empty() {
                ok = Command::new("curl")
                    .args([
                        "-fsSL",
                        "-H",
                        &format!("Authorization: token {}", token),
                        &url,
                        "-o",
                        dest,
                    ])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
            }
        }
    }
    if !ok {
        std::fs::remove_file(dest).ok();
        ok = Command::new("gh")
            .args([
                "release",
                "download",
                "--repo",
                &repo(),
                "--pattern",
                asset,
                "--output",
                dest,
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
    if ok {
        Ok(())
    } else {
        Err(format!(
            "could not download {} (private repo? run 'gh auth login' or set GITHUB_TOKEN)",
            asset
        ))
    }
}

fn installed_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if exe.starts_with("/usr/bin") || exe.starts_with("/usr/local/bin") {
        return Some(exe);
    }
    if std::path::Path::new(INSTALL_PATH).exists() {
        return Some(std::path::PathBuf::from(INSTALL_PATH));
    }
    None
}

fn file_sha256(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(hex(&sha256(&bytes)))
}

fn verify_checksum(binary_path: &str, sums_path: &str, asset: &str) -> Result<(), String> {
    let sums = std::fs::read_to_string(sums_path)
        .map_err(|e| format!("cannot read SHA256SUMS: {}", e))?;
    let expected = sums
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let sum = parts.next()?;
            let name = parts.next()?.rsplit('/').next()?;
            if name == asset {
                Some(sum.to_string())
            } else {
                None
            }
        })
        .next()
        .ok_or_else(|| format!("SHA256SUMS has no entry for {}", asset))?;
    let actual = file_sha256(binary_path)?;
    if actual != expected {
        return Err(format!(
            "checksum mismatch for {}: expected {}, got {}",
            asset, expected, actual
        ));
    }
    Ok(())
}

pub fn update() -> Result<(), String> {
    let target = installed_path().unwrap_or_else(|| std::path::PathBuf::from(INSTALL_PATH));
    let pid = std::process::id();
    let tmp_bin = format!("/tmp/rush.update.{}", pid);
    let tmp_sums = format!("/tmp/rush.sums.{}", pid);

    download_asset("rush-linux", &tmp_bin)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp_bin, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;

    download_asset("SHA256SUMS", &tmp_sums)?;
    if let Err(e) = verify_checksum(&tmp_bin, &tmp_sums, "rush-linux") {
        std::fs::remove_file(&tmp_bin).ok();
        std::fs::remove_file(&tmp_sums).ok();
        return Err(e);
    }
    std::fs::remove_file(&tmp_sums).ok();

    let version_out = Command::new(&tmp_bin)
        .arg("--version")
        .output()
        .map_err(|e| {
            std::fs::remove_file(&tmp_bin).ok();
            format!("downloaded binary is broken: {}", e)
        })?;
    let new_version = String::from_utf8_lossy(&version_out.stdout)
        .trim()
        .trim_start_matches("rush ")
        .to_string();
    if !version_out.status.success() || new_version.is_empty() {
        std::fs::remove_file(&tmp_bin).ok();
        return Err("downloaded binary is not rush".into());
    }

    let staged = target.with_extension("update");
    std::fs::copy(&tmp_bin, &staged).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_bin);
        format!("cannot stage {}: {} (are you root?)", target.display(), e)
    })?;
    let _ = std::fs::remove_file(&tmp_bin);
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&staged, &target)
        .map_err(|e| format!("cannot replace {}: {} (are you root?)", target.display(), e))?;
    println!(
        "rush updated to {} at {}",
        new_version,
        target.display()
    );
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    if !pty::running_as_root() {
        return Err("--uninstall must be run as root".into());
    }
    let systemctl = Command::new("systemctl")
        .args(["disable", "--now", "rush.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if systemctl {
        std::fs::remove_file("/etc/systemd/system/rush.service").ok();
        Command::new("systemctl").arg("daemon-reload").status().ok();
    } else {
        Command::new("rc-service").args(["rush", "stop"]).status().ok();
        Command::new("rc-update").args(["del", "rush", "default"]).status().ok();
        std::fs::remove_file("/etc/init.d/rush").ok();
    }
    for path in [INSTALL_PATH, "/usr/local/bin/rush"] {
        if std::path::Path::new(path).exists() {
            std::fs::remove_file(path).map_err(|e| format!("cannot remove {}: {}", path, e))?;
            println!("removed {}", path);
        }
    }
    println!("rush uninstalled.");
    Ok(())
}
