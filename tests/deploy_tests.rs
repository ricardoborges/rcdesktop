use rcdesktop::domain::container::ContainerState;
use rcdesktop::domain::deploy::{parse_lines, split_command, ContainerSpec};
use rcdesktop::wslc::client::WslcClient;
use rcdesktop::wslc::mock::MockWslcClient;

fn spec(image: &str) -> ContainerSpec {
    ContainerSpec {
        image: image.into(),
        start: true,
        ..Default::default()
    }
}

#[test]
fn test_minimal_spec_runs_detached() {
    assert_eq!(spec("nginx:alpine").to_args(), vec!["run", "-d", "nginx:alpine"]);
}

#[test]
fn test_full_spec_args() {
    let s = ContainerSpec {
        name: "web".into(),
        image: "nginx:alpine".into(),
        command: r#"sh -c "echo hi && sleep 1""#.into(),
        ports: vec!["8080:80".into(), "127.0.0.1:8443:443/tcp".into()],
        env: vec!["TZ=UTC".into()],
        volumes: vec!["data:/data:ro".into()],
        network: "bridge".into(),
        cpus: "1.5".into(),
        memory: "512M".into(),
        pull_always: true,
        auto_remove: true,
        tty: true,
        start: true,
        ..Default::default()
    };
    assert!(s.validate().is_ok());
    assert_eq!(
        s.to_args(),
        vec![
            "run", "-d", "--name", "web", "-p", "8080:80", "-p", "127.0.0.1:8443:443/tcp", "-e", "TZ=UTC",
            "-v", "data:/data:ro", "--network", "bridge", "--cpus", "1.5", "--memory", "512M", "--pull",
            "always", "--rm", "-i", "-t", "nginx:alpine", "sh", "-c", "echo hi && sleep 1",
        ]
    );
    assert!(s.command_line().ends_with(r#"nginx:alpine sh -c "echo hi && sleep 1""#));
}

#[test]
fn test_create_without_start() {
    let s = ContainerSpec { start: false, ..spec("alpine") };
    assert_eq!(s.to_args(), vec!["create", "alpine"]);
}

#[test]
fn test_validation_errors() {
    assert!(spec("").validate().is_err());
    assert!(spec("bad image").validate().is_err());
    assert!(ContainerSpec { name: "-bad".into(), ..spec("nginx") }.validate().is_err());
    for port in ["80:", "abc:80", "8080:80/sctp", "70000:80", "1:2:3:4"] {
        let s = ContainerSpec { ports: vec![port.into()], ..spec("nginx") };
        assert!(s.validate().is_err(), "port {port} should be rejected");
    }
    for port in ["80", "8080:80", "8000-8010:8000-8010", "0.0.0.0:53:53/udp", "127.0.0.1::80"] {
        let s = ContainerSpec { ports: vec![port.into()], ..spec("nginx") };
        assert!(s.validate().is_ok(), "port {port} should be accepted");
    }
    assert!(ContainerSpec { env: vec!["NOVALUE".into()], ..spec("nginx") }.validate().is_err());
    assert!(ContainerSpec { env: vec!["EMPTY=".into()], ..spec("nginx") }.validate().is_ok());
    assert!(ContainerSpec { cpus: "0".into(), ..spec("nginx") }.validate().is_err());
    assert!(ContainerSpec { memory: "lots".into(), ..spec("nginx") }.validate().is_err());
    assert!(ContainerSpec { memory: "1g".into(), ..spec("nginx") }.validate().is_ok());
    assert!(ContainerSpec { command: "echo 'oops".into(), ..spec("nginx") }.validate().is_err());
}

#[test]
fn test_parse_lines_and_split_command() {
    assert_eq!(parse_lines("  A=1 \n\n# comment\r\nB=2\n"), vec!["A=1", "B=2"]);
    assert_eq!(split_command("  ").unwrap(), Vec::<String>::new());
    assert_eq!(split_command(r#"a 'b c' "" d"#).unwrap(), vec!["a", "b c", "", "d"]);
}

#[tokio::test]
async fn test_mock_run_container() {
    let client = MockWslcClient::new();
    let before = client.list_containers(true).await.unwrap().len();

    let s = ContainerSpec {
        name: "cache".into(),
        ports: vec!["6379:6379".into()],
        ..spec("redis:7")
    };
    let id = client.run_container(&s).await.expect("run failed");

    let containers = client.list_containers(true).await.unwrap();
    assert_eq!(containers.len(), before + 1);
    let c = containers.iter().find(|c| c.id == id).unwrap();
    assert_eq!(c.primary_name(), "cache");
    assert_eq!(c.state, ContainerState::Running);
    assert_eq!(c.ports[0].host_port, 6379);

    // Missing image was pulled
    let images = client.list_images().await.unwrap();
    assert!(images.iter().any(|i| i.repository == "redis" && i.tag == "7"));

    // Same name again conflicts; create-only leaves it stopped
    assert!(client.run_container(&s).await.is_err());
    let created = ContainerSpec { start: false, ..spec("redis:7") };
    let id = client.run_container(&created).await.unwrap();
    let containers = client.list_containers(true).await.unwrap();
    assert_eq!(containers.iter().find(|c| c.id == id).unwrap().state, ContainerState::Created);
}
