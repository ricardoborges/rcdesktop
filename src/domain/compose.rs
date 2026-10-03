//! A pragmatic Docker Compose implementation on top of plain wslc commands.
//!
//! wslc has no `compose` subcommand, so a compose file is translated into
//! networks, volumes and one `wslc run` per service, following Compose's
//! naming (`<project>_default`, `<project>_<volume>`, `<project>-<service>-1`)
//! and labels, so the containers show up grouped as a stack.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde_yaml::{Mapping, Value};

use crate::domain::deploy::{quote_args, split_command, ContainerSpec};

pub const PROJECT_LABEL: &str = "com.docker.compose.project";
pub const SERVICE_LABEL: &str = "com.docker.compose.service";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComposeProject {
    pub name: String,
    /// Networks to create (full names)
    pub networks: Vec<String>,
    /// Named volumes to create (full names)
    pub volumes: Vec<String>,
    /// Services in dependency order
    pub services: Vec<ComposeService>,
    /// Things that were ignored or approximated
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposeService {
    pub name: String,
    pub spec: ContainerSpec,
}

impl ComposeProject {
    /// Labels for networks and volumes owned by this project.
    pub fn labels(&self) -> Vec<String> {
        vec![format!("{}={}", PROJECT_LABEL, self.name)]
    }

    /// The equivalent wslc commands, one per line, plus warnings.
    pub fn plan(&self) -> String {
        let mut lines = vec![format!("# Stack: {}", self.name)];
        for n in &self.networks {
            lines.push(format!("$ wslc network create {}", n));
        }
        for v in &self.volumes {
            lines.push(format!("$ wslc volume create {}", v));
        }
        for s in &self.services {
            // Blank line first: long run commands wrap and would blend together
            lines.push(format!("
# service {}
$ {}", s.name, s.spec.command_line()));
        }
        if !self.warnings.is_empty() {
            lines.push(String::new());
            lines.extend(self.warnings.iter().map(|w| format!("⚠ {}", w)));
        }
        lines.join("\n")
    }
}

/// Lowercase letters, digits, `_` and `-`, starting with a letter or digit;
/// the same rule Compose applies to project names.
pub fn normalize_project_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .skip_while(|c| !c.is_ascii_alphanumeric())
        .collect()
}

/// Variables available for `${VAR}` interpolation: the `.env` file next to the
/// compose file, overridden by the process environment (as Compose does).
pub fn load_env(base_dir: Option<&Path>) -> HashMap<String, String> {
    let mut env = HashMap::new();
    if let Some(text) = base_dir.and_then(|d| std::fs::read_to_string(d.join(".env")).ok()) {
        env.extend(parse_env_file(&text));
    }
    env.extend(std::env::vars());
    env
}

/// `KEY=value` lines; blanks and `#` comments are skipped, quotes stripped.
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let l = l.strip_prefix("export ").unwrap_or(l);
            let (k, v) = l.split_once('=')?;
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(v);
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

/// Expands `$VAR`, `${VAR}`, `${VAR:-default}`, `${VAR-default}`,
/// `${VAR:?error}`, `${VAR?error}` and `$$`. Unset variables become empty
/// and are reported as warnings.
pub fn interpolate(text: &str, env: &HashMap<String, String>) -> Result<(String, Vec<String>), String> {
    let mut out = String::with_capacity(text.len());
    let mut warnings = Vec::new();
    let mut chars = text.chars().peekable();
    let is_name = |c: char| c.is_ascii_alphanumeric() || c == '_';

    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                chars.next();
                out.push('$');
            }
            Some('{') => {
                chars.next();
                let mut expr = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => expr.push(c),
                        None => return Err(format!("Unclosed variable '${{{}'", expr)),
                    }
                }
                let name_len = expr.find(|c: char| !is_name(c)).unwrap_or(expr.len());
                let (name, modifier) = expr.split_at(name_len);
                if name.is_empty() {
                    return Err(format!("Invalid variable '${{{}}}'", expr));
                }
                let value = env.get(name);
                let set_and_non_empty = value.is_some_and(|v| !v.is_empty());
                let resolved = if let Some(default) = modifier.strip_prefix(":-") {
                    if set_and_non_empty { value.cloned() } else { Some(default.to_string()) }
                } else if let Some(default) = modifier.strip_prefix('-') {
                    Some(value.cloned().unwrap_or_else(|| default.to_string()))
                } else if let Some(msg) = modifier.strip_prefix(":?") {
                    if !set_and_non_empty {
                        return Err(format!("Required variable {} is missing: {}", name, msg));
                    }
                    value.cloned()
                } else if let Some(msg) = modifier.strip_prefix('?') {
                    if value.is_none() {
                        return Err(format!("Required variable {} is missing: {}", name, msg));
                    }
                    value.cloned()
                } else if modifier.is_empty() {
                    value.cloned()
                } else {
                    return Err(format!("Unsupported variable syntax '${{{}}}'", expr));
                };
                out.push_str(&resolved.unwrap_or_else(|| {
                    warnings.push(format!("Variable {} is not set; using an empty string", name));
                    String::new()
                }));
            }
            Some(&n) if n.is_ascii_alphabetic() || n == '_' => {
                let mut name = String::new();
                while let Some(&n) = chars.peek() {
                    if !is_name(n) {
                        break;
                    }
                    name.push(n);
                    chars.next();
                }
                out.push_str(&env.get(&name).cloned().unwrap_or_else(|| {
                    warnings.push(format!("Variable {} is not set; using an empty string", name));
                    String::new()
                }));
            }
            _ => out.push('$'),
        }
    }
    warnings.dedup();
    Ok((out, warnings))
}

