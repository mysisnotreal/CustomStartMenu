fn main() {
    // Embeds the app icon and version info (same as the C++ backend).
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=../assets/logo.ico");
    let _ = embed_resource::compile("app.rc", embed_resource::NONE);
}
