use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/lttl.ico");
    println!("cargo:rerun-if-env-changed=RC");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") { return; }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("assets/lttl.ico");
    let resource = out.join("lttl.rc");
    let compiled = out.join("lttl.res");
    fs::write(&resource, format!("1 ICON \"{}\"\n", icon.display().to_string().replace('\\', "/"))).unwrap();
    let rc = env::var_os("RC").map(PathBuf::from).unwrap_or_else(|| {
        let sdk = PathBuf::from(env::var_os("ProgramFiles(x86)").expect("Windows SDK or RC is required"))
            .join("Windows Kits/10/bin");
        let mut candidates: Vec<_> = fs::read_dir(sdk).expect("Install the Windows SDK or set RC")
            .filter_map(Result::ok).map(|e| e.path().join("x64/rc.exe")).filter(|p| p.is_file()).collect();
        candidates.sort();
        candidates.pop().expect("Windows SDK rc.exe not found; set RC")
    });
    let status = Command::new(rc).arg("/nologo").arg("/fo").arg(&compiled).arg(&resource)
        .status().expect("Run Windows resource compiler");
    assert!(status.success(), "Windows icon resource compilation failed");
    println!("cargo:rustc-link-arg-bins={}", compiled.display());
}
