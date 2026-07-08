fn main() {
    // Compile the Swift system-audio helper for macOS targets; the binary is
    // embedded via include_bytes! and written out at runtime.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
        let out = format!("{out_dir}/arya-system-audio-helper");
        println!("cargo:rerun-if-changed=native/system-audio-helper/main.swift");
        let status = std::process::Command::new("swiftc")
            .args([
                "-O",
                "-framework",
                "CoreAudio",
                "-framework",
                "AVFoundation",
                "-o",
                &out,
                "native/system-audio-helper/main.swift",
            ])
            // AudioHardwareCreateProcessTap and friends require macOS 14.2, so
            // pin the Swift deployment target. Without this, swiftc inherits the
            // host default (older than 14.2 on the macos-14 CI runner) and the
            // availability check fails even though the SDK has the symbols.
            .env("MACOSX_DEPLOYMENT_TARGET", "14.2")
            .status()
            .expect("failed to run swiftc (Xcode command line tools required)");
        assert!(
            status.success(),
            "swiftc failed to build the system-audio helper"
        );
    }
    tauri_build::build()
}
