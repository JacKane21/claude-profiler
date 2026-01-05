use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct LMStudioModel {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct LMStudioModelsResponse {
    pub data: Vec<LMStudioModel>,
}

pub fn list_local_models() -> Result<Vec<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get("http://localhost:1234/v1/models")
        .send()
        .context("Failed to connect to LMStudio. Is it running? Download from https://lmstudio.ai")?
        .error_for_status()
        .context("LMStudio returned an error response")?;

    let models: LMStudioModelsResponse = response
        .json()
        .context("Failed to parse LMStudio response")?;

    Ok(models.data.into_iter().map(|m| m.id).collect())
}
