//! What the user has chosen, kept in a small file next to the log.
//!
//! One `key=value` per line in `%LOCALAPPDATA%\Picline\settings.txt`. There are
//! only a handful of settings, so the whole file is rewritten on every change
//! rather than growing a parser for a format nobody else reads.

use std::path::PathBuf;

fn file() -> Option<PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    let folder = PathBuf::from(base).join("Picline");
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder.join("settings.txt"))
}

fn load() -> Vec<(String, String)> {
    let Some(path) = file() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

pub fn get(key: &str) -> Option<String> {
    load()
        .into_iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

pub fn set(key: &str, value: &str) {
    let mut pairs = load();
    match pairs.iter_mut().find(|(name, _)| name == key) {
        Some(slot) => slot.1 = value.to_string(),
        None => pairs.push((key.to_string(), value.to_string())),
    }
    let Some(path) = file() else { return };
    let mut text = String::new();
    for (name, value) in pairs {
        text.push_str(&format!("{name}={value}\n"));
    }
    // Best effort: a read-only folder only means the choice is not remembered.
    let _ = std::fs::write(path, text);
}
