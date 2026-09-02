#![cfg(target_os = "linux")]

use std::process::Command;

const REPO: &str = "FolderFile/rush";
const INSTALL_PATH: &str = "/usr/bin/rush";

fn download(url: &str, dest: &str) -> Result<(), String> {
    let curl = Command::new("curl")
        .args(["-fsSL", url, "-o", dest])
        .status();
    let mut ok = match curl {
        Ok(s) if s.success() => true,
        _ => Command::new("wget")
            .args(["-qO", dest, url])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
    };
    if !ok {
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.is_empty() {
                ok = Command::new("curl")
                    .args(["-fsSL", "-H", &format!("Authorization: token {}", token), url, "-o", dest])
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
                REPO,
                "--pattern",
                "rush-linux",
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
            "could not download {} (private repo? use gh auth or set GITHUB_TOKEN)",
            url
        ))
    }
}

fn is_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().unwrap_or(1) == 0)
        .unwrap_or(false)
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

pub fn update() -> Result<(), String> {
    let target = installed_path().unwrap_or_else(|| std::path::PathBuf::from(INSTALL_PATH));
    let url = format!("https://github.com/{}/releases/latest/download/rush-linux", REPO);
    let tmp = format!("/tmp/rush.update.{}", std::process::id());
    download(&url, &tmp)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    let version = Command::new(&tmp)
        .arg("--version")
        .output()
        .map_err(|e| format!("downloaded binary is broken: {}", e))?;
    if !version.status.success() || !String::from_utf8_lossy(&version.stdout).starts_with("rush") {
        std::fs::remove_file(&tmp).ok();
        return Err("downloaded binary is not rush".into());
    }
    let staged = target.with_extension("update");
    std::fs::copy(&tmp, &staged).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot stage {}: {} (are you root?)", target.display(), e)
    })?;
    let _ = std::fs::remove_file(&tmp);
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&staged, &target).map_err(|e| {
        format!("cannot replace {}: {} (are you root?)", target.display(), e)
    })?;
    println!("rush updated to {} at {}", crate::VERSION, target.display());
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    if !is_root() {
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
