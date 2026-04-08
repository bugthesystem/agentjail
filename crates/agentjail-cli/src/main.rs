//! agentjail CLI - Real-time jail monitoring TUI

mod app;
mod tui;
mod ui;

use app::{App, JailStatus};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Parser)]
#[command(name = "agentjail")]
#[command(about = "Minimal Linux sandbox for untrusted code")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a command in a jail
    Run {
        /// Source directory (mounted read-only)
        #[arg(short, long)]
        source: PathBuf,

        /// Output directory (mounted read-write)
        #[arg(short, long)]
        output: PathBuf,

        /// Preset: build, agent, dev, test
        #[arg(short, long, default_value = "build")]
        preset: String,

        /// Timeout in seconds (0 = no limit)
        #[arg(short, long)]
        timeout: Option<u64>,

        /// Command to run
        cmd: String,

        /// Arguments
        args: Vec<String>,
    },

    /// Open TUI dashboard to monitor jails
    Tui,

    /// Demo mode: spawn sample jails and monitor in TUI
    Demo,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            source,
            output,
            preset,
            timeout,
            cmd,
            args,
        } => {
            run_jail(source, output, &preset, timeout, &cmd, &args).await?;
        }
        Commands::Tui => {
            let mut app = App::new();
            tui::run(&mut app).await?;
        }
        Commands::Demo => {
            run_demo().await?;
        }
    }

    Ok(())
}

async fn run_jail(
    source: PathBuf,
    output: PathBuf,
    preset: &str,
    timeout: Option<u64>,
    cmd: &str,
    args: &[String],
) -> anyhow::Result<()> {
    use agentjail::{Jail, JailConfig, SeccompLevel, preset_agent, preset_build, preset_dev};

    let mut config = match preset {
        "build" => preset_build(&source, &output),
        "agent" => preset_agent(&source, &output),
        "dev" => preset_dev(&source, &output),
        "test" => JailConfig {
            source,
            output,
            timeout_secs: 30,
            user_namespace: false,
            seccomp: SeccompLevel::Disabled,
            landlock: false,
            memory_mb: 0,
            cpu_percent: 0,
            max_pids: 0,
            env: vec![("PATH".to_string(), "/usr/local/bin:/usr/bin:/bin".to_string())],
            ..Default::default()
        },
        _ => {
            eprintln!("Unknown preset: {}. Using 'build'", preset);
            preset_build(&source, &output)
        }
    };

    if let Some(t) = timeout {
        config.timeout_secs = t;
    }

    let jail = Jail::new(config)?;
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let result = jail.run(cmd, &args_refs).await?;

    print!("{}", String::from_utf8_lossy(&result.stdout));
    eprint!("{}", String::from_utf8_lossy(&result.stderr));

    std::process::exit(result.exit_code);
}

async fn run_demo() -> anyhow::Result<()> {
    use std::fs;

    // Setup temp dirs
    let base = PathBuf::from("/tmp/agentjail-demo");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("src"))?;
    fs::create_dir_all(base.join("out"))?;

    // Create demo scripts
    let slow_script = r#"#!/bin/sh
echo "Starting slow build..."
for i in $(seq 1 10); do
    echo "Step $i/10..."
    sleep 1
done
echo "Build complete!"
"#;

    let quick_script = r#"#!/bin/sh
echo "Quick task running"
echo "Output line 1"
echo "Output line 2"
echo "Done!"
"#;

    let fail_script = r#"#!/bin/sh
echo "Starting..."
echo "Something went wrong!" >&2
exit 1
"#;

    fs::write(base.join("src/slow.sh"), slow_script)?;
    fs::write(base.join("src/quick.sh"), quick_script)?;
    fs::write(base.join("src/fail.sh"), fail_script)?;

    let app = Arc::new(Mutex::new(App::new()));
    let app_clone = app.clone();
    let base_clone = base.clone();

    // Spawn demo jails in background
    tokio::spawn(async move {
        spawn_demo_jails(app_clone, &base_clone).await;
    });

    // Run TUI with shared state
    tui::run_shared(app).await?;

    // Cleanup
    let _ = fs::remove_dir_all(&base);

    Ok(())
}

