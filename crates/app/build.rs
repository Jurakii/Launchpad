fn main() {
    slint_build::compile_with_config("ui/main.slint", slint_build::CompilerConfiguration::new().with_style("fluent".into())).unwrap();
    // The app icon as resource 1: Explorer, the taskbar, the tray and notifications use it.
    embed_resource::compile("launchpad.rc", embed_resource::NONE).manifest_optional().unwrap();
    println!("cargo:rerun-if-changed=../../build/icon.ico");
}
