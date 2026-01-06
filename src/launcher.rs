use std::collections::HashMap;
use std::io::{self, Write};
use std::process::Command;
use std::time::Duration;

use anyhow::Result;

use crate::config::{
    ENV_AUTH_TOKEN, ENV_BASE_URL, ENV_MODEL, ENV_OPENAI_OAUTH, ENV_PROXY_TARGET_URL,
    ENV_SMALL_FAST_MODEL, Profile,
};
use crate::openai_oauth;
use crate::proxy;

/// Spinner characters for visual feedback
const SPINNER_CHARS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Timeout for proxy startup
const PROXY_STARTUP_TIMEOUT_SECS: u64 = 10;

/// Get non-empty env var value from a map
fn get_non_empty_env(map: &HashMap<String, String>, key: &str) -> Option<String> {
    map.get(key).cloned().filter(|v| !v.trim().is_empty())
}

/// Launch Claude Code with the specified profile's environment variables.
/// We spawn a child process to run Claude, then unload models after it exits.
pub fn exec_claude(profile: &Profile) -> Result<()> {
    let mut resolved_env = profile.env.clone();

    if openai_oauth::openai_oauth_enabled(resolved_env.get(ENV_OPENAI_OAUTH)) {
        let rt = tokio::runtime::Runtime::new()?;
        let access_token = rt.block_on(openai_oauth::ensure_access_token_interactive())?;
        resolved_env.insert(ENV_AUTH_TOKEN.to_string(), access_token);
    }

    let proxy_target_url = get_non_empty_env(&resolved_env, ENV_PROXY_TARGET_URL);
    let needs_proxy = proxy_target_url.is_some();

    if let Some(proxy_target_url) = proxy_target_url {
        let model_override = get_non_empty_env(&resolved_env, ENV_MODEL);
        let auxiliary_model = get_non_empty_env(&resolved_env, ENV_SMALL_FAST_MODEL);

        // Start proxy in a background thread (not fork - fork causes issues with reqwest/TLS)
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            rt.block_on(async {
                if let Err(e) =
                    proxy::start_server(proxy_target_url, model_override, auxiliary_model).await
                {
                    eprintln!("Proxy error: {}", e);
                }
            });
        });

        // Wait for proxy to be ready
        print!("Starting proxy ");
        io::stdout().flush()?;

        let timeout = Duration::from_secs(PROXY_STARTUP_TIMEOUT_SECS);
        let start = std::time::Instant::now();
        let mut spinner_idx = 0;

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .expect("Failed to build HTTP client");
        let health_url = format!("http://localhost:{}/health", proxy::PROXY_PORT);

        while start.elapsed() < timeout {
            if let Ok(resp) = client.get(&health_url).send()
                && resp.status().is_success()
            {
                println!("\r{} Proxy started!        ", SPINNER_CHARS[spinner_idx]);
                break;
            }

            print!("\r{} Starting proxy...", SPINNER_CHARS[spinner_idx]);
            io::stdout().flush()?;
            spinner_idx = (spinner_idx + 1) % SPINNER_CHARS.len();
            std::thread::sleep(Duration::from_millis(100));
        }

        if start.elapsed() >= timeout {
            println!();
            anyhow::bail!("Proxy did not start within {} seconds", PROXY_STARTUP_TIMEOUT_SECS);
        }
    }

    let mut cmd = Command::new("claude");

    // Set all environment variables from the profile
    for (key, value) in &resolved_env {
        if key == ENV_PROXY_TARGET_URL || key == ENV_OPENAI_OAUTH {
            continue;
        }
        cmd.env(key, value);
    }

    if needs_proxy {
        cmd.env(ENV_BASE_URL, proxy::PROXY_ANTHROPIC_URL);
    }

    // Spawn and wait so we can unload after exit.
    let status = cmd.status()?;

    if !status.success() {
        anyhow::bail!("Claude Code exited with status: {}", status);
    }

    Ok(())
}
