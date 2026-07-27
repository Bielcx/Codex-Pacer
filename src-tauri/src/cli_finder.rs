//! Shared PATH lookup for npm-installed CLI binaries (codex, claude, ...).
//!
//! npm's global installer drops several shims per package on Windows (an
//! extension-less POSIX shell script for Git Bash/WSL, plus `.cmd`/`.ps1`
//! for native Windows). `where` lists the extension-less one first because
//! it matches the bare name exactly, but CreateProcess can't execute it
//! directly (error 193), so a real `.exe` or `.cmd`/`.bat` shim is preferred.

use std::process::Command;

pub fn find_cli_binary(name: &str, fallback_paths: &[&str]) -> Option<String> {
    let finder = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    if let Ok(output) = Command::new(finder).arg(name).output() {
        if output.status.success() {
            if let Ok(text) = String::from_utf8(output.stdout) {
                let candidates: Vec<&str> = text
                    .lines()
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty())
                    .collect();

                if cfg!(target_os = "windows") {
                    if let Some(exe) = candidates
                        .iter()
                        .find(|p| p.to_lowercase().ends_with(".exe"))
                    {
                        return Some(exe.to_string());
                    }
                    if let Some(cmd) = candidates.iter().find(|p| {
                        let lower = p.to_lowercase();
                        lower.ends_with(".cmd") || lower.ends_with(".bat")
                    }) {
                        return Some(cmd.to_string());
                    }
                }

                if let Some(first) = candidates.first() {
                    return Some(first.to_string());
                }
            }
        }
    }

    fallback_paths
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|s| s.to_string())
}
