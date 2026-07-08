fn main() {
    // Compile the Swift system-audio helper for macOS targets; the binary is
    // embedded via include_bytes! and written out at runtime.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
        let out = format!("{out_dir}/arya-system-audio-helper");
        println!("cargo:rerun-if-changed=native/system-audio-helper/main.swift");
        // AudioHardwareCreateProcessTap and friends require macOS 14.2. swiftc's
        // availability check keys off the -target triple (not MACOSX_DEPLOYMENT_TARGET),
        // so pass an explicit deployment target; otherwise it inherits the host
        // default — 14.0 on the macos-14 CI runner — and fails to compile even
        // though the SDK ships the symbols.
        let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("x86_64") => "x86_64",
            _ => "arm64", // Rust's "aarch64" is spelled "arm64" in an Apple triple
        };
        let target = format!("{arch}-apple-macosx14.2");
        let status = std::process::Command::new("swiftc")
            .args([
                "-O",
                "-framework",
                "CoreAudio",
                "-framework",
                "AVFoundation",
                "-target",
                &target,
                "-o",
                &out,
                "native/system-audio-helper/main.swift",
            ])
            .status()
            .expect("failed to run swiftc (Xcode command line tools required)");
        assert!(
            status.success(),
            "swiftc failed to build the system-audio helper"
        );
    }
    tauri_build::build()
}
