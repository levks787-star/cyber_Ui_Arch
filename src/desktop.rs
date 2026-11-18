use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub exec: String,
    pub icon: String,
    pub wm_class: String,
}

pub fn scan_apps() -> Vec<AppEntry> {
    let mut out = Vec::new();
    for dir in data_dirs() {
        let apps = dir.join("applications");
        let Ok(rd) = std::fs::read_dir(&apps) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if let Some(app) = parse_desktop(&text, &path) {
                if !out.iter().any(|e: &AppEntry| e.id == app.id) {
                    out.push(app);
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub fn parse_desktop(text: &str, path: &Path) -> Option<AppEntry> {
    let mut name = None;
    let mut exec = None;
    let mut icon = String::new();
    let mut wm = String::new();
    let mut no_display = false;
    let mut app_type = false;
    let mut in_action = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_action = line != "[Desktop Entry]";
            continue;
        }
        if in_action || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        match key.trim() {
            "Type" if value.trim() == "Application" => app_type = true,
            "NoDisplay" if value.trim().eq_ignore_ascii_case("true") => no_display = true,
            "Hidden" if value.trim().eq_ignore_ascii_case("true") => no_display = true,
            "Name" if name.is_none() => name = Some(value.trim().to_string()),
            "Exec" if exec.is_none() => exec = Some(strip_fields(value.trim())),
            "Icon" if icon.is_empty() => icon = value.trim().to_string(),
            "StartupWMClass" if wm.is_empty() => wm = value.trim().to_string(),
            _ => {}
        }
    }
    if !app_type || no_display {
        return None;
    }
    let name = name?;
    let exec = exec.filter(|s| !s.is_empty())?;
    let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name.clone());
    if wm.is_empty() {
        wm = id.clone();
    }
    Some(AppEntry { id, name, exec, icon, wm_class: wm })
}

fn strip_fields(exec: &str) -> String {
    exec.split_whitespace()
        .filter(|part| !part.starts_with('%'))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn launch(exec: &str) {
    if exec.is_empty() {
        return;
    }
    let _ = Command::new("sh").arg("-c").arg(exec).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

pub fn find_icon(name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let direct = PathBuf::from(name);
    if direct.is_file() {
        return Some(direct);
    }
    let themes = ["hicolor", "Adwaita", "Papirus", "breeze"];
    let sizes = ["48x48", "64x64", "32x32", "128x128", "scalable", "256x256"];
    let cats = ["apps", "places", "devices", "mimetypes", "categories"];
    let exts = ["png", "svg"];
    for dir in data_dirs() {
        for theme in themes {
            for size in sizes {
                for cat in cats {
                    for ext in exts {
                        let path = dir.join("icons").join(theme).join(size).join(cat).join(format!("{name}.{ext}"));
                        if path.is_file() {
                            return Some(path);
                        }
                    }
                }
            }
        }
        let pix = dir.join("pixmaps").join(format!("{name}.png"));
        if pix.is_file() {
            return Some(pix);
        }
    }
    None
}

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("XDG_DATA_HOME") {
        if !home.is_empty() {
            dirs.push(PathBuf::from(home));
        }
    } else if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    if let Ok(list) = std::env::var("XDG_DATA_DIRS") {
        for part in list.split(':') {
            if !part.is_empty() {
                dirs.push(PathBuf::from(part));
            }
        }
    } else {
        dirs.push(PathBuf::from("/usr/local/share"));
        dirs.push(PathBuf::from("/usr/share"));
    }
    dirs
}

pub fn discover_fonts() -> Vec<(String, Option<PathBuf>)> {
    let mut found = vec![("JetBrains Mono".to_string(), None)];
    let roots = ["/usr/share/fonts", "/usr/local/share/fonts"];
    let mut extra = Vec::new();
    for root in roots {
        let _ = walk_fonts(Path::new(root), &mut extra, 0);
    }
    if let Ok(home) = std::env::var("HOME") {
        let _ = walk_fonts(&PathBuf::from(home).join(".local/share/fonts"), &mut extra, 0);
    }
    extra.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    extra.truncate(24);
    found.extend(extra);
    found
}

fn walk_fonts(dir: &Path, out: &mut Vec<(String, Option<PathBuf>)>, depth: u32) -> std::io::Result<()> {
    if depth > 3 || out.len() > 24 || !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_fonts(&path, out, depth + 1)?;
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "ttf" && ext != "otf" {
            continue;
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().replace(['-', '_'], " ")).unwrap_or_default();
        let interesting = ["dejavu", "noto sans", "noto sans mono", "liberation", "adwaita", "jetbrains", "inter", "source sans", "hack"];
        let lower = stem.to_lowercase();
        if interesting.iter().any(|k| lower.contains(k)) && !out.iter().any(|(n, _)| n.eq_ignore_ascii_case(&stem)) {
            out.push((stem, Some(path)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_application_and_strips_field_codes() {
        let text = "[Desktop Entry]\nType=Application\nName=Kitty\nExec=kitty --single-instance %F\nIcon=kitty\nStartupWMClass=kitty\n";
        let app = parse_desktop(text, Path::new("/usr/share/applications/kitty.desktop")).unwrap();
        assert_eq!(app.name, "Kitty");
        assert_eq!(app.exec, "kitty --single-instance");
        assert_eq!(app.wm_class, "kitty");
        assert_eq!(app.id, "kitty");
    }

    #[test]
    fn skips_hidden_and_actions() {
        let text = "[Desktop Entry]\nType=Application\nNoDisplay=true\nName=Hidden\nExec=hidden\n\n[Desktop Action New]\nName=New\nExec=hidden --new\n";
        assert!(parse_desktop(text, Path::new("hidden.desktop")).is_none());
    }
}
