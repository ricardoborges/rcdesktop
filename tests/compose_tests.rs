use std::collections::HashMap;
use std::path::Path;

use rcdesktop::domain::compose::{interpolate, normalize_project_name, parse_compose};
use rcdesktop::domain::deploy::parse_run_command;
use rcdesktop::wslc::client::WslcClient;
use rcdesktop::wslc::mock::MockWslcClient;
use rcdesktop::wslc::stack::deploy_project;

const WORDPRESS: &str = r#"
name: blog
services:
  wordpress:
    image: wordpress:6
    depends_on: [db]
    ports:
      - "8080:80"
      - target: 443
        published: 8443
        protocol: tcp
    environment:
      WORDPRESS_DB_HOST: db
      WORDPRESS_DB_PASSWORD: ${DB_PASSWORD:-changeme}
    volumes:
      - ./html:/var/www/html
    restart: always
  db:
    image: mariadb:11
    command: ["--max-connections", "200"]
    environment:
      - MARIADB_ROOT_PASSWORD=${DB_PASSWORD:-changeme}
    volumes:
      - dbdata:/var/lib/mysql
    deploy:
      resources:
        limits:
          cpus: "1.5"
          memory: 512M
volumes:
  dbdata:
"#;

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn test_parse_wordpress_stack() {
    let base = Path::new(r"C:\stacks\blog");
    let p = parse_compose(WORDPRESS, "", Some(base), &env(&[])).expect("parse failed");

    assert_eq!(p.name, "blog");
    assert_eq!(p.networks, vec!["blog_default"]);
    assert_eq!(p.volumes, vec!["blog_dbdata"]);
    // db first because wordpress depends on it
    let names: Vec<_> = p.services.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["db", "wordpress"]);

    let db = &p.services[0].spec;
    assert_eq!(db.name, "blog-db-1");
    assert_eq!(db.network, "blog_default");
    assert_eq!(db.network_aliases, vec!["db"]);
    assert_eq!(db.volumes, vec!["blog_dbdata:/var/lib/mysql"]);
    assert_eq!(db.env, vec!["MARIADB_ROOT_PASSWORD=changeme"]);
    assert_eq!(db.command, "--max-connections 200");
    assert_eq!((db.cpus.as_str(), db.memory.as_str()), ("1.5", "512M"));
    assert!(db.labels.contains(&"com.docker.compose.project=blog".to_string()));
    assert!(db.labels.contains(&"com.docker.compose.service=db".to_string()));

    let wp = &p.services[1].spec;
    assert_eq!(wp.ports, vec!["8080:80", "8443:443/tcp"]);
    assert_eq!(wp.volumes, vec![r"C:\stacks\blog\html:/var/www/html"]);
    assert!(wp.env.contains(&"WORDPRESS_DB_HOST=db".to_string()));
    assert!(p.warnings.iter().any(|w| w.contains("restart")));
    assert!(p.plan().contains("wslc network create blog_default"));
}

#[test]
fn test_project_name_override_and_interpolation() {
    let p = parse_compose(WORDPRESS, "My Blog!", Some(Path::new("C:\\x")), &env(&[("DB_PASSWORD", "s3cret")])).unwrap();
    assert_eq!(p.name, "myblog");
    assert_eq!(p.services[0].spec.env, vec!["MARIADB_ROOT_PASSWORD=s3cret"]);
    assert_eq!(p.services[0].spec.name, "myblog-db-1");
}

#[test]
fn test_compose_errors() {
    let e = |yaml: &str| parse_compose(yaml, "p", None, &env(&[])).unwrap_err();
    assert!(e("services: {}").contains("no services"));
    assert!(e("services:\n  a:\n    build: .\n").contains("build"));
    assert!(e("services:\n  a:\n    image: x\n    volumes: [data:/d]\n").contains("not declared"));
    assert!(e("services:\n  a:\n    image: x\n    volumes: [./d:/d]\n").contains("opened from disk"));
    assert!(e("services:\n  a:\n    image: x\n    depends_on: [b]\n").contains("unknown service"));
    assert!(e("services:\n  a:\n    image: x\n    depends_on: [b]\n  b:\n    image: y\n    depends_on: [a]\n")
        .contains("Circular"));
    assert!(e("services: [\n").contains("Invalid YAML"));
    assert!(parse_compose("services:\n  a:\n    image: x\n", "", None, &env(&[])).unwrap_err().contains("Stack name"));
}

#[test]
fn test_external_and_custom_networks() {
    let yaml = r#"
services:
  api:
    image: api
    container_name: my-api
    network_mode: host
  web:
    image: nginx
    networks:
      front:
        aliases: [www]
networks:
  front:
  proxy:
    external: true
"#;
    let p = parse_compose(yaml, "shop", None, &env(&[])).unwrap();
    assert_eq!(p.networks, vec!["shop_front"]);
    assert_eq!(p.services[0].spec.name, "my-api");
    assert_eq!(p.services[0].spec.network, "host");
    assert_eq!(p.services[1].spec.network, "shop_front");
    assert_eq!(p.services[1].spec.network_aliases, vec!["web", "www"]);
}