/// Parses a compose file into a deployable project.
///
/// `project` overrides the file's top-level `name:`. `base_dir` is the compose
/// file's directory; relative bind mounts and `env_file`s need it.
pub fn parse_compose(
    yaml: &str,
    project: &str,
    base_dir: Option<&Path>,
    env: &HashMap<String, String>,
) -> Result<ComposeProject, String> {
    let (text, mut warnings) = interpolate(yaml, env)?;
    let root: Value = serde_yaml::from_str(&text).map_err(|e| format!("Invalid YAML: {}", e))?;
    let root = root.as_mapping().ok_or("The compose file must be a YAML mapping")?;

    // Same precedence as Compose: explicit name, then `name:`, then the directory
    let name = if !project.trim().is_empty() {
        project.to_string()
    } else if let Some(n) = root.get("name").and_then(scalar) {
        n
    } else {
        base_dir
            .and_then(|d| d.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let name = normalize_project_name(&name);
    if name.is_empty() {
        return Err("Stack name is required (set it above or with a top-level `name:`)".into());
    }

    for key in root.keys().filter_map(Value::as_str) {
        match key {
            "services" | "networks" | "volumes" | "name" | "version" => {}
            k if k.starts_with("x-") => {}
            other => warnings.push(format!("Top-level '{}' is not supported and was ignored", other)),
        }
    }

    let services = root
        .get("services")
        .and_then(Value::as_mapping)
        .filter(|m| !m.is_empty())
        .ok_or("The compose file has no services")?;

    // Top-level networks/volumes: key -> (full name, created by us)
    let declared = |section: &str| -> Result<Vec<(String, String, bool)>, String> {
        let mut out = Vec::new();
        for (key, def) in root.get(section).and_then(Value::as_mapping).into_iter().flatten() {
            let key = scalar(key).ok_or(format!("Invalid {} name", section))?;
            let external = def.get("external").is_some_and(|e| e.as_bool() == Some(true) || e.is_mapping());
            let full = def
                .get("name")
                .and_then(scalar)
                .unwrap_or_else(|| if external { key.clone() } else { format!("{}_{}", name, key) });
            out.push((key, full, !external));
        }
        Ok(out)
    };
    let mut networks = declared("networks")?;
    if !networks.iter().any(|(k, _, _)| k == "default") {
        networks.push(("default".into(), format!("{}_default", name), true));
    }
    let volumes = declared("volumes")?;

    let mut used_networks = BTreeSet::new();
    let mut used_volumes = BTreeSet::new();
    let mut parsed = Vec::new();
    for (svc_name, def) in services {
        let svc_name = scalar(svc_name).ok_or("Invalid service name")?;
        let def = def.as_mapping().ok_or(format!("Service '{}' must be a mapping", svc_name))?;
        if def.get("profiles").is_some() {
            warnings.push(format!("Service '{}' has profiles and was skipped", svc_name));
            continue;
        }
        let ctx = ServiceContext {
            project: &name,
            service: &svc_name,
            base_dir,
            networks: &networks,
            volumes: &volumes,
        };
        let (spec, deps) = ctx.parse(def, &mut warnings, &mut used_networks, &mut used_volumes)?;
        parsed.push((ComposeService { name: svc_name, spec }, deps));
    }

    let services = sort_by_dependencies(parsed)?;
    Ok(ComposeProject {
        networks: networks
            .into_iter()
            .filter(|(k, _, create)| *create && used_networks.contains(k))
            .map(|(_, full, _)| full)
            .collect(),
        volumes: volumes
            .into_iter()
            .filter(|(k, _, create)| *create && used_volumes.contains(k))
            .map(|(_, full, _)| full)
            .collect(),
        name,
        services,
        warnings,
    })
}

struct ServiceContext<'a> {
    project: &'a str,
    service: &'a str,
    base_dir: Option<&'a Path>,
    networks: &'a [(String, String, bool)],
    volumes: &'a [(String, String, bool)],
}

impl ServiceContext<'_> {
    fn err(&self, msg: impl std::fmt::Display) -> String {
        format!("Service '{}': {}", self.service, msg)
    }

    fn parse(
        &self,
        def: &Mapping,
        warnings: &mut Vec<String>,
        used_networks: &mut BTreeSet<String>,
        used_volumes: &mut BTreeSet<String>,
    ) -> Result<(ContainerSpec, Vec<String>), String> {
        let mut spec = ContainerSpec {
            start: true,
            ..Default::default()
        };
        let mut deps = Vec::new();
        let mut env: Vec<(String, String)> = Vec::new();
        let mut entrypoint_args: Vec<String> = Vec::new();
        let mut command: Option<String> = None;
        let mut network_mode: Option<String> = None;
        let mut service_networks: Vec<(String, Vec<String>)> = Vec::new();

        for (key, value) in def {
            let key = key.as_str().unwrap_or_default();
            match key {
                "image" => spec.image = scalar(value).ok_or_else(|| self.err("invalid image"))?,
                "build" => {}
                "container_name" => spec.name = scalar(value).unwrap_or_default(),
                "command" => command = Some(self.command_string(value)?),
                "entrypoint" => {
                    let mut parts = match value {
                        Value::Sequence(_) => string_list(value),
                        _ => split_command(&scalar(value).unwrap_or_default()).map_err(|e| self.err(e))?,
                    };
                    if !parts.is_empty() {
                        spec.entrypoint = parts.remove(0);
                        entrypoint_args = parts;
                    }
                }
                "environment" => match value {
                    Value::Mapping(m) => {
                        for (k, v) in m {
                            let k = scalar(k).unwrap_or_default();
                            match scalar(v) {
                                Some(v) => set_env(&mut env, k, v),
                                None => self.env_from_host(&mut env, k),
                            }
                        }
                    }
                    _ => {
                        for item in string_list(value) {
                            match item.split_once('=') {
                                Some((k, v)) => set_env(&mut env, k.into(), v.into()),
                                None => self.env_from_host(&mut env, item),
                            }
                        }
                    }
                },
                "env_file" => {
                    // env_file comes first; `environment` overrides it
                    let files: Vec<String> = match value {
                        Value::Sequence(items) => items
                            .iter()
                            .filter_map(|i| scalar(i).or_else(|| i.get("path").and_then(scalar)))
                            .collect(),
                        _ => scalar(value).into_iter().collect(),
                    };
                    let mut from_files = Vec::new();
                    for f in files {
                        let path = self.resolve_path(&f)?;
                        let text = std::fs::read_to_string(&path)
                            .map_err(|e| self.err(format!("cannot read env_file {}: {}", path.display(), e)))?;
                        for (k, v) in parse_env_file(&text) {
                            set_env(&mut from_files, k, v);
                        }
                    }
                    for (k, v) in std::mem::take(&mut env) {
                        set_env(&mut from_files, k, v);
                    }
                    env = from_files;
                }
                "ports" => {
                    for p in value.as_sequence().into_iter().flatten() {
                        spec.ports.push(self.port(p)?);
                    }
                }
                "volumes" => {
                    for v in value.as_sequence().into_iter().flatten() {
                        if let Some(v) = self.volume(v, warnings, used_volumes)? {
                            spec.volumes.push(v);
                        }
                    }
                }
                "networks" => match value {
                    Value::Mapping(m) => {
                        for (k, v) in m {
                            let aliases = v.get("aliases").map(string_list).unwrap_or_default();
                            service_networks.push((scalar(k).unwrap_or_default(), aliases));
                        }
                    }
                    _ => service_networks.extend(string_list(value).into_iter().map(|n| (n, vec![]))),
                },
                "network_mode" => network_mode = scalar(value),
                "depends_on" => {
                    deps = match value {
                        Value::Mapping(m) => m.keys().filter_map(scalar).collect(),
                        _ => string_list(value),
                    }
                }
                "labels" => match value {
                    Value::Mapping(m) => {
                        for (k, v) in m {
                            spec.labels.push(format!(
                                "{}={}",
                                scalar(k).unwrap_or_default(),
                                scalar(v).unwrap_or_default()
                            ));
                        }
                    }
                    _ => spec.labels.extend(string_list(value)),
                },
                "working_dir" => spec.workdir = scalar(value).unwrap_or_default(),
                "hostname" => spec.hostname = scalar(value).unwrap_or_default(),
                "user" => spec.user = scalar(value).unwrap_or_default(),
                "tty" | "stdin_open" => spec.tty |= value.as_bool() == Some(true),
                "cpus" => spec.cpus = scalar(value).unwrap_or_default(),
                "mem_limit" => spec.memory = scalar(value).unwrap_or_default(),
                "deploy" => {
                    let limits = value.get("resources").and_then(|r| r.get("limits"));
                    if let Some(cpus) = limits.and_then(|l| l.get("cpus")).and_then(scalar) {
                        spec.cpus = cpus;
                    }
                    if let Some(mem) = limits.and_then(|l| l.get("memory")).and_then(scalar) {
                        spec.memory = mem;
                    }
                }
                "pull_policy" => spec.pull_always = scalar(value).as_deref() == Some("always"),
                "restart" => warnings.push(format!(
                    "Service '{}': restart policies are not supported by wslc and were ignored",
                    self.service
                )),
                "expose" => {}
                other => warnings.push(format!(
                    "Service '{}': '{}' is not supported and was ignored",
                    self.service, other
                )),
            }
        }

        if spec.image.is_empty() {
            return Err(self.err(if def.get("build").is_some() {
                "`build` is not supported; build the image first (wslc build) and set `image`"
            } else {
                "`image` is required"
            }));
        }
        if def.get("build").is_some() {
            warnings.push(format!("Service '{}': `build` was ignored; using image {}", self.service, spec.image));
        }
        if spec.name.is_empty() {
            spec.name = format!("{}-{}-1", self.project, self.service);
        }

        spec.env = env.into_iter().map(|(k, v)| format!("{}={}", k, v)).collect();

        let mut cmd_args = entrypoint_args;
        if let Some(c) = command {
            cmd_args.extend(split_command(&c).map_err(|e| self.err(e))?);
        }
        spec.command = quote_args(&cmd_args);

        match network_mode.as_deref() {
            Some(mode @ ("host" | "none" | "bridge")) => spec.network = mode.into(),
            Some(other) => {
                return Err(self.err(format!("network_mode '{}' is not supported", other)));
            }
            None => {
                if service_networks.is_empty() {
                    service_networks.push(("default".into(), vec![]));
                }
                let (first, aliases) = &service_networks[0];
                let full = self
                    .networks
                    .iter()
                    .find(|(k, _, _)| k == first)
                    .map(|(_, full, _)| full.clone())
                    .ok_or_else(|| self.err(format!("network '{}' is not declared", first)))?;
                used_networks.insert(first.clone());
                spec.network = full;
                spec.network_aliases.push(self.service.to_string());
                spec.network_aliases.extend(aliases.iter().cloned());
                if service_networks.len() > 1 {
                    warnings.push(format!(
                        "Service '{}': only the first network ({}) is attached; wslc run takes one network",
                        self.service, first
                    ));
                }
            }
        }

        spec.labels.push(format!("{}={}", PROJECT_LABEL, self.project));
        spec.labels.push(format!("{}={}", SERVICE_LABEL, self.service));
        spec.validate().map_err(|e| self.err(e))?;
        Ok((spec, deps))
    }

    fn command_string(&self, value: &Value) -> Result<String, String> {
        match value {
            Value::Sequence(_) => Ok(quote_args(&string_list(value))),
            Value::Null => Ok(String::new()),
            _ => scalar(value).ok_or_else(|| self.err("invalid command")),
        }
    }

    fn env_from_host(&self, env: &mut Vec<(String, String)>, key: String) {
        if let Ok(v) = std::env::var(&key) {
            set_env(env, key, v);
        }
    }

    fn port(&self, value: &Value) -> Result<String, String> {
        if let Some(s) = scalar(value) {
            return Ok(s);
        }
        let target = value
            .get("target")
            .and_then(scalar)
            .ok_or_else(|| self.err("port mapping needs a `target`"))?;
        let mut out = String::new();
        if let Some(ip) = value.get("host_ip").and_then(scalar) {
            out.push_str(&ip);
            out.push(':');
        }
        if let Some(published) = value.get("published").and_then(scalar) {
            out.push_str(&published);
            out.push(':');
        } else if !out.is_empty() {
            out.push(':');
        }
        out.push_str(&target);
        if let Some(proto) = value.get("protocol").and_then(scalar) {
            out.push('/');
            out.push_str(&proto);
        }
        Ok(out)
    }

    /// Converts a volume entry into `-v` syntax, resolving named volumes and
    /// relative bind paths. Returns `None` for entries that are skipped.
    fn volume(
        &self,
        value: &Value,
        warnings: &mut Vec<String>,
        used_volumes: &mut BTreeSet<String>,
    ) -> Result<Option<String>, String> {
        let (source, target, read_only, mode) = if let Some(s) = scalar(value) {
            let (source, rest) = split_volume_source(&s);
            match (source, rest.split_once(':')) {
                (None, _) => (None, rest.to_string(), false, None),
                (Some(src), Some((target, mode))) => (Some(src), target.to_string(), false, Some(mode.to_string())),
                (Some(src), None) => (Some(src), rest.to_string(), false, None),
            }
        } else {
            let kind = value.get("type").and_then(scalar).unwrap_or_else(|| "volume".into());
            if kind != "volume" && kind != "bind" {
                warnings.push(format!(
                    "Service '{}': {} mounts are not supported and were skipped",
                    self.service, kind
                ));
                return Ok(None);
            }
            let target = value
                .get("target")
                .and_then(scalar)
                .ok_or_else(|| self.err("volume needs a `target`"))?;
            let ro = value.get("read_only").and_then(Value::as_bool).unwrap_or(false);
            (value.get("source").and_then(scalar), target, ro, None)
        };

        let source = match source {
            None => None,
            Some(src) if is_path(&src) => Some(self.resolve_path(&src)?.display().to_string()),
            Some(src) => {
                let full = self
                    .volumes
                    .iter()
                    .find(|(k, _, _)| *k == src)
                    .map(|(_, full, _)| full.clone())
                    .ok_or_else(|| self.err(format!("volume '{}' is not declared under top-level `volumes`", src)))?;
                used_volumes.insert(src);
                Some(full)
            }
        };

        let mut out = match source {
            Some(src) => format!("{}:{}", src, target),
            None => target,
        };
        match (mode, read_only) {
            (Some(mode), _) => out = format!("{}:{}", out, mode),
            (None, true) => out.push_str(":ro"),
            _ => {}
        }
        Ok(Some(out))
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf, String> {
        if let Some(rest) = path.strip_prefix('~') {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map_err(|_| self.err("cannot resolve ~: no home directory"))?;
            return Ok(PathBuf::from(home).join(rest.trim_start_matches(['/', '\\'])));
        }
        let p = Path::new(path);
        if p.is_absolute() || path.starts_with('/') {
            return Ok(p.to_path_buf());
        }
        let base = self.base_dir.ok_or_else(|| {
            self.err(format!(
                "relative path '{}' needs the compose file to be opened from disk",
                path
            ))
        })?;
        let joined = base.join(path.trim_start_matches("./").trim_start_matches(".\\"));
        Ok(joined)
    }
}

/// Splits `source:target[:mode]` into the source (if any) and the rest,
/// keeping Windows drive letters (`C:\data:/data`) intact.
fn split_volume_source(s: &str) -> (Option<String>, &str) {
    let b = s.as_bytes();
    let skip = if b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/') {
        2
    } else {
        0
    };
    match s[skip..].find(':') {
        Some(i) => (Some(s[..skip + i].to_string()), &s[skip + i + 1..]),
        None => (None, s),
    }
}

