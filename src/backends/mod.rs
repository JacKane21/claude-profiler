//! Backend implementations for local model inference servers.
//!
//! This module provides a unified interface for interacting with different
//! local LLM backends.

pub mod lmstudio;

pub use lmstudio::LMStudioBackend;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Supported model backends
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendType {
    /// LMStudio backend (local models via LMStudio)
    LMStudio,
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendType::LMStudio => write!(f, "LMStudio"),
        }
    }
}

impl Default for BackendType {
    fn default() -> Self {
        BackendType::LMStudio
    }
}

/// Information about a local model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModel {
    /// Model name/identifier
    pub name: String,
    /// Which backend this model belongs to
    pub backend: BackendType,
    /// Model size in bytes (if known)
    pub size: Option<u64>,
    /// Quantization format (e.g., "Q4_0", "4bit")
    pub quantization: Option<String>,
    /// Model family (e.g., "llama", "qwen", "mistral")
    pub family: Option<String>,
    /// Whether the model is downloaded locally
    pub is_local: bool,
    /// Human-readable description
    pub description: Option<String>,
}

impl LocalModel {
    /// Format size as human-readable string
    pub fn size_display(&self) -> String {
        match self.size {
            Some(bytes) => {
                let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                if gb >= 1.0 {
                    format!("{:.1} GB", gb)
                } else {
                    let mb = bytes as f64 / (1024.0 * 1024.0);
                    format!("{:.0} MB", mb)
                }
            }
            None => "Unknown".to_string(),
        }
    }
}

/// Status of a pull/download operation
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum PullStatus {
    /// Download is starting
    Starting,
    /// Download in progress with percentage
    Downloading { progress: f32, status: String },
    /// Verifying/processing the model
    Processing { status: String },
    /// Download completed successfully
    Completed,
    /// Download failed
    Failed { error: String },
}

/// Trait for model backend implementations
#[allow(dead_code)]
#[async_trait]
pub trait ModelBackend: Send + Sync {
    /// Get the backend type
    fn backend_type(&self) -> BackendType;

    /// List locally available models
    async fn list_local_models(&self) -> Result<Vec<LocalModel>>;

    /// List models available in the registry (popular/recommended)
    async fn list_registry_models(&self) -> Result<Vec<LocalModel>>;

    /// Pull/download a model by name
    async fn pull_model(&self, name: &str) -> Result<()>;

    /// Delete a local model
    async fn delete_model(&self, name: &str) -> Result<()>;

    /// Check if the backend server is running
    async fn is_server_running(&self) -> bool;

    /// Start the backend server (blocks until server is ready)
    async fn start_server(&self) -> Result<()>;

    /// Stop the backend server
    async fn stop_server(&self) -> Result<()>;

    /// Get the OpenAI-compatible API endpoint URL
    fn api_endpoint(&self) -> String;

    /// Check if the backend CLI/tool is installed
    async fn is_installed(&self) -> bool;

    /// Get installation instructions for the backend
    fn install_instructions(&self) -> &'static str;
}

/// Result of checking backend dependencies
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DependencyStatus {
    pub lmstudio_installed: bool,
}

#[allow(dead_code)]
impl DependencyStatus {
    /// Check all dependencies
    pub async fn check() -> Self {
        let lmstudio = Self::check_command("lms", &["--version"]).await;

        Self {
            lmstudio_installed: lmstudio,
        }
    }

    async fn check_command(cmd: &str, args: &[&str]) -> bool {
        tokio::process::Command::new(cmd)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Check if any backend is available
    pub fn any_backend_available(&self) -> bool {
        self.lmstudio_installed
    }

    /// Get a summary of missing dependencies
    pub fn missing_summary(&self) -> Option<String> {
        if self.lmstudio_installed {
            None
        } else {
            Some("Missing: LMStudio (https://lmstudio.ai)".to_string())
        }
    }
}
