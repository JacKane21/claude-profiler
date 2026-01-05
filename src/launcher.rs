use std::io::{self, Write};
use std::process::Command;
use std::time::Duration;

use anyhow::Result;

use crate::config::{Profile, ENV_BASE_URL, ENV_MODEL};
use crate::proxy;

/// Check if this profile requires the proxy (i.e., it's an lmstudio profile)
fn needs_proxy(profile: &Profile) -> bool {
    profile.name == "lmstudio"
        && profile
            .env
            .get(ENV_BASE_URL)
            .map_or(false, |url| url.contains(&proxy::PROXY_PORT.to_string()))
}

/// Spinner characters for visual feedback
const SPINNER_CHARS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Prompt the user with a yes/no question
fn prompt_yes_no(question: &str) -> Result<bool> {
    print!("{} [Y/n] ", question);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    Ok(input.is_empty() || input == "y" || input == "yes")
}

/// Find the lms CLI binary
fn find_lms_binary() -> Option<std::path::PathBuf> {
    let lms_paths = [
        dirs::home_dir().map(|h| h.join(".lmstudio/bin/lms")),
        Some(std::path::PathBuf::from("lms")), // In PATH
    ];

    lms_paths
        .iter()
        .filter_map(|p| p.as_ref())
        .find(|p| p.exists() || Command::new(p).arg("--version").output().is_ok())
        .cloned()
}

/// Install the lms CLI by running the bootstrap command
fn install_lms_cli() -> Result<bool> {
    let bootstrap_path = dirs::home_dir()
        .map(|h| h.join(".lmstudio/bin/lms"))
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

    if !bootstrap_path.exists() {
        println!();
        println!("The LM Studio CLI bootstrap binary was not found.");
        println!("Please run LM Studio at least once to install it.");
        println!();
        return Ok(false);
    }

    println!();
    println!("Installing LM Studio CLI...");
    println!("Running: {} bootstrap", bootstrap_path.display());
    println!();

    let status = Command::new(&bootstrap_path)
        .arg("bootstrap")
        .status()?;

    if status.success() {
        println!("LM Studio CLI installed successfully!");
        return Ok(true);
    }

    println!("Failed to install LM Studio CLI.");
    Ok(false)
}

/// Prompt the user to install the lms CLI
fn prompt_install_lms() -> Result<bool> {
    println!();
    println!("The LM Studio CLI (lms) is required to load models automatically.");
    println!();
    println!("Without it, you'll need to manually load models in LM Studio before");
    println!("using ClaudeProfiler with an LM Studio profile.");
    println!();

    if prompt_yes_no("Would you like to install the LM Studio CLI now?")? {
        return install_lms_cli();
    }

    println!();
    println!("To install later, run:");
    println!("  ~/.lmstudio/bin/lms bootstrap");
    println!();
    println!("Or load models manually in LM Studio before launching.");
    println!();

    Ok(false)
}

/// Load a model in LM Studio using the lms CLI
fn load_lmstudio_model(model: &str) -> Result<()> {
    // Try to find lms
    let lms_path = match find_lms_binary() {
        Some(p) => p,
        None => {
            // Prompt to install
            if !prompt_install_lms()? {
                println!("Continuing without auto-loading model...");
                println!("Make sure the model is loaded in LM Studio!");
                println!();
                return Ok(());
            }
            // Try to find it again after installation
            match find_lms_binary() {
                Some(p) => p,
                None => {
                    println!("LM Studio CLI still not found after installation.");
                    println!("Continuing without auto-loading model...");
                    return Ok(());
                }
            }
        }
    };

    println!("Loading model in LM Studio...");

    // Run lms load and capture output
    let output = Command::new(&lms_path)
        .args(["load", model, "--yes"]) // --yes to skip confirmation prompts
        .output()?;

    if output.status.success() {
        println!("Model loaded!");
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("\r  Model load failed:");
        if !stderr.is_empty() {
            for line in stderr.lines().take(5) {
                println!("    {}", line);
            }
        }
        if !stdout.is_empty() && stderr.is_empty() {
            for line in stdout.lines().take(5) {
                println!("    {}", line);
            }
        }
        println!();
        println!("  Make sure the model is downloaded and loaded in LM Studio.");
        println!();
    }

    Ok(())
}

