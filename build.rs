//! On Windows: puts the icon and version info into the .exe, so Explorer, the taskbar
//! and the Start menu show it.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "oh! Presenter");
        res.set("FileDescription", "oh! Presenter");
        if let Err(e) = res.compile() {
            // a missing resource compiler only costs the icon, not the build
            println!("cargo:warning=no icon in the .exe: {e}");
        }
    }
}