fn is_path(s: &str) -> bool {
    let b = s.as_bytes();
    s.starts_with(['.', '/', '\\', '~']) || (b.len() > 1 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

fn set_env(env: &mut Vec<(String, String)>, key: String, value: String) {
    match env.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => env.push((key, value)),
    }
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn string_list(v: &Value) -> Vec<String> {
    match v {
        Value::Sequence(items) => items.iter().filter_map(scalar).collect(),
        other => scalar(other).into_iter().collect(),
    }
}

/// Orders services so dependencies start first; keeps file order otherwise.
fn sort_by_dependencies(mut pending: Vec<(ComposeService, Vec<String>)>) -> Result<Vec<ComposeService>, String> {
    let names: BTreeSet<String> = pending.iter().map(|(s, _)| s.name.clone()).collect();
    for (svc, deps) in &pending {
        if let Some(d) = deps.iter().find(|d| !names.contains(*d)) {
            return Err(format!("Service '{}' depends on unknown service '{}'", svc.name, d));
        }
    }

    let mut done = BTreeSet::new();
    let mut ordered = Vec::new();
    while !pending.is_empty() {
        let Some(i) = pending.iter().position(|(_, deps)| deps.iter().all(|d| done.contains(d))) else {
            let names: Vec<_> = pending.iter().map(|(s, _)| s.name.as_str()).collect();
            return Err(format!("Circular depends_on between: {}", names.join(", ")));
        };
        let (svc, _) = pending.remove(i);
        done.insert(svc.name.clone());
        ordered.push(svc);
    }
    Ok(ordered)
}
