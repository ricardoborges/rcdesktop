use rcdesktop::domain::container::ContainerState;
use rcdesktop::wslc::parser::{parse_containers, parse_images, parse_version, parse_volumes};

#[test]
fn test_parse_containers_json() {
    let json_output = r#"[
        {
            "Id": "c1a2b3c4d5e6",
            "Names": ["/web-server"],
            "Image": "nginx:alpine",
            "Command": "nginx -g 'daemon off;'",
            "Created": 1710000000,
            "State": "running",
            "Status": "Up 2 hours",
            "Ports": [{"PublicPort": 8080, "PrivatePort": 80, "Type": "tcp", "IP": "0.0.0.0"}]
        }
    ]"#;
    let list = parse_containers(json_output);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "c1a2b3c4d5e6");
    assert_eq!(list[0].primary_name(), "web-server");
    assert_eq!(list[0].state, ContainerState::Running);
    assert_eq!(list[0].ports.len(), 1);
    assert_eq!(list[0].ports[0].host_port, 8080);
}

#[test]
fn test_parse_containers_compose_project() {
    let json_output = r#"[
        {"Id": "a1", "Names": ["/web"], "Image": "nginx", "State": "running",
         "Labels": {"com.docker.compose.project": "shop", "com.docker.compose.service": "web"}},
        {"Id": "b2", "Names": ["/api"], "Image": "python", "State": "running",
         "Labels": "com.docker.compose.service=api,com.docker.compose.project=shop"},
        {"Id": "c3", "Names": ["/solo"], "Image": "redis", "State": "exited", "Labels": ""}
    ]"#;
    let list = parse_containers(json_output);
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].compose_project.as_deref(), Some("shop"));
    assert_eq!(list[1].compose_project.as_deref(), Some("shop"));
    assert_eq!(list[2].compose_project, None);
}

#[test]
fn test_parse_containers_tabular() {
    let tabular_output = "CONTAINER ID   IMAGE          COMMAND                  CREATED         STATUS         PORTS                  NAMES\n\
c1a2b3c4d5e6   nginx:alpine   \"nginx -g 'daemon of…\"   2 hours ago     Up 2 hours     0.0.0.0:8080->80/tcp   web-server\n\
f9e8d7c6b5a4   redis:7        \"docker-entrypoint.s…\"   5 hours ago     Exited (0)     6379/tcp               cache-db\n";
    let list = parse_containers(tabular_output);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, "c1a2b3c4d5e6");
    assert_eq!(list[0].image, "nginx:alpine");
    assert_eq!(list[0].primary_name(), "web-server");
    assert_eq!(list[0].state, ContainerState::Running);
    assert_eq!(list[1].id, "f9e8d7c6b5a4");
    assert_eq!(list[1].state, ContainerState::Exited(0));
}

#[test]
fn test_parse_version() {
    let out = "wslc version 5.0.1.1\ncommit: deadbeef\n";
    assert_eq!(parse_version(out), Some("5.0.1.1".to_string()));
}

#[test]
fn test_parse_images_tabular() {
    let out = "REPOSITORY   TAG       IMAGE ID       CREATED        SIZE\n\
nginx        alpine    9a8b7c6d5e4f   3 days ago     42.5MB\n\
redis        latest    1a2b3c4d5e6f   2 weeks ago    117MB\n";
    let images = parse_images(out);
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].repository, "nginx");
    assert_eq!(images[0].tag, "alpine");
    assert_eq!(images[0].size, "42.5MB");
}

#[test]
fn test_parse_volumes_tabular() {
    let out = "DRIVER    VOLUME NAME\n\
local     pgdata_volume\n\
local     nginx_cache\n";
    let vols = parse_volumes(out);
    assert_eq!(vols.len(), 2);
    assert_eq!(vols[0].name, "pgdata_volume");
    assert_eq!(vols[1].name, "nginx_cache");
}

#[test]
fn test_parse_containers_json_lines() {
    let raw = r#"{"ID":"5e87b3799438","Image":"delphos-frontend","Labels":"com.docker.compose.project=delphos,com.docker.compose.service=frontend","Names":"delphos-frontend-1","Ports":"127.0.0.1:9090->80/tcp","State":"running","Status":"Up 2 minutes"}
{"ID":"51e86e783886","Image":"delphos-backend","Labels":"com.docker.compose.project=delphos","Names":"delphos-backend-1","Ports":"","State":"exited","Status":"Exited (143)"}"#;
    let list = rcdesktop::wslc::parser::parse_containers(raw);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].names, vec!["delphos-frontend-1".to_string()]);
    assert_eq!(list[0].compose_project.as_deref(), Some("delphos"));
    assert_eq!(list[0].ports.len(), 1);
    assert_eq!(list[0].ports[0].host_port, 9090);
    assert_eq!(list[1].compose_project.as_deref(), Some("delphos"));
    assert!(list[1].ports.is_empty());
}
