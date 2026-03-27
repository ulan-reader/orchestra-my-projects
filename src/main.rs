use serde::Deserialize;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SERVICES: &[&str] = &[
    "admin-service",
    "application-order-service",
    "auth",
    "bpm",
    "db-struct",
    "dictionary",
    "file-service",
    "gateway",
    "hr",
    "notification",
    "organization-service",
];

#[derive(Debug, Deserialize, Default)]
struct AppConfig {
    server: Option<ServerConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct ServerConfig {
    port: Option<PortValue>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PortValue {
    Number(u16),
    Text(String),
}

fn find_jar(path: &str) -> Option<PathBuf> {
    let target = PathBuf::from(path).join("target");

    fs::read_dir(target)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension().map(|ext| ext == "jar").unwrap_or(false)
                && !p.to_string_lossy().contains("original")
        })
}

fn parse_port_value(port: &PortValue) -> Option<u16> {
    match port {
        PortValue::Number(n) => Some(*n),
        PortValue::Text(s) => resolve_spring_placeholder_port(s),
    }
}

fn resolve_spring_placeholder_port(value: &str) -> Option<u16> {
    let trimmed = value.trim();

    if let Ok(n) = trimmed.parse::<u16>() {
        return Some(n);
    }

    // поддержка формата ${ENV_NAME:8083}
    if trimmed.starts_with("${") && trimmed.ends_with('}') {
        let inner = &trimmed[2..trimmed.len() - 1];

        if let Some((env_name, default_value)) = inner.split_once(':') {
            if let Ok(env_val) = std::env::var(env_name.trim()) {
                if let Ok(n) = env_val.trim().parse::<u16>() {
                    return Some(n);
                }
            }

            if let Ok(n) = default_value.trim().parse::<u16>() {
                return Some(n);
            }
        } else {
            if let Ok(env_val) = std::env::var(inner.trim()) {
                if let Ok(n) = env_val.trim().parse::<u16>() {
                    return Some(n);
                }
            }
        }
    }

    None
}

fn read_yaml_config(path: &Path) -> Option<AppConfig> {
    let content = fs::read_to_string(path).ok()?;
    serde_yaml::from_str::<AppConfig>(&content).ok()
}

fn resolve_service_port(service: &str, profile: &str) -> Option<u16> {
    let resources = PathBuf::from(service).join("src").join("main").join("resources");

    let base_file = resources.join("application.yml");
    let profile_file = resources.join(format!("application-{}.yml", profile));

    let base_cfg = read_yaml_config(&base_file).unwrap_or_default();
    let profile_cfg = read_yaml_config(&profile_file).unwrap_or_default();

    // Spring profile config должен переопределять base
    if let Some(server) = profile_cfg.server {
        if let Some(port) = server.port {
            if let Some(n) = parse_port_value(&port) {
                return Some(n);
            }
        }
    }

    if let Some(server) = base_cfg.server {
        if let Some(port) = server.port {
            if let Some(n) = parse_port_value(&port) {
                return Some(n);
            }
        }
    }

    None
}

fn run_service(jar: PathBuf, service: &str, profile: &str) -> Child {
    let port = resolve_service_port(service, profile).unwrap_or(8080);

    println!(
        "🚀 Starting: {} [{}] on port {}",
        jar.display(),
        profile,
        port
    );

    let service_name = PathBuf::from(service)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let log_file = File::create(format!("logs/{}.log", service_name)).unwrap();

    Command::new("java")
        .env("SPRING_PROFILES_ACTIVE", profile)
        .arg("-jar")
        .arg(jar)
        .stdout(Stdio::from(log_file.try_clone().unwrap()))
        .stderr(Stdio::from(log_file))
        .spawn()
        .expect("Failed to start service")
}

fn print_ports(profile: &str) {
    println!("\n📡 Service ports [{}]:", profile);

    for service in SERVICES {
        match resolve_service_port(service, profile) {
            Some(port) => println!("  {:<28} -> {}", service, port),
            None => println!("  {:<28} -> {} (default/fallback)", service, 8080),
        }
    }

    println!();
}

fn main() {
    fs::create_dir_all("logs").unwrap();

    let children: Arc<Mutex<Vec<Child>>> = Arc::new(Mutex::new(vec![]));
    let children_clone = Arc::clone(&children);

    ctrlc::set_handler(move || {
        println!("\n🛑 Shutting down services...");

        let mut children = children_clone.lock().unwrap();

        for child in children.iter_mut() {
            let _ = child.kill();
        }

        println!("✅ All services stopped");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let profile = "dev";

    print_ports(profile);

    {
        let mut children_guard = children.lock().unwrap();

        for service in SERVICES {
            if let Some(jar) = find_jar(service) {
                let child = run_service(jar, service, profile);
                children_guard.push(child);
            } else {
                eprintln!("❌ Jar not found for {}", service);
            }

            std::thread::sleep(Duration::from_millis(500));
        }
    }

    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}