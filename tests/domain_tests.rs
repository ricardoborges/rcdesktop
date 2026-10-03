use rcdesktop::domain::container::{Container, ContainerState, PortMapping};
use rcdesktop::domain::session::WslcSessionInfo;

#[test]
fn test_container_state_parsing() {
    let state = ContainerState::from_str_loose("Up 2 hours");
    assert_eq!(state, ContainerState::Running);

    let state = ContainerState::from_str_loose("Exited (0) 10 minutes ago");
    assert_eq!(state, ContainerState::Exited(0));

    let state = ContainerState::from_str_loose("Created");
    assert_eq!(state, ContainerState::Created);
}

#[test]
fn test_port_mapping_parse() {
    let port = PortMapping::parse("0.0.0.0:8080->80/tcp");
    assert!(port.is_some());
    let p = port.unwrap();
    assert_eq!(p.host_port, 8080);
    assert_eq!(p.container_port, 80);
    assert_eq!(p.protocol, "tcp");
}

#[test]
fn test_session_elevation_warning() {
    let s = WslcSessionInfo {
        session_id: "default".into(),
        is_elevated: true,
        is_healthy: true,
        status_message: "Active".into(),
        version: "5.0.1.1".into(),
    };
    assert!(s.is_elevated);
}

#[test]
fn test_container_primary_name() {
    let c = Container {
        id: "abcdef123456".into(),
        names: vec!["/my-web-app".into()],
        image: "nginx:alpine".into(),
        command: "nginx".into(),
        created: "1h".into(),
        status: "Running".into(),
        state: ContainerState::Running,
        ports: vec![],
        compose_project: Some("project1".into()),
    };
    assert_eq!(c.primary_name(), "my-web-app");
}