#[test]
fn test_interpolate() {
    let vars = env(&[("A", "1"), ("EMPTY", "")]);
    let (out, warnings) = interpolate("$A ${A} ${EMPTY:-d} ${EMPTY-d} ${NOPE:-x} $$A $NOPE", &vars).unwrap();
    assert_eq!(out, "1 1 d  x $A ");
    assert_eq!(warnings.len(), 1);
    assert!(interpolate("${NOPE:?must be set}", &vars).unwrap_err().contains("must be set"));
    assert!(interpolate("${A", &vars).is_err());
}

#[test]
fn test_normalize_project_name() {
    assert_eq!(normalize_project_name("  _My-App_2 "), "my-app_2");
    assert_eq!(normalize_project_name("!!!"), "");
}

#[test]
fn test_parse_run_command() {
    let (spec, warnings) = parse_run_command(
        "docker run -d --name web \\\n  -p 8080:80 -e TZ=UTC --restart=always \\\n  -v data:/data -it nginx:alpine nginx -g 'daemon off;'",
    )
    .unwrap();
    assert!(spec.start);
    assert!(spec.tty);
    assert_eq!(spec.name, "web");
    assert_eq!(spec.ports, vec!["8080:80"]);
    assert_eq!(spec.env, vec!["TZ=UTC"]);
    assert_eq!(spec.volumes, vec!["data:/data"]);
    assert_eq!(spec.image, "nginx:alpine");
    assert_eq!(spec.command, "nginx -g \"daemon off;\"");
    assert_eq!(warnings.len(), 1);

    let (spec, _) = parse_run_command("docker container create --rm --memory=1g redis").unwrap();
    assert!(!spec.start && spec.auto_remove);
    assert_eq!(spec.memory, "1g");

    assert!(parse_run_command("docker run --privileged nginx").unwrap_err().contains("--privileged"));
    assert!(parse_run_command("docker run -d").unwrap_err().contains("No image"));
    assert!(parse_run_command("docker run --name").is_err());
}

#[tokio::test]
async fn test_deploy_project_on_mock() {
    let client = MockWslcClient::new();
    let p = parse_compose(WORDPRESS, "", Some(Path::new(r"C:\stacks\blog")), &env(&[])).unwrap();

    let log = std::sync::Mutex::new(Vec::new());
    deploy_project(&client, &p, |l| log.lock().unwrap().push(l)).await.expect("deploy failed");

    let containers = client.list_containers(true).await.unwrap();
    let stack: Vec<_> = containers.iter().filter(|c| c.compose_project.as_deref() == Some("blog")).collect();
    assert_eq!(stack.len(), 2);
    assert!(client.list_networks().await.unwrap().iter().any(|n| n.name == "blog_default"));
    assert!(client.list_volumes().await.unwrap().iter().any(|v| v.name == "blog_dbdata"));

    // Deploying again recreates instead of failing on the name conflict
    deploy_project(&client, &p, |l| log.lock().unwrap().push(l)).await.expect("redeploy failed");
    let containers = client.list_containers(true).await.unwrap();
    assert_eq!(containers.iter().filter(|c| c.compose_project.as_deref() == Some("blog")).count(), 2);
    assert!(log.lock().unwrap().iter().any(|l| l.starts_with("Recreating")));
}

#[test]
fn test_parse_networks_json_lines() {
    use rcdesktop::wslc::parser::parse_networks;
    let raw = concat!(
        r#"{"Driver":"bridge","ID":"6442a99f3cb3","Labels":"","Name":"bridge","Scope":"local"}"#, "\n",
        r#"{"Driver":"bridge","ID":"c757857301f1","Labels":"com.docker.compose.network=default,com.docker.compose.project=delphos","Name":"delphos_default","Scope":"local"}"#, "\n",
    );
    let nets = parse_networks(raw);
    assert_eq!(nets.len(), 2);
    assert_eq!(nets[0].compose_project, None);
    assert_eq!(nets[1].name, "delphos_default");
    assert_eq!(nets[1].compose_project.as_deref(), Some("delphos"));

    let table = parse_networks("NETWORK ID     NAME    DRIVER    SCOPE\n9b710bb9190a   bridge  bridge    local\n");
    assert_eq!(table[0].name, "bridge");
}

#[tokio::test]
async fn test_remove_project_on_mock() {
    use rcdesktop::wslc::stack::remove_project;
    let client = MockWslcClient::new();
    let p = parse_compose(WORDPRESS, "", Some(Path::new(r"C:\stacks\blog")), &env(&[])).unwrap();
    deploy_project(&client, &p, |_| {}).await.unwrap();

    let removed = remove_project(&client, "blog", |_| {}).await.expect("remove failed");
    assert_eq!(removed, 2);
    let containers = client.list_containers(true).await.unwrap();
    assert!(containers.iter().all(|c| c.compose_project.as_deref() != Some("blog")));
    // Other stacks are untouched
    assert!(containers.iter().any(|c| c.compose_project.as_deref() == Some("my-stack")));
    assert!(!client.list_networks().await.unwrap().iter().any(|n| n.name == "blog_default"));
    // Data survives, like `compose down` without -v
    assert!(client.list_volumes().await.unwrap().iter().any(|v| v.name == "blog_dbdata"));
}
