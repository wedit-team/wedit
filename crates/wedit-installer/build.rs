use std::path::PathBuf;

fn main() {
    // The installer embeds the release build of Wedit. Build it first:
    //   cargo build --release -p wedit
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let payload = std::env::var("WEDIT_PAYLOAD")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest.join("../../target/release/wedit.exe"));
    let payload = payload.canonicalize().unwrap_or_else(|_| {
        panic!(
            "Wedit payload not found at {}.\nBuild the editor first with `cargo build --release -p wedit` (or run build.ps1).",
            payload.display()
        )
    });
    println!("cargo:rerun-if-changed={}", payload.display());
    println!("cargo:rerun-if-env-changed=WEDIT_PAYLOAD");
    println!("cargo:rustc-env=WEDIT_PAYLOAD_PATH={}", payload.display());

    println!("cargo:rerun-if-changed=../../assets/wedit.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/wedit.ico")
            .set("ProductName", "Wedit")
            .set("FileDescription", "Wedit Setup")
            .set("CompanyName", "Wedit")
            .set("LegalCopyright", "Wedit");
        res.compile().expect("failed to embed Windows resources");
    }
}
