use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=resources/tomato.rc");
    println!("cargo:rerun-if-changed=resources/tomato.ico");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=WINDRES");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let target = env::var("TARGET").expect("Cargo target");
    let directory = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("resources");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let resource = output.join(if msvc { "tomato.res" } else { "tomato.o" });
    let mut compiler = if let Some(path) = env::var_os("RC") {
        Command::new(path)
    } else if msvc {
        cc::windows_registry::find_tool(&target, "rc.exe")
            .map(|tool| tool.to_command())
            .unwrap_or_else(|| Command::new("rc.exe"))
    } else {
        let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("target architecture");
        let arch = if arch == "x86" { "i686" } else { &arch };
        Command::new(
            env::var_os("WINDRES").unwrap_or_else(|| format!("{arch}-w64-mingw32-windres").into()),
        )
    };
    compiler.current_dir(&directory);
    if msvc {
        compiler
            .args(["/nologo", "/fo"])
            .arg(&resource)
            .arg("tomato.rc");
    } else {
        compiler
            .args([
                "--input",
                "tomato.rc",
                "--output-format",
                "coff",
                "--output",
            ])
            .arg(&resource);
    }
    let status = compiler
        .status()
        .expect("Windows resource compiler (Windows SDK rc.exe or MinGW windres) is required");
    assert!(
        status.success(),
        "Windows icon resource compilation failed: {status}"
    );
    println!("cargo:rustc-link-arg-bin=tomato={}", resource.display());
}
