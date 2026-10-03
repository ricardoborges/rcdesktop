/// Everything needed to create a new container, roughly what `docker run` takes.
/// Built from the deploy form and turned into `wslc run`/`wslc create` arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerSpec {
    pub name: String,
    pub image: String,
    pub command: String,
    /// `[ip:]host:container[/proto]` or just `container[/proto]`
    pub ports: Vec<String>,
    /// `KEY=value`
    pub env: Vec<String>,
    /// `source:target[:options]` or an anonymous `target`
    pub volumes: Vec<String>,
    pub network: String,
    /// Extra DNS names on `network` (compose uses the service name)
    pub network_aliases: Vec<String>,
    /// `key=value`
    pub labels: Vec<String>,
    pub entrypoint: String,
    pub workdir: String,
    pub hostname: String,
    pub user: String,
    pub cpus: String,
    pub memory: String,
    pub pull_always: bool,
    pub auto_remove: bool,
    /// Keep stdin open and allocate a TTY (`-i -t`), so shells don't exit right away
    pub tty: bool,
    /// Start the container after creating it (`run -d`); otherwise only `create`
    pub start: bool,
}

impl ContainerSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.image.trim().is_empty() {
            return Err("Image is required".into());
        }
        if self.image.trim().contains(char::is_whitespace) {
            return Err("Image name cannot contain spaces".into());
        }
        let name = self.name.trim();
        if !name.is_empty() && !is_valid_name(name) {
            return Err(format!(
                "Invalid container name '{}': use letters, digits, '_', '.' or '-'",
                name
            ));
        }
        for p in &self.ports {
            if !is_valid_port_mapping(p) {
                return Err(format!(
                    "Invalid port mapping '{}': expected host:container (e.g. 8080:80)",
                    p
                ));
            }
        }
        for l in &self.labels {
            if l.split_once('=').map_or(true, |(k, _)| k.trim().is_empty()) {
                return Err(format!("Invalid label '{}': expected key=value", l));
            }
        }
        for e in &self.env {
            match e.split_once('=') {
                Some((key, _)) if !key.trim().is_empty() && !key.contains(char::is_whitespace) => {}
                _ => return Err(format!("Invalid environment variable '{}': expected KEY=value", e)),
            }
        }
        if !self.cpus.trim().is_empty() && self.cpus.trim().parse::<f64>().map_or(true, |c| c <= 0.0) {
            return Err(format!("Invalid CPU limit '{}': expected a number like 0.5 or 2", self.cpus));
        }
        if !self.memory.trim().is_empty() && !is_valid_memory(self.memory.trim()) {
            return Err(format!("Invalid memory limit '{}': expected a size like 512M or 1G", self.memory));
        }
        split_command(&self.command)?;
        Ok(())
    }

    /// Arguments for wslc.exe (without the binary itself).
    pub fn to_args(&self) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        if self.start {
            args.extend(["run".into(), "-d".into()]);
        } else {
            args.push("create".into());
        }

        let mut opt = |flag: &str, value: &str| {
            let value = value.trim();
            if !value.is_empty() {
                args.push(flag.into());
                args.push(value.into());
            }
        };
        opt("--name", &self.name);
        for p in &self.ports {
            opt("-p", p);
        }
        for e in &self.env {
            opt("-e", e);
        }
        for v in &self.volumes {
            opt("-v", v);
        }
        opt("--network", &self.network);
        for a in &self.network_aliases {
            opt("--network-alias", a);
        }
        for l in &self.labels {
            opt("--label", l);
        }
        opt("--entrypoint", &self.entrypoint);
        opt("-w", &self.workdir);
        opt("--hostname", &self.hostname);
        opt("-u", &self.user);
        opt("--cpus", &self.cpus);
        opt("--memory", &self.memory);
        if self.pull_always {
            opt("--pull", "always");
        }

        if self.auto_remove {
            args.push("--rm".into());
        }
        if self.tty {
            args.extend(["-i".into(), "-t".into()]);
        }

        args.push(self.image.trim().into());
        // validate() rejects unbalanced quotes; fall back to nothing here
        args.extend(split_command(&self.command).unwrap_or_default());
        args
    }

    /// Human-readable equivalent command, for the form preview.
    pub fn command_line(&self) -> String {
        format!("wslc {}", quote_args(&self.to_args()))
    }
}

/// Joins arguments into one command line, quoting where needed;
/// the inverse of [`split_command`].
pub fn quote_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.is_empty() || arg.contains(char::is_whitespace) || arg.contains(['"', '\'']) {
                if arg.contains('"') {
                    format!("'{}'", arg)
                } else {
                    format!("\"{}\"", arg)
                }
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Splits a multi-line form field into entries: one per line, trimmed,
/// skipping blanks and `#` comments.
pub fn parse_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

/// Splits a command line into arguments, honouring single and double quotes.
pub fn split_command(cmd: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_arg = false;
    let mut quote: Option<char> = None;

    for c in cmd.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                in_arg = true;
            }
            None if c.is_whitespace() => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            None => {
                current.push(c);
                in_arg = true;
            }
        }
    }
    if quote.is_some() {
        return Err("Command has an unclosed quote".into());
    }
    if in_arg {
        args.push(current);
    }
    Ok(args)
}

fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn is_valid_port(s: &str) -> bool {
    let valid = |p: &str| p.parse::<u16>().is_ok_and(|n| n > 0);
    match s.split_once('-') {
        Some((a, b)) => valid(a) && valid(b),
        None => valid(s),
    }
}

fn is_valid_port_mapping(s: &str) -> bool {
    let (ports, proto) = match s.rsplit_once('/') {
        Some((p, proto)) => (p, Some(proto)),
        None => (s, None),
    };
    if proto.is_some_and(|p| !matches!(p, "tcp" | "udp")) {
        return false;
    }
    let parts: Vec<&str> = ports.split(':').collect();
    match parts.as_slice() {
        [container] => is_valid_port(container),
        [host, container] => is_valid_port(host) && is_valid_port(container),
        [ip, host, container] => {
            !ip.is_empty() && (host.is_empty() || is_valid_port(host)) && is_valid_port(container)
        }
        _ => false,
    }
}

fn is_valid_memory(s: &str) -> bool {
    let digits = s.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let unit = &s[digits.len()..];
    digits.parse::<u64>().is_ok_and(|n| n > 0)
        && matches!(unit.to_ascii_lowercase().as_str(), "" | "b" | "k" | "kb" | "m" | "mb" | "g" | "gb")
}

/// Parses a pasted `docker run …` (or `podman`/`wslc`, `run`/`create`) command
/// into a spec. Line continuations (`\`, `^`, `` ` ``) are accepted.
/// Returns the spec and the flags that were ignored.
pub fn parse_run_command(cmd: &str) -> Result<(ContainerSpec, Vec<String>), String> {
    let joined: String = cmd
        .lines()
        .map(|l| l.trim().trim_end_matches(['\\', '^', '`']).trim())
        .collect::<Vec<_>>()
        .join(" ");
    let mut args = split_command(&joined)?.into_iter().peekable();

    let mut spec = ContainerSpec::default();
    let mut warnings = Vec::new();

    // Optional program and subcommand: `docker run`, `docker container run`, `run`…
    if args.peek().is_some_and(|a| {
        let a = a.to_lowercase();
        let a = a.trim_end_matches(".exe");
        a.ends_with("docker") || a.ends_with("podman") || a.ends_with("wslc") || a.ends_with("nerdctl")
    }) {
        args.next();
    }
    if args.peek().map(String::as_str) == Some("container") {
        args.next();
    }
    match args.peek().map(String::as_str) {
        Some("run") => {
            args.next();
            spec.start = true;
        }
        Some("create") => {
            args.next();
        }
        _ => spec.start = true,
    }

    while let Some(arg) = args.next() {
        if !arg.starts_with('-') || arg == "-" {
            spec.image = arg;
            spec.command = quote_args(&args.by_ref().collect::<Vec<_>>());
            break;
        }

        // --flag=value / -p=value
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f.to_string(), Some(v.to_string())),
            None => (arg.clone(), None),
        };
        let value = |args: &mut std::iter::Peekable<std::vec::IntoIter<String>>| {
            inline.clone().or_else(|| args.next()).ok_or(format!("Flag {} needs a value", flag))
        };

        match flag.as_str() {
            "--name" => spec.name = value(&mut args)?,
            "-p" | "--publish" => spec.ports.push(value(&mut args)?),
            "-e" | "--env" => {
                let v = value(&mut args)?;
                // `-e KEY` passes the variable through from the host
                if v.contains('=') {
                    spec.env.push(v);
                } else if let Ok(host) = std::env::var(&v) {
                    spec.env.push(format!("{}={}", v, host));
                }
            }
            "-v" | "--volume" => spec.volumes.push(value(&mut args)?),
            "--network" | "--net" => spec.network = value(&mut args)?,
            "--network-alias" => spec.network_aliases.push(value(&mut args)?),
            "-l" | "--label" => spec.labels.push(value(&mut args)?),
            "--entrypoint" => spec.entrypoint = value(&mut args)?,
            "-w" | "--workdir" => spec.workdir = value(&mut args)?,
            "-h" | "--hostname" => spec.hostname = value(&mut args)?,
            "-u" | "--user" => spec.user = value(&mut args)?,
            "--cpus" => spec.cpus = value(&mut args)?,
            "-m" | "--memory" => spec.memory = value(&mut args)?,
            "--pull" => spec.pull_always = value(&mut args)? == "always",
            "--rm" => spec.auto_remove = true,
            "-d" | "--detach" => spec.start = true,
            "-i" | "--interactive" | "-t" | "--tty" => spec.tty = true,
            "--restart" => {
                value(&mut args)?;
                warnings.push("--restart is not supported by wslc and was ignored".to_string());
            }
            // Clusters of boolean short flags: -it, -dit, -itd…
            f if !f.starts_with("--") && f.len() > 2 && f[1..].chars().all(|c| matches!(c, 'd' | 'i' | 't')) => {
                spec.tty |= f.contains(['i', 't']);
            }
            other => return Err(format!("Unsupported flag {}", other)),
        }
    }

    if spec.image.is_empty() {
        return Err("No image found in the command".into());
    }
    spec.validate()?;
    Ok((spec, warnings))
}
