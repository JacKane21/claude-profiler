use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;

use crate::config::{Profile, ENV_BASE_URL, ENV_MODEL, ENV_SMALL_FAST_MODEL};
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

#[derive(Debug)]
struct LmstudioUnloadInfo {
    lms_path: std::path::PathBuf,
    model: String,
    loaded_by_us: bool,
}

#[derive(Debug, Deserialize)]
struct LmstudioPsModel {
    #[serde(default)]
    path: String,
    #[serde(default)]
    identifier: Option<String>,
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

fn model_matches_loaded(model: &str, loaded: &LmstudioPsModel) -> bool {
    if let Some(identifier) = loaded.identifier.as_deref() {
        if identifier == model {
            return true;
        }
    }
    if loaded.path == model {
        return true;
    }
    let model_lower = model.to_lowercase();
    loaded.path.to_lowercase().contains(&model_lower)
}

fn is_lmstudio_model_loaded(lms_path: &Path, model: &str) -> Result<bool> {
    let output = Command::new(lms_path).args(["ps", "--json"]).output()?;
    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let models: Vec<LmstudioPsModel> = match serde_json::from_str(&stdout) {
        Ok(models) => models,
        Err(_) => return Ok(false),
    };

    Ok(models.iter().any(|m| model_matches_loaded(model, m)))
}

/// Install the lms CLI by running the bootstrap command
fn install_lms_cli() -> Result<bool> {
    let bootstrap_path = dirs::home_dir()
        .map(|h| h.join(".lmstudio/bin/lms"))
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

    if !bootstrap_path.exists() {
        println!();
        println!("The LM Studio CLI bootstrap binary was not found.");
        println!();
        println!("If you haven't installed LM Studio, download it from:");
        println!("  https://lmstudio.ai");
        println!();
        println!("If already installed, please run LM Studio at least once to set up the CLI.");
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
fn load_lmstudio_model(model: &str) -> Result<Option<LmstudioUnloadInfo>> {
    // Try to find lms
    let lms_path = match find_lms_binary() {
        Some(p) => p,
        None => {
            // Prompt to install
            if !prompt_install_lms()? {
                println!("Continuing without auto-loading model...");
                println!("Make sure the model is loaded in LM Studio!");
                println!();
                return Ok(None);
            }
            // Try to find it again after installation
            match find_lms_binary() {
                Some(p) => p,
                None => {
                    println!("LM Studio CLI still not found after installation.");
                    println!("Continuing without auto-loading model...");
                    return Ok(None);
                }
            }
        }
    };

    if is_lmstudio_model_loaded(&lms_path, model)? {
        println!("Model already loaded in LM Studio.");
        return Ok(Some(LmstudioUnloadInfo {
            lms_path,
            model: model.to_string(),
            loaded_by_us: false,
        }));
    }

    println!("Loading model in LM Studio...");

    // Run lms load and capture output
    let output = Command::new(&lms_path)
        .args(["load", model, "--yes"]) // --yes to skip confirmation prompts
        .output()?;

    if output.status.success() {
        println!("Model loaded!");
        return Ok(Some(LmstudioUnloadInfo {
            lms_path,
            model: model.to_string(),
            loaded_by_us: true,
        }));
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

    Ok(None)
}

fn unload_lmstudio_model(info: &LmstudioUnloadInfo) {
    if !info.loaded_by_us {
        return;
    }
    println!("Unloading model in LM Studio...");
    match Command::new(&info.lms_path)
        .args(["unload", &info.model])
        .output()
    {
        Ok(output) if output.status.success() => {
            println!("Model unloaded.");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            println!("Model unload failed:");
            if !stderr.is_empty() {
                for line in stderr.lines().take(5) {
                    println!("  {}", line);
                }
            } else if !stdout.is_empty() {
                for line in stdout.lines().take(5) {
                    println!("  {}", line);
                }
            }
        }
        Err(err) => {
            println!("Model unload failed: {}", err);
        }
    }
}

/// Launch Claude Code with the specified profile's environment variables.
/// We spawn a child process to run Claude, then unload models after it exits.
pub fn exec_claude(profile: &Profile) -> Result<()> {
    let needs_proxy = needs_proxy(profile);

    let mut unload_infos: Vec<LmstudioUnloadInfo> = Vec::new();

    if needs_proxy {
        // Get the LMStudio model name from the profile
        let model = profile
            .env
            .get(ENV_MODEL)
            .cloned()
            .unwrap_or_else(|| "default".to_string());

        // Get the optional auxiliary model for lightweight requests
        let auxiliary_model = profile.env.get(ENV_SMALL_FAST_MODEL).cloned();

        // Load the main model in LM Studio first
        if let Some(info) = load_lmstudio_model(&model)? {
            unload_infos.push(info);
        }

        // Load the auxiliary model if configured and different from main
        if let Some(ref aux_model) = auxiliary_model {
            if aux_model != &model {
                println!("Loading auxiliary model: {}", aux_model);
                if let Some(info) = load_lmstudio_model(aux_model)? {
                    unload_infos.push(info);
                }
            }
        }

        // Start proxy in a background thread (not fork - fork causes issues with reqwest/TLS)
        let model_for_proxy = model;
        let aux_model_for_proxy = auxiliary_model;
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            rt.block_on(async {
                if let Err(e) = proxy::start_server(model_for_proxy, aux_model_for_proxy).await {
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

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .expect("Failed to build HTTP client");
        let health_url = format!("http://localhost:{}/health", proxy::PROXY_PORT);

        while start.elapsed() < timeout {
            match client.get(&health_url).send() {
                Ok(resp) => {
                    if resp.status().is_success() {
                        println!("\r{} Proxy started!        ", SPINNER_CHARS[spinner_idx]);
                        break;
                    }
                }
                Err(_) => {}
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

    // Spawn and wait so we can unload after exit.
    let status = cmd.status()?;

    // Unload all models that were loaded by us
    for info in unload_infos {
        unload_lmstudio_model(&info);
    }

    if !status.success() {
        anyhow::bail!("Claude Code exited with status: {}", status);
    }

    Ok(())
}
