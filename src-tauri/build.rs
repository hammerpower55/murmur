fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        link_clang_runtime();
    }
    tauri_build::build()
}

/// whisper.cpp's Metal code uses Objective-C `@available`, which calls
/// `__isPlatformVersionAtLeast` from clang's runtime library. rustc links
/// with -nodefaultlibs, so that library is never added on its own and the
/// link fails with recent Xcode ("Undefined symbols: ___isPlatformVersionAtLeast").
fn link_clang_runtime() {
    let output = std::process::Command::new("xcrun")
        .args(["clang", "-print-resource-dir"])
        .output();
    let resources = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => {
            println!("cargo:warning=could not find clang's resource directory");
            return;
        }
    };
    let dir = std::path::Path::new(&resources).join("lib/darwin");
    if dir.join("libclang_rt.osx.a").is_file() {
        println!("cargo:rustc-link-search=native={}", dir.display());
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    } else {
        println!("cargo:warning=libclang_rt.osx.a is not in {}", dir.display());
    }
}
