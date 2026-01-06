use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::Config;

/// Cache TTL: 15 minutes
const CACHE_TTL_SECS: u64 = 15 * 60;

/// GitHub API for the latest release
const GITHUB_API_RELEASES: &str = "https://api.github.com/repos/openai/codex/releases/latest";

/// Model family for prompt selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamily {
    Gpt52Codex,
    CodexMax,
    Codex,
    Gpt52,
    Gpt51,
}

impl ModelFamily {
    /// Get the prompt file name for this model family
    fn prompt_file(&self) -> &'static str {
        match self {
            ModelFamily::Gpt52Codex => "gpt-5.2-codex_prompt.md",
            ModelFamily::CodexMax => "gpt-5.1-codex-max_prompt.md",
            ModelFamily::Codex => "gpt_5_codex_prompt.md",
            ModelFamily::Gpt52 => "gpt_5_2_prompt.md",
            ModelFamily::Gpt51 => "gpt_5_1_prompt.md",
        }
    }

    /// Get cache file name for this model family
    fn cache_file(&self) -> &'static str {
        match self {
            ModelFamily::Gpt52Codex => "gpt-5.2-codex-instructions.md",
            ModelFamily::CodexMax => "codex-max-instructions.md",
            ModelFamily::Codex => "codex-instructions.md",
            ModelFamily::Gpt52 => "gpt-5.2-instructions.md",
            ModelFamily::Gpt51 => "gpt-5.1-instructions.md",
        }
    }
}

/// Determine model family from model name
pub fn get_model_family(model: &str) -> ModelFamily {
    let normalized = model.to_lowercase();

    // Check more specific patterns first
    if normalized.contains("gpt-5.2-codex") || normalized.contains("gpt 5.2 codex") {
        return ModelFamily::Gpt52Codex;
    }
    if normalized.contains("codex-max") || normalized.contains("codex max") {
        return ModelFamily::CodexMax;
    }
    if normalized.contains("codex") || normalized.starts_with("codex-") {
        return ModelFamily::Codex;
    }
    if normalized.contains("gpt-5.2") || normalized.contains("gpt 5.2") {
        return ModelFamily::Gpt52;
    }
    // Default to gpt-5.1
    ModelFamily::Gpt51
}

/// Cache metadata
#[derive(Debug, Serialize, Deserialize)]
struct CacheMetadata {
    etag: Option<String>,
    tag: String,
    last_checked: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn cache_dir() -> Option<PathBuf> {
    Config::config_dir().map(|p| p.join("cache"))
}

/// Fetch the latest release tag from GitHub
async fn get_latest_release_tag(client: &reqwest::Client) -> Result<String> {
    #[derive(Deserialize)]
    struct GitHubRelease {
        tag_name: Option<String>,
    }

    let response = client
        .get(GITHUB_API_RELEASES)
        .header("User-Agent", "claude-profiler")
        .send()
        .await
        .context("Failed to fetch GitHub releases")?;

    if response.status().is_success() {
        let release: GitHubRelease = response.json().await?;
        if let Some(tag) = release.tag_name {
            return Ok(tag);
        }
    }

    // Fallback: try HTML redirect
    let html_url = "https://github.com/openai/codex/releases/latest";
    let response = client
        .get(html_url)
        .header("User-Agent", "claude-profiler")
        .send()
        .await?;

    let final_url = response.url().to_string();
    if let Some(tag) = final_url.split("/tag/").last() {
        if !tag.contains('/') {
            return Ok(tag.to_string());
        }
    }

    anyhow::bail!("Failed to determine latest release tag")
}


pub async fn get_codex_instructions(model: &str) -> Result<String> {
    let family = get_model_family(model);
    let prompt_file = family.prompt_file();
    let cache_file_name = family.cache_file();

    let Some(cache_path) = cache_dir() else {
        return fetch_instructions_direct(model).await;
    };

    let cache_file = cache_path.join(cache_file_name);
    let meta_file = cache_path.join(format!(
        "{}-meta.json",
        cache_file_name.trim_end_matches(".md")
    ));

    // Check if the cache is still valid (within TTL)
    if let Ok(meta_content) = fs::read_to_string(&meta_file) {
        if let Ok(meta) = serde_json::from_str::<CacheMetadata>(&meta_content) {
            if now_secs().saturating_sub(meta.last_checked) < CACHE_TTL_SECS {
                if let Ok(instructions) = fs::read_to_string(&cache_file) {
                    return Ok(instructions);
                }
            }
        }
    }

    // Fetch fresh instructions
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let tag = match get_latest_release_tag(&client).await {
        Ok(t) => t,
        Err(_) => {
            // Try to use the cached version even if stale
            if let Ok(instructions) = fs::read_to_string(&cache_file) {
                eprintln!("[codex] Using cached instructions (GitHub unreachable)");
                return Ok(instructions);
            }
            anyhow::bail!("Cannot fetch Codex instructions and no cache available")
        }
    };

    let url = format!(
        "https://raw.githubusercontent.com/openai/codex/{}/codex-rs/core/{}",
        tag, prompt_file
    );

    let response = client
        .get(&url)
        .header("User-Agent", "claude-profiler")
        .send()
        .await
        .context("Failed to fetch Codex instructions")?;

    if !response.status().is_success() {
        // Try the cached version
        if let Ok(instructions) = fs::read_to_string(&cache_file) {
            eprintln!("[codex] Using cached instructions (fetch failed)");
            return Ok(instructions);
        }
        anyhow::bail!("Failed to fetch Codex instructions: {}", response.status());
    }

    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let instructions = response.text().await?;

    // Save to cache
    if let Err(e) = fs::create_dir_all(&cache_path) {
        eprintln!("[codex] Failed to create cache dir: {}", e);
    } else {
        if let Err(e) = fs::write(&cache_file, &instructions) {
            eprintln!("[codex] Failed to write cache: {}", e);
        }
        let meta = CacheMetadata {
            etag,
            tag,
            last_checked: now_secs(),
        };
        if let Ok(meta_json) = serde_json::to_string(&meta) {
            let _ = fs::write(&meta_file, meta_json);
        }
    }

    Ok(instructions)
}

/// Direct fetch without caching (fallback)
async fn fetch_instructions_direct(model: &str) -> Result<String> {
    let family = get_model_family(model);
    let prompt_file = family.prompt_file();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let tag = get_latest_release_tag(&client).await?;

    let url = format!(
        "https://raw.githubusercontent.com/openai/codex/{}/codex-rs/core/{}",
        tag, prompt_file
    );

    let response = client
        .get(&url)
        .header("User-Agent", "claude-profiler")
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to fetch Codex instructions: {}", response.status());
    }

