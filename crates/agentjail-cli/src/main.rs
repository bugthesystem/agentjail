//! agentjail CLI — sandbox runner and monitoring TUI.

mod app;
mod tui;
mod ui;

use app::{App, JailStatus, Stream};
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
        #[arg(short, long)]
        source: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Preset: build, install, agent, dev, gpu, test
        #[arg(short, long, default_value = "build")]
        preset: String,
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Enable GPU passthrough (NVIDIA)
        #[arg(long)]
        gpu: bool,
        /// Command to run
        cmd: String,
        /// Arguments
        args: Vec<String>,
    },
    /// Open TUI dashboard
    Tui,
    /// Spawn sample jails and monitor in TUI
    Demo,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run { source, output, preset, timeout, gpu, cmd, args } => {
            run_jail(source, output, &preset, timeout, gpu, &cmd, &args).await?;
        }
        Commands::Tui => {
            tui::run(&mut App::new()).await?;
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
    gpu_flag: bool,
    cmd: &str,
    args: &[String],
) -> anyhow::Result<()> {
    use agentjail::{GpuConfig, Jail, JailConfig, SeccompLevel, preset_agent, preset_build, preset_dev, preset_gpu};

    let mut config = match preset {
        "build" => preset_build(&source, &output),
        "agent" => preset_agent(&source, &output),
        "dev" => preset_dev(&source, &output),
        "gpu" => preset_gpu(&source, &output),
        "test" => JailConfig {
            source, output,
            timeout_secs: 30,
            user_namespace: false,
            seccomp: SeccompLevel::Disabled,
            landlock: false,
            memory_mb: 0, cpu_percent: 0, max_pids: 0,
            env: vec![("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into())],
            ..Default::default()
        },
        _ => {
            eprintln!("Unknown preset: {}. Using 'build'", preset);
            preset_build(&source, &output)
        }
    };

    if gpu_flag {
        config.gpu = GpuConfig { enabled: true, devices: vec![] };
    }
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

// ---------------------------------------------------------------------------
// Demo mode
// ---------------------------------------------------------------------------

async fn run_demo() -> anyhow::Result<()> {
    use agentjail::{JailConfig, SeccompLevel};
    use std::fs;

    let base = PathBuf::from("/tmp/agentjail-demo");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("src"))?;
    fs::create_dir_all(base.join("out"))?;

    fs::write(base.join("src/slow.sh"), "#!/bin/sh\necho 'Starting slow build...'\nfor i in $(seq 1 10); do echo \"Step $i/10...\"; sleep 1; done\necho 'Build complete!'\n")?;
    fs::write(base.join("src/quick.sh"), "#!/bin/sh\necho 'Quick task running'\necho 'Output line 1'\necho 'Output line 2'\necho 'Done!'\n")?;
    fs::write(base.join("src/fail.sh"), "#!/bin/sh\necho 'Starting...'\necho 'Something went wrong!' >&2\nexit 1\n")?;

    let config = JailConfig {
        source: base.join("src"),
        output: base.join("out"),
        timeout_secs: 30,
        user_namespace: false,
        seccomp: SeccompLevel::Disabled,
        landlock: false,
        memory_mb: 0, cpu_percent: 0, max_pids: 0,
        env: vec![("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into())],
        ..Default::default()
    };

    let app = Arc::new(Mutex::new(App::new()));

    // Spawn jails in background
    let app_bg = app.clone();
    let cfg = config.clone();
    tokio::spawn(async move {
        let demos = [
            DemoSpec { script: "/workspace/slow.sh", label: "sh slow.sh", preset: "build", network: "none", seccomp: "standard", timeout_secs: 30, memory_limit_mb: 512 },
            DemoSpec { script: "/workspace/quick.sh", label: "sh quick.sh", preset: "agent", network: "loopback", seccomp: "standard", timeout_secs: 30, memory_limit_mb: 256 },
            DemoSpec { script: "/workspace/fail.sh", label: "sh fail.sh", preset: "dev", network: "allowlist", seccomp: "strict", timeout_secs: 10, memory_limit_mb: 128 },
        ];
        for spec in &demos {
            spawn_demo(&app_bg, &cfg, spec).await;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });

    tui::run_shared(app).await?;
    let _ = std::fs::remove_dir_all(&base);
    Ok(())
}

struct DemoSpec {
    script: &'static str,
    label: &'static str,
    preset: &'static str,
    network: &'static str,
    seccomp: &'static str,
    timeout_secs: u64,
    memory_limit_mb: u64,
}

async fn spawn_demo(
    app: &Arc<Mutex<App>>,
    config: &agentjail::JailConfig,
    spec: &DemoSpec,
) {
    use agentjail::Jail;
    use app::JailInfo;
    use std::collections::VecDeque;
    use std::time::Instant;

    let jail = match Jail::new(config.clone()) {
        Ok(j) => j,
        Err(_) => return,
    };
    let handle = match jail.spawn("/bin/sh", &[spec.script]) {
        Ok(h) => h,
        Err(_) => return,
    };

    let id = {
        let mut a = app.lock().await;
        a.add_jail(JailInfo {
            pid: handle.pid(),
            command: spec.label.into(),
            preset: spec.preset.into(),
            status: JailStatus::Running,
            started_at: Instant::now(),
            memory_bytes: 0,
            network: spec.network.into(),
            seccomp: spec.seccomp.into(),
            timeout_secs: spec.timeout_secs,
            memory_limit_mb: spec.memory_limit_mb,
            output: VecDeque::new(),
            stdout_count: 0,
            stderr_count: 0,
        })
    };

    let app = app.clone();
    tokio::spawn(async move {
        let result = handle.wait().await;
        let mut a = app.lock().await;
        match result {
            Ok(out) => {
                let status = if out.timed_out {
                    JailStatus::TimedOut
                } else {
                    JailStatus::Completed(out.exit_code)
                };
                a.update_status(id, status);
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    a.append_output(id, Stream::Stdout, line.to_string());
                }
                for line in String::from_utf8_lossy(&out.stderr).lines() {
                    a.append_output(id, Stream::Stderr, line.to_string());
                }
            }
            Err(_) => a.update_status(id, JailStatus::Killed),
        }
    });
}
