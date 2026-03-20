use std::process::{Command, Stdio, Child};
use std::path::PathBuf;
use std::fs::File;
use std::thread;

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

fn build_service(path: &str) -> bool {
    println!("🔨 Building: {}", path);

    let current_path = PathBuf::from(path);

    let status = Command::new("mvn.cmd")
        .arg("clean")
        .arg("package")
        .arg("-DskipTests")
        .arg("-T1C")
        .current_dir(current_path)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("✅ Built: {}", path);
            true
        }
        _ => {
            eprintln!("❌ Failed: {}", path);
            false
        }
    }
}

fn find_jar(path: &str) -> Option<PathBuf> {
    let target = PathBuf::from(path).join("target");

    std::fs::read_dir(target)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension().map(|ext| ext == "jar").unwrap_or(false)
                && !p.to_string_lossy().contains("original")
        })
}

fn run_service(jar: PathBuf, service: &str) -> Child {
    println!("🚀 Starting: {}", jar.display());

    let service_name = PathBuf::from(service)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let log_file = File::create(format!("logs/{}.log", service_name)).unwrap();

    Command::new("java")
        .arg("-jar")
        .arg(jar)
        .stdout(Stdio::from(log_file.try_clone().unwrap()))
        .stderr(Stdio::from(log_file))
        .spawn()
        .expect("Failed to start service")
}

use std::sync::{Arc, Mutex};

fn main() {
    std::fs::create_dir_all("logs").unwrap();

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
    }).expect("Error setting Ctrl-C handler");

    // 🚀 RUN
    {
        let mut children_guard = children.lock().unwrap();

        for service in SERVICES {
            if let Some(jar) = find_jar(service) {
                let child = run_service(jar, service);
                children_guard.push(child);
            }
        }
    }

    // держим процесс живым
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}