async fn spawn_demo_jails(app: Arc<Mutex<App>>, base: &std::path::Path) {
    use agentjail::{Jail, JailConfig, SeccompLevel};

    let source = base.join("src");
    let output = base.join("out");

    let config = JailConfig {
        source: source.clone(),
        output: output.clone(),
        timeout_secs: 30,
        user_namespace: false,
        seccomp: SeccompLevel::Disabled,
        landlock: false,
        memory_mb: 0,
        cpu_percent: 0,
        max_pids: 0,
        env: vec![("PATH".to_string(), "/usr/local/bin:/usr/bin:/bin".to_string())],
        ..Default::default()
    };

    // Spawn slow job
    {
        let jail = match Jail::new(config.clone()) {
            Ok(j) => j,
            Err(_) => return,
        };

        let handle = match jail.spawn("/bin/sh", &["/workspace/slow.sh"]) {
            Ok(h) => h,
            Err(_) => return,
        };

        let id = {
            let mut app = app.lock().await;
            app.add_jail(handle.pid(), "sh slow.sh".to_string(), "build".to_string())
        };

        let app_clone = app.clone();
        tokio::spawn(async move {
            let result = handle.wait().await;
            let mut app = app_clone.lock().await;
            match result {
                Ok(out) => {
                    let status = if out.timed_out {
                        JailStatus::TimedOut
                    } else {
                        JailStatus::Completed(out.exit_code)
                    };
                    app.update_status(id, status);
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        app.append_stdout(id, line.to_string());
                    }
                    for line in String::from_utf8_lossy(&out.stderr).lines() {
                        app.append_stderr(id, line.to_string());
                    }
                }
                Err(_) => app.update_status(id, JailStatus::Killed),
            }
        });
    }

    // Small delay before next
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Spawn quick job
    {
        let jail = match Jail::new(config.clone()) {
            Ok(j) => j,
            Err(_) => return,
        };

        let handle = match jail.spawn("/bin/sh", &["/workspace/quick.sh"]) {
            Ok(h) => h,
            Err(_) => return,
        };

        let id = {
            let mut app = app.lock().await;
            app.add_jail(handle.pid(), "sh quick.sh".to_string(), "agent".to_string())
        };

        let app_clone = app.clone();
        tokio::spawn(async move {
            let result = handle.wait().await;
            let mut app = app_clone.lock().await;
            match result {
                Ok(out) => {
                    let status = if out.timed_out {
                        JailStatus::TimedOut
                    } else {
                        JailStatus::Completed(out.exit_code)
                    };
                    app.update_status(id, status);
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        app.append_stdout(id, line.to_string());
                    }
                    for line in String::from_utf8_lossy(&out.stderr).lines() {
                        app.append_stderr(id, line.to_string());
                    }
                }
                Err(_) => app.update_status(id, JailStatus::Killed),
            }
        });
    }

    // Small delay before next
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Spawn failing job
    {
        let jail = match Jail::new(config.clone()) {
            Ok(j) => j,
            Err(_) => return,
        };

        let handle = match jail.spawn("/bin/sh", &["/workspace/fail.sh"]) {
            Ok(h) => h,
            Err(_) => return,
        };

        let id = {
            let mut app = app.lock().await;
            app.add_jail(handle.pid(), "sh fail.sh".to_string(), "dev".to_string())
        };

        let app_clone = app.clone();
        tokio::spawn(async move {
            let result = handle.wait().await;
            let mut app = app_clone.lock().await;
            match result {
                Ok(out) => {
                    let status = if out.timed_out {
                        JailStatus::TimedOut
                    } else {
                        JailStatus::Completed(out.exit_code)
                    };
                    app.update_status(id, status);
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        app.append_stdout(id, line.to_string());
                    }
                    for line in String::from_utf8_lossy(&out.stderr).lines() {
                        app.append_stderr(id, line.to_string());
                    }
                }
                Err(_) => app.update_status(id, JailStatus::Killed),
            }
        });
    }
}
