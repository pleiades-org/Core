#[cfg(target_os = "windows")]
fn main() {
    println!("cargo:rerun-if-changed=assets/app_icon.rc");
    println!("cargo:rerun-if-changed=assets/app_icon.ico");

    embed_resource::compile("assets/app_icon.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("app icon resource should compile");
}

#[cfg(not(target_os = "windows"))]
fn main() {}
