fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    if cfg!(target_os = "windows") {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "iPhonePlay")
            .set("FileDescription", "iPhonePlay")
            .set("OriginalFilename", "iPhonePlay.exe")
            .set(
                "LegalCopyright",
                "GPL-3.0-only; includes iPhoneMirror by RayrenSX and contributors",
            );
        if std::path::Path::new("assets/app.ico").exists() {
            res.set_icon("assets/app.ico");
        }
        res.compile().expect("compile Windows version resource");
    }
}
