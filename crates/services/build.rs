fn main() {
    let target = std::env::var("TARGET").unwrap();
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../vendor/access-export");
    let name = if target.contains("windows") {
        "tomato-access.exe"
    } else {
        "tomato-access"
    };
    let binary = std::env::var_os("TOMATO_ACCESS_EXPORTER")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("bin").join(&target).join(name));
    println!("cargo:rerun-if-env-changed=TOMATO_ACCESS_EXPORTER");
    println!("cargo:rerun-if-changed={}", binary.display());
    assert!(
        binary.is_file(),
        "Build the embedded native Access writer first: python scripts/build-access-export.py (GraalVM 25 required)"
    );
    println!(
        "cargo:rustc-env=TOMATO_ACCESS_EXPORTER={}",
        binary.canonicalize().unwrap().display()
    );
    if target.contains("windows") {
        let reader = root.join("../odbc-import/bin/i686-pc-windows-msvc/tomato-odbc.exe");
        assert!(
            reader.is_file(),
            "Build the ODBC x86 bridge first: python scripts/build-odbc32.py"
        );
        println!("cargo:rerun-if-changed={}", reader.display());
        println!(
            "cargo:rustc-env=TOMATO_ODBC32_READER={}",
            reader.canonicalize().unwrap().display()
        );
    }
}
