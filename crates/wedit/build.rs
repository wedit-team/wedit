fn main() {
    println!("cargo:rerun-if-changed=../../assets/wedit.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/wedit.ico")
            .set("ProductName", "Wedit")
            .set("FileDescription", "Wedit Code Editor")
            .set("CompanyName", "Wedit")
            .set("LegalCopyright", "Wedit");
        res.compile().expect("failed to embed Windows resources");
    }
}
