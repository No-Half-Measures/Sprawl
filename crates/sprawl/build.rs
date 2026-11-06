fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        #[cfg(windows)]
        winresource::WindowsResource::new()
            .set_icon_with_id("assets/icon.ico", "1")
            .set("ProductName", "Sprawl")
            .set("FileDescription", "Sprawl")
            .set_manifest_file("assets/app.manifest")
            .compile()
            .expect("compile Windows resources");
    }
}