    Ok(response.text().await?)
}

/// Default Codex models with reasoning effort variants
fn default_codex_models() -> Vec<String> {
    vec![
        // GPT-5.1 general purpose (supports none/low/medium/high)
        "gpt-5.1".to_string(),
        "gpt-5.1-low".to_string(),
        "gpt-5.1-medium".to_string(),
        "gpt-5.1-high".to_string(),
        // GPT-5.1 Codex (supports low/medium/high)
        "gpt-5.1-codex".to_string(),
        "gpt-5.1-codex-low".to_string(),
        "gpt-5.1-codex-medium".to_string(),
        "gpt-5.1-codex-high".to_string(),
        // GPT-5.1 Codex Max (supports low/medium/high/xhigh)
        "gpt-5.1-codex-max".to_string(),
        "gpt-5.1-codex-max-low".to_string(),
        "gpt-5.1-codex-max-medium".to_string(),
        "gpt-5.1-codex-max-high".to_string(),
        "gpt-5.1-codex-max-xhigh".to_string(),
        // GPT-5.1 Codex Mini (supports medium/high only)
        "gpt-5.1-codex-mini".to_string(),
        "gpt-5.1-codex-mini-medium".to_string(),
        "gpt-5.1-codex-mini-high".to_string(),
        // GPT-5.2 general purpose (supports none/low/medium/high/xhigh)
        "gpt-5.2".to_string(),
        "gpt-5.2-low".to_string(),
        "gpt-5.2-medium".to_string(),
        "gpt-5.2-high".to_string(),
        "gpt-5.2-xhigh".to_string(),
        // GPT-5.2 Codex (supports low/medium/high/xhigh)
        "gpt-5.2-codex".to_string(),
        "gpt-5.2-codex-low".to_string(),
        "gpt-5.2-codex-medium".to_string(),
        "gpt-5.2-codex-high".to_string(),
        "gpt-5.2-codex-xhigh".to_string(),
    ]
}

/// Get available Codex models for UI
pub fn get_cached_codex_models() -> Vec<String> {
    default_codex_models()
}

/// Claude Code bridge prompt - maps Codex tools to Claude Code tools
pub const CLAUDE_CODE_BRIDGE: &str = r#"# Codex Running in Claude Code

You are running Codex through Claude Code. Claude Code provides different tools but follows Codex operating principles.

## CRITICAL: Tool Replacements

<critical_rule priority="0">
apply_patch DOES NOT EXIST -> USE "Edit" INSTEAD
- NEVER use: apply_patch, applyPatch
- ALWAYS use: Edit tool for ALL file modifications
- Before modifying files: Verify you're using "Edit", NOT "apply_patch"
</critical_rule>

<critical_rule priority="0">
update_plan DOES NOT EXIST -> USE "TodoWrite" INSTEAD
- NEVER use: update_plan, updatePlan, read_plan, readPlan
- ALWAYS use: TodoWrite for task/plan updates
- Before plan operations: Verify you're using "TodoWrite", NOT "update_plan"
</critical_rule>

## Available Claude Code Tools

**File Operations:**
- Read - Read file contents (absolute paths required)
- Edit - Modify existing files (REPLACES apply_patch) - requires prior Read
- Write - Create new files (absolute paths required)

**Search/Discovery:**
- Grep - Search file contents with regex
- Glob - Find files by pattern

**Execution:**
- Bash - Run shell commands (use absolute paths, no cd)

**Task Management:**
- TodoWrite - Manage tasks/plans (REPLACES update_plan)

**Advanced:**
- Task - Launch sub-agents for complex work
- WebFetch - Fetch web content

## Substitution Rules

Base instruction says:    You MUST use instead:
apply_patch           ->  Edit
update_plan           ->  TodoWrite
read_plan             ->  (read todo list via TodoWrite)

## Verification Checklist

Before file/plan modifications:
1. Am I using "Edit" NOT "apply_patch"?
2. Am I using "TodoWrite" NOT "update_plan"?
3. Have I read the file before editing?

If ANY answer is NO -> STOP and correct before proceeding.

## What Remains from Codex

Sandbox policies, approval mechanisms, final answer formatting, git commit protocols, and file reference formats all follow Codex instructions."#;
