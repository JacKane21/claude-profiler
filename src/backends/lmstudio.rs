//! LMStudio backend implementation for local models.
//!
//! LMStudio is a tool for running large language models locally.
//! It provides a REST API compatible with OpenAI's API format.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{sleep, Duration};

use super::{BackendType, LocalModel, ModelBackend};

/// LMStudio backend for running local models
#[allow(dead_code)]
pub struct LMStudioBackend {
    host: String,
}

#[allow(dead_code)]
impl LMStudioBackend {
    pub fn new(host: String) -> Self {
        Self { host }
    }

    /// Get the base API URL
    fn api_url(&self) -> String {
        format!("http://{}", self.host)
    }

    /// Parse the output of `lms ps --json` command
    fn parse_ps_json_output(output: &str) -> Vec<LocalModel> {
        // lms ps --json outputs JSON with loaded models
        // Try to parse as array of model objects
        if let Ok(models) = serde_json::from_str::<Vec<LMStudioPsModel>>(output) {
            return models
                .into_iter()
                .map(|m| LocalModel {
                    name: m.identifier.unwrap_or(m.path),
                    backend: BackendType::LMStudio,
                    size: None,
                    quantization: None,
                    family: None,
                    is_local: true,
                    description: None,
                })
                .collect();
        }
        Vec::new()
    }

    fn extract_family(name: &str) -> Option<String> {
        let lower = name.to_lowercase();
        if lower.contains("llama") {
            Some("llama".to_string())
        } else if lower.contains("qwen") {
            Some("qwen".to_string())
        } else if lower.contains("mistral") {
            Some("mistral".to_string())
        } else if lower.contains("gemma") {
            Some("gemma".to_string())
        } else if lower.contains("phi") {
            Some("phi".to_string())
        } else if lower.contains("deepseek") {
            Some("deepseek".to_string())
        } else if lower.contains("codellama") {
            Some("codellama".to_string())
        } else {
            None
        }
    }
}

/// Response from LMStudio /v1/models endpoint (OpenAI format)
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct LMStudioModelsResponse {
    data: Vec<LMStudioModelEntry>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct LMStudioModelEntry {
    id: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct LMStudioPsModel {
    #[serde(default)]
    path: String,
    #[serde(default)]
    identifier: Option<String>,
}

#[async_trait]
impl ModelBackend for LMStudioBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::LMStudio
    }

    async fn list_local_models(&self) -> Result<Vec<LocalModel>> {
        // Try API first, fall back to CLI
        let client = reqwest::Client::new();
        let url = format!("{}/v1/models", self.api_url());

        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                let models: LMStudioModelsResponse = response.json().await?;
                Ok(models
                    .data
                    .into_iter()
                    .map(|m| {
                        let family = Self::extract_family(&m.id);
                        LocalModel {
                            name: m.id,
                            backend: BackendType::LMStudio,
                            size: None,
                            quantization: None,
                            family,
                            is_local: true,
                            description: None,
                        }
                    })
                    .collect())
            }
            _ => {
                // Fall back to CLI
                let output = Command::new("lms")
                    .args(["ps", "--json"])
                    .output()
                    .await
                    .context("Failed to run lms ps --json")?;

                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    Ok(Self::parse_ps_json_output(&stdout))
                } else {
                    Ok(Vec::new())
                }
            }
        }
    }

    async fn list_registry_models(&self) -> Result<Vec<LocalModel>> {
        // LMStudio doesn't have a registry API - models are managed via the GUI
        Ok(Vec::new())
    }

    async fn pull_model(&self, _name: &str) -> Result<()> {
        anyhow::bail!("LMStudio models must be downloaded via the LMStudio application. Press 'l' to open LMStudio.")
    }

    async fn delete_model(&self, _name: &str) -> Result<()> {
        anyhow::bail!("LMStudio models must be deleted via the LMStudio application. Press 'l' to open LMStudio.")
    }

    async fn is_server_running(&self) -> bool {
        let client = reqwest::Client::new();
        let url = format!("{}/v1/models", self.api_url());

        client
            .get(&url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn start_server(&self) -> Result<()> {
        // Check if already running
        if self.is_server_running().await {
            return Ok(());
        }

        // Start lms server in background
        Command::new("lms")
            .args(["server", "start"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to start lms server")?;

        // Wait for server to be ready (up to 30 seconds)
        for _ in 0..30 {
            sleep(Duration::from_secs(1)).await;
            if self.is_server_running().await {
                return Ok(());
            }
        }

        anyhow::bail!("LMStudio server did not start within 30 seconds")
    }

    async fn stop_server(&self) -> Result<()> {
        // LMStudio doesn't have a clean stop command via CLI
        // Leave the server running
        Ok(())
    }

    fn api_endpoint(&self) -> String {
        format!("{}/v1", self.api_url())
    }

    async fn is_installed(&self) -> bool {
        Command::new("lms")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn install_instructions(&self) -> &'static str {
        "Install LMStudio:\n  Download from https://lmstudio.ai\n  Then install the CLI: lms bootstrap"
    }
}
