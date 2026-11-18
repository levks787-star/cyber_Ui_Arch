use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct HyprSnap {
    #[allow(dead_code)]
    pub available: bool,
    pub workspaces: Vec<i32>,
    pub active_workspace: i32,
    pub clients: Vec<HyprClient>,
}

#[derive(Clone, Debug)]
pub struct HyprClient {
    pub address: String,
    pub class: String,
    #[allow(dead_code)]
    pub title: String,
    #[allow(dead_code)]
    pub workspace: i32,
}

pub fn start() -> Arc<Mutex<HyprSnap>> {
    let slot = Arc::new(Mutex::new(HyprSnap::default()));
    let shared = Arc::clone(&slot);
    let _ = thread::Builder::new().name("acc-hypr".into()).spawn(move || loop {
        let snap = poll();
        if let Ok(mut guard) = shared.lock() {
            *guard = snap;
        }
        thread::sleep(Duration::from_millis(450));
    });
    slot
}

pub fn focus_workspace(id: i32) {
    dispatch(&format!("workspace {id}"));
}

pub fn focus_client(address: &str) {
    if address.is_empty() {
        return;
    }
    dispatch(&format!("focuswindow address:{address}"));
}

fn dispatch(arg: &str) {
    let _ = Command::new("hyprctl")
        .args(["dispatch", arg])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn poll() -> HyprSnap {
    let Some(ws) = hyprctl(&["workspaces"]) else {
        return HyprSnap::default();
    };
    let Some(clients) = hyprctl(&["clients"]) else {
        return HyprSnap { available: true, ..HyprSnap::default() };
    };
    let active = hyprctl(&["activeworkspace"]).unwrap_or_default();
    HyprSnap {
        available: true,
        workspaces: parse_workspaces(&ws),
        active_workspace: parse_active(&active).unwrap_or(1),
        clients: parse_clients(&clients),
    }
}

fn hyprctl(args: &[&str]) -> Option<String> {
    let out = Command::new("hyprctl").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn parse_workspaces(text: &str) -> Vec<i32> {
    let mut ids = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("workspace ID ") {
            if let Some(id) = rest.split_whitespace().next().and_then(|s| s.trim_end_matches('(').parse().ok()) {
                ids.push(id);
            }
        }
    }
    if ids.is_empty() {
        ids.push(1);
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn parse_active(text: &str) -> Option<i32> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("workspace ID ") {
            return rest.split_whitespace().next()?.trim_end_matches('(').parse().ok();
        }
    }
    None
}

fn parse_clients(text: &str) -> Vec<HyprClient> {
    let mut out = Vec::new();
    let mut address = String::new();
    let mut class = String::new();
    let mut title = String::new();
    let mut workspace = 1;
    let flush = |out: &mut Vec<HyprClient>, address: &mut String, class: &mut String, title: &mut String, workspace: i32| {
        if !address.is_empty() || !class.is_empty() {
            out.push(HyprClient {
                address: address.clone(),
                class: class.clone(),
                title: title.clone(),
                workspace,
            });
        }
        address.clear();
        class.clear();
        title.clear();
    };
    for line in text.lines() {
        let trim = line.trim();
        if trim.starts_with("Window ") {
            flush(&mut out, &mut address, &mut class, &mut title, workspace);
            if let Some(rest) = trim.strip_prefix("Window ") {
                address = rest.split_whitespace().next().unwrap_or("").to_string();
            }
            workspace = 1;
            continue;
        }
        if let Some(rest) = trim.strip_prefix("class: ") {
            class = rest.trim().to_string();
        } else if let Some(rest) = trim.strip_prefix("title: ") {
            title = rest.trim().to_string();
        } else if let Some(rest) = trim.strip_prefix("workspace: ") {
            workspace = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(1);
        }
    }
    flush(&mut out, &mut address, &mut class, &mut title, workspace);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hyprctl_text() {
        let ws = "workspace ID 2 (2) on monitor DP-1:\nworkspace ID 1 (1) on monitor DP-1:\n";
        let mut ids = parse_workspaces(ws);
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2]);
        let clients = "Window 0x55 -> kitty:\n\tclass: kitty\n\ttitle: zsh\n\tworkspace: 2 (2)\nWindow 0x66 -> firefox:\n\tclass: firefox\n\ttitle: Mozilla\n\tworkspace: 1 (1)\n";
        let parsed = parse_clients(clients);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].class, "kitty");
        assert_eq!(parsed[0].workspace, 2);
        assert_eq!(parsed[1].address, "0x66");
    }
}
