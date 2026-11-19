use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_windows_rc_exe() -> Option<PathBuf> {
    if Command::new("rc.exe").arg("/?").output().is_ok() {
        return Some(PathBuf::from("rc.exe"));
    }

    let pf86 = env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let kits_bin = Path::new(&pf86).join("Windows Kits").join("10").join("bin");
    if let Ok(entries) = fs::read_dir(&kits_bin) {
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        versions.sort();
        for ver_dir in versions.into_iter().rev() {
            let candidate = ver_dir.join("x64").join("rc.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.rc");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let Some(rc_exe) = find_windows_rc_exe() else {
        println!("cargo:warning=Windows SDK rc.exe bulunamadı, .exe ikonu atlandı.");
        return;
    };

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR mevcut olmalı"));
    let res_file = out_dir.join("app_icon.res");

    let status = Command::new(rc_exe)
        .args([
            "/nologo",
            "/fo",
            res_file.to_str().expect("geçerli yol"),
            "assets/app.rc",
        ])
        .status();

    if let Ok(st) = status
        && st.success()
        && res_file.exists()
    {
        println!("cargo:rustc-link-arg-bins={}", res_file.display());
    }
}
