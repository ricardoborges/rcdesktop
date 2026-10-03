fn main() {
    // The app is dark-only; keep std widgets (TextEdit, Spinner) dark too
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint build failed");

    // App icon for Explorer, the taskbar and the tray (resource ID 1)
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/rcdesktop.ico");
        embed_resource::compile("assets/rcdesktop.rc", embed_resource::NONE)
            .manifest_required()
            .expect("Embedding the app icon failed");
    }
}
