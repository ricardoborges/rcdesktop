fn main() {
    // The app is dark-only; keep std widgets (TextEdit, Spinner) dark too
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint build failed");
}