/// Launch Claude Code with the specified profile's environment variables.
/// On Unix, this forks a child process to run the proxy, then exec()s Claude.
#[cfg(unix)]
pub fn exec_claude(profile: &Profile) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let needs_proxy = needs_proxy(profile);

    if needs_proxy {
        // Get the LMStudio model name from the profile
        let model = profile
            .env
            .get(ENV_MODEL)
            .cloned()
            .unwrap_or_else(|| "default".to_string());

        // Load the model in LM Studio first
        load_lmstudio_model(&model)?;

        // Fork: child runs proxy, parent will exec claude
        match unsafe { libc::fork() } {
            -1 => anyhow::bail!("Failed to fork"),
            0 => {
                // Child process: run the proxy server
                // This process will be orphaned when parent execs, but that's fine
                let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                rt.block_on(async {
                    if let Err(e) = proxy::start_server(model).await {
                        eprintln!("Proxy error: {}", e);
                    }
                });
                std::process::exit(0);
            }
            _child_pid => {
                // Parent process: wait for proxy to be ready, then exec claude
                print!("Starting proxy ");
                io::stdout().flush()?;

                let timeout = Duration::from_secs(10);
                let start = std::time::Instant::now();
                let mut spinner_idx = 0;

                while start.elapsed() < timeout {
                    if let Ok(client) = reqwest::blocking::Client::builder()
                        .timeout(Duration::from_millis(200))
                        .build()
                    {
                        let health_url = format!("http://localhost:{}/health", proxy::PROXY_PORT);
                        if let Ok(resp) = client.get(&health_url).send() {
                            if resp.status().is_success() {
                                println!("\r{} Proxy started!        ", SPINNER_CHARS[spinner_idx]);
                                break;
                            }
                        }
                    }

                    print!("\r{} Starting proxy...", SPINNER_CHARS[spinner_idx]);
                    io::stdout().flush()?;
                    spinner_idx = (spinner_idx + 1) % SPINNER_CHARS.len();
                    std::thread::sleep(Duration::from_millis(100));
                }

                if start.elapsed() >= timeout {
                    println!();
                    anyhow::bail!("Proxy did not start within 10 seconds");
                }
            }
        }
    }

    let mut cmd = Command::new("claude");

    // Set all environment variables from the profile
    for (key, value) in &profile.env {
        cmd.env(key, value);
    }

    // exec() replaces the current process - this never returns on success
    let err = cmd.exec();

    // If we get here, exec failed
    Err(err.into())
}

/// Launch Claude Code with the specified profile's environment variables.
/// On Windows, we spawn a child process since exec() isn't available.
#[cfg(windows)]
pub fn exec_claude(profile: &Profile) -> Result<()> {
    let needs_proxy = needs_proxy(profile);

    if needs_proxy {
        // Get the LMStudio model name from the profile
        let model = profile
            .env
            .get(ENV_MODEL)
            .cloned()
            .unwrap_or_else(|| "default".to_string());

        // Load the model in LM Studio first
        load_lmstudio_model(&model)?;

        // Start proxy in a background thread
        let model_for_proxy = model;
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            rt.block_on(async {
                if let Err(e) = proxy::start_server(model_for_proxy).await {
                    eprintln!("Proxy error: {}", e);
                }
            });
        });

        // Wait for proxy to be ready
        print!("Starting proxy ");
        io::stdout().flush()?;

        let timeout = Duration::from_secs(10);
        let start = std::time::Instant::now();
        let mut spinner_idx = 0;

        while start.elapsed() < timeout {
            if let Ok(client) = reqwest::blocking::Client::builder()
                .timeout(Duration::from_millis(200))
                .build()
            {
                let health_url = format!("http://localhost:{}/health", proxy::PROXY_PORT);
                if let Ok(resp) = client.get(&health_url).send() {
                    if resp.status().is_success() {
                        println!("\r{} Proxy started!        ", SPINNER_CHARS[spinner_idx]);
                        break;
                    }
                }
            }

            print!("\r{} Starting proxy...", SPINNER_CHARS[spinner_idx]);
            io::stdout().flush()?;
            spinner_idx = (spinner_idx + 1) % SPINNER_CHARS.len();
            std::thread::sleep(Duration::from_millis(100));
        }

        if start.elapsed() >= timeout {
            println!();
            anyhow::bail!("Proxy did not start within 10 seconds");
        }
    }

    let mut cmd = Command::new("claude");

    // Set all environment variables from the profile
    for (key, value) in &profile.env {
        cmd.env(key, value);
    }

    // On Windows, we spawn and wait for the process
    // The main process stays alive, keeping the proxy thread running
    let status = cmd.status()?;

    if !status.success() {
        anyhow::bail!("Claude Code exited with status: {}", status);
    }

    Ok(())
}
