use rcdesktop::app::AppController;
use rcdesktop::rcompose::{strip_ansi, ComposeTarget};

const YAML: &str = "services:\n  web:\n    image: nginx:alpine\n";

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rcdesktop-test-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn pasted_yaml_is_saved_under_the_stack_folder() {
    let root = scratch("pasted");
    let t = ComposeTarget::prepare(YAML, "blog", "", &root).unwrap();
    assert_eq!(t.file, root.join("blog").join("compose.yaml"));
    assert_eq!(t.dir, root.join("blog"));
    assert_eq!(t.project.as_deref(), Some("blog"));
    assert!(!t.temporary);
    assert_eq!(std::fs::read_to_string(&t.file).unwrap(), YAML);
    assert_eq!(t.up_args()[2..], ["-p", "blog", "up", "-d"]);
}

#[test]
fn pasted_yaml_uses_top_level_name_or_asks_for_one() {
    let root = scratch("named");
    let t = ComposeTarget::prepare(&format!("name: shop\n{}", YAML), "", "", &root).unwrap();
    assert_eq!(t.project.as_deref(), Some("shop"));
    assert!(ComposeTarget::prepare(YAML, "", "", &root).unwrap_err().contains("stack name"));
}

#[test]
fn unchanged_file_runs_in_place_and_edited_file_uses_a_sibling_copy() {
    let dir = scratch("file");
    let file = dir.join("compose.yml");
    std::fs::write(&file, YAML).unwrap();
    let file_str = file.display().to_string();

    let t = ComposeTarget::prepare(YAML, "", &file_str, &dir).unwrap();
    assert_eq!(t.file, file);
    assert_eq!(t.dir, dir);
    assert_eq!(t.project, None);
    assert!(!t.temporary);

    let edited = format!("{}    ports: [\"8080:80\"]\n", YAML);
    let t = ComposeTarget::prepare(&edited, "", &file_str, &dir).unwrap();
    assert_eq!(t.file, dir.join(".compose.rcdesktop.yaml"));
    assert_eq!(t.dir, dir);
    assert!(t.temporary);
    assert_eq!(std::fs::read_to_string(&t.file).unwrap(), edited);
    // The original is left untouched
    assert_eq!(std::fs::read_to_string(&file).unwrap(), YAML);
}

#[test]
fn preview_shows_command_and_services() {
    let p = AppController::compose_preview(YAML, "blog", "");
    assert!(p.starts_with("$ rcompose -f "), "{}", p);
    assert!(p.contains("-p blog up -d"), "{}", p);
    assert!(p.contains("• web"), "{}", p);

    assert!(AppController::compose_preview("services: [", "", "").starts_with("⚠ Invalid YAML"));
    assert!(AppController::compose_preview("name: x\n", "", "").contains("no services"));
    assert!(AppController::compose_preview(YAML, "", "").contains("stack name"));
}

#[test]
fn ansi_colors_are_stripped() {
    assert_eq!(strip_ansi("\u{1b}[32m✔\u{1b}[0m web \u{1b}[1;34mStarted\u{1b}[0m"), "✔ web Started");
}
