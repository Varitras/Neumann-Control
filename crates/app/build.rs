//! Gives the Windows executable its icon (Explorer, desktop shortcuts); the running window sets
//! its own from the same picture in `main.rs`.

fn main() {
    println!("cargo:rerun-if-changed=assets/brand/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/brand/icon.ico");
    resource
        .compile()
        .expect("the icon resource compiles with the Windows SDK's rc.exe");
}
