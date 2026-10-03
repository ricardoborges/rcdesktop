use rcdesktop::domain::system::PruneTarget;
use rcdesktop::wslc::client::WslcClient;
use rcdesktop::wslc::mock::MockWslcClient;
use rcdesktop::wslc::parser::{parse_system_info, parse_version};

#[test]
fn version_accepts_current_and_legacy_formats() {
    assert_eq!(parse_version("wslc 3.0.1.0\n"), Some("3.0.1.0".into()));
    assert_eq!(parse_version("wslc version 5.0.1.1"), Some("5.0.1.1".into()));
    assert_eq!(parse_version("Copyright (c) Microsoft\nwslc is a tool"), None);
}

#[test]
fn system_info_parses_wslc_info_json() {
    // Real `wslc info --format json` output
    let raw = r#"{"Client":{"Direct3DVersion":"1.611.1-81528511","DxCoreVersion":"10.0.26100.1-240331-1435.ge-release","KernelVersion":"6.18.40.1-1","SettingsFile":"C:\\Users\\me\\AppData\\Local\\wslc\\settings.yaml","Version":"3.0.1.0","WindowsVersion":"10.0.26200.9457"},"Server":{"SessionManagerVersion":"3.0.1","Sessions":[{"CreatorPid":5164,"ID":1,"Name":"wslc-cli-me"}]}}"#;
    let info = parse_system_info(raw).unwrap();
    assert_eq!(info.version, "3.0.1.0");
    assert_eq!(info.kernel_version, "6.18.40.1-1");
    assert_eq!(info.settings_file, r"C:\Users\me\AppData\Local\wslc\settings.yaml");
    assert_eq!(info.session_manager_version, "3.0.1");
    assert_eq!(info.sessions.len(), 1);
    assert_eq!(info.sessions[0].id, "1");
    assert_eq!(info.sessions[0].creator_pid, "5164");
    assert!(parse_system_info("not json").is_none());
}

#[test]
fn prune_targets_map_to_forced_wslc_commands() {
    let args = |k| PruneTarget::from_key(k).unwrap().args().join(" ");
    assert_eq!(args("containers"), "container prune -f");
    assert_eq!(args("images"), "image prune -f");
    assert_eq!(args("images-all"), "image prune -a -f");
    assert_eq!(args("networks"), "network prune -f");
    assert_eq!(args("volumes"), "volume prune -a -f");
    assert!(PruneTarget::from_key("everything").is_none());
}

#[tokio::test]
async fn mock_prune_removes_only_stopped_containers() {
    let client = MockWslcClient::new();
    let out = client.prune(PruneTarget::Containers).await.unwrap();
    assert!(out.contains("db-postgres"), "{}", out);
    let left = client.list_containers(true).await.unwrap();
    assert!(!left.is_empty());
    assert!(left.iter().all(|c| c.state == rcdesktop::domain::container::ContainerState::Running));
}
