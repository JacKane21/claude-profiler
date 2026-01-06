use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
};
use base64::Engine as _;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

/// Shared HTTP client for OAuth requests
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

use crate::config::Config;

pub const OPENAI_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const OPENAI_OAUTH_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const OPENAI_OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const OPENAI_OAUTH_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub const OPENAI_OAUTH_SCOPE: &str = "openid profile email offline_access";
pub const OPENAI_OAUTH_CALLBACK_PORT: u16 = 1455;

pub const OPENAI_JWT_CLAIM_PATH: &str = "https://api.openai.com/auth";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiOAuthTokens {
    pub access: String,
    pub refresh: String,
    /// Epoch millis
    pub expires: u64,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn token_file_path() -> Option<PathBuf> {
    Config::config_dir().map(|p| p.join("openai-oauth.json"))
}

/// Check if a string value represents a truthy boolean (1, true, yes, y, on)
pub fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "y" | "on"
    )
}

pub fn openai_oauth_enabled(env_value: Option<&String>) -> bool {
    env_value.map(|s| is_truthy(s)).unwrap_or(false)
}

/// OAuth token response from OpenAI
#[derive(Deserialize)]
struct OAuthTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

pub fn decode_chatgpt_account_id(access_token: &str) -> Option<String> {
    let payload_b64 = access_token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .ok()
        .or_else(|| {
            // Some environments may hand us standard base64.
            base64::engine::general_purpose::STANDARD.decode(payload_b64).ok()
        })?;
    let json: Value = serde_json::from_slice(&decoded).ok()?;
    json.get(OPENAI_JWT_CLAIM_PATH)?
        .get("chatgpt_account_id")?
        .as_str()
        .map(|s| s.to_string())
}

fn parse_authorization_input(input: &str) -> (Option<String>, Option<String>) {
    let value = input.trim();
    if value.is_empty() {
        return (None, None);
    }

    if let Ok(url) = url::Url::parse(value) {
        let code = url
            .query_pairs()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v.to_string());
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.to_string());
        return (code, state);
    }

    if let Some((code, state)) = value.split_once('#') {
        return (Some(code.to_string()), Some(state.to_string()));
    }

    if value.contains("code=") {
        let parsed: Vec<(String, String)> = value
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let code = parsed.iter().find(|(k, _)| k == "code").map(|(_, v)| v.clone());
        let state = parsed
            .iter()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.clone());
        return (code, state);
    }

    (Some(value.to_string()), None)
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

fn generate_pkce() -> (String, String) {
    // RFC 7636 recommends 43-128 chars; we use 32 random bytes base64url.
    let mut verifier_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut verifier_bytes);
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);

    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

    (verifier, challenge)
}

fn build_authorize_url(code_challenge: &str, state: &str) -> Result<String> {
    let mut url = url::Url::parse(OPENAI_OAUTH_AUTHORIZE_URL)?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("response_type", "code");
        qp.append_pair("client_id", OPENAI_OAUTH_CLIENT_ID);
        qp.append_pair("redirect_uri", OPENAI_OAUTH_REDIRECT_URI);
        qp.append_pair("scope", OPENAI_OAUTH_SCOPE);
        qp.append_pair("code_challenge", code_challenge);
        qp.append_pair("code_challenge_method", "S256");
        qp.append_pair("state", state);
        qp.append_pair("id_token_add_organizations", "true");
        qp.append_pair("codex_cli_simplified_flow", "true");
        qp.append_pair("originator", "codex_cli_rs");
    }
    Ok(url.to_string())
}

async fn exchange_authorization_code(code: &str, verifier: &str) -> Result<OpenAiOAuthTokens> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("client_id", OPENAI_OAUTH_CLIENT_ID)
        .append_pair("code", code)
        .append_pair("code_verifier", verifier)
        .append_pair("redirect_uri", OPENAI_OAUTH_REDIRECT_URI)
        .finish();
    let response = HTTP_CLIENT
        .post(OPENAI_OAUTH_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .context("OAuth code->token request failed")?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("OAuth code->token failed: {} {}", status, body);
    }

    let parsed: OAuthTokenResponse =
        serde_json::from_str(&body).context("OAuth code->token response parse failed")?;
    let access = parsed
        .access_token
        .context("OAuth code->token response missing access_token")?;
    let refresh = parsed
        .refresh_token
        .context("OAuth code->token response missing refresh_token")?;
    let expires_in = parsed
        .expires_in
        .context("OAuth code->token response missing expires_in")?;

    Ok(OpenAiOAuthTokens {
        access,
        refresh,
        expires: now_millis() + expires_in * 1000,
    })
}

async fn refresh_access_token(refresh_token: &str) -> Result<OpenAiOAuthTokens> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("refresh_token", refresh_token)
        .append_pair("client_id", OPENAI_OAUTH_CLIENT_ID)
        .finish();
    let response = HTTP_CLIENT
        .post(OPENAI_OAUTH_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .context("OAuth refresh request failed")?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("OAuth refresh failed: {} {}", status, body);
    }

    let parsed: OAuthTokenResponse =
        serde_json::from_str(&body).context("OAuth refresh response parse failed")?;
    let access = parsed
        .access_token
        .context("OAuth refresh response missing access_token")?;
    let refresh = parsed
        .refresh_token
        .context("OAuth refresh response missing refresh_token")?;
    let expires_in = parsed
        .expires_in
        .context("OAuth refresh response missing expires_in")?;

    Ok(OpenAiOAuthTokens {
        access,
        refresh,
        expires: now_millis() + expires_in * 1000,
    })
}

fn load_tokens() -> Result<Option<OpenAiOAuthTokens>> {
    let Some(path) = token_file_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let tokens: OpenAiOAuthTokens =
        serde_json::from_str(&contents).context("Failed to parse openai-oauth.json")?;
    Ok(Some(tokens))
}

pub fn clear_tokens() -> Result<()> {
    if let Some(path) = token_file_path() {
        if path.exists() {
            fs::remove_file(path).context("Failed to delete token file")?;
        }
    }
    Ok(())
}

fn save_tokens(tokens: &OpenAiOAuthTokens) -> Result<()> {
    let Some(path) = token_file_path() else {
        anyhow::bail!("Could not determine config directory for saving tokens");
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let contents = serde_json::to_string_pretty(tokens).context("Failed to serialize tokens")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = fs::OpenOptions::new();
        opts.create(true).truncate(true).write(true).mode(0o600);
        let mut f = opts
            .open(&path)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        f.write_all(contents.as_bytes())?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        fs::write(&path, contents)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }
}

#[derive(Clone)]
struct CallbackState {
    expected_state: String,
    code_tx: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

async fn callback_handler(
    State(st): State<CallbackState>,
    Query(q): Query<CallbackQuery>,
) -> impl IntoResponse {
    let Some(code) = q.code else {
        return (StatusCode::BAD_REQUEST, "Missing code").into_response();
    };
    let Some(state) = q.state else {
        return (StatusCode::BAD_REQUEST, "Missing state").into_response();
    };
    if state != st.expected_state {
        return (StatusCode::BAD_REQUEST, "Invalid state").into_response();
    }

    if let Ok(mut guard) = st.code_tx.lock()
        && let Some(tx) = guard.take()
    {
        let _ = tx.send(code);
    }

    Html(
        r#"<!doctype html>
<html lang="en">
  <head><meta charset="utf-8"><title>OAuth complete</title></head>
  <body>
    <h2>Authentication complete</h2>
    <p>You can close this window and return to your terminal.</p>
  </body>
</html>"#,
    )
    .into_response()
}

async fn wait_for_oauth_code(expected_state: String, timeout: Duration) -> Result<Option<String>> {
    let (code_tx, code_rx) = oneshot::channel::<String>();

    let state = CallbackState {
        expected_state,
        code_tx: Arc::new(Mutex::new(Some(code_tx))),
    };

    let app = Router::new()
        .route("/auth/callback", get(callback_handler))
        .with_state(state);

    let addr = format!("127.0.0.1:{}", OPENAI_OAUTH_CALLBACK_PORT);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind OAuth callback server on {}", addr))?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    let result = tokio::time::timeout(timeout, code_rx).await;
    let _ = shutdown_tx.send(());
    let _ = server.await;

    match result {
        Ok(Ok(code)) => Ok(Some(code)),
        Ok(Err(_)) => Ok(None),
        Err(_) => Ok(None),
    }
}

fn try_open_browser(url: &str) {
    if cfg!(target_os = "windows") {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
        return;
    }

    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };

    let _ = std::process::Command::new(opener).arg(url).spawn();
}

pub async fn ensure_access_token_interactive() -> Result<String> {
    const EXPIRY_SAFETY_WINDOW_MS: u64 = 60_000;

    if let Some(tokens) = load_tokens()? {
        if tokens.expires.saturating_sub(EXPIRY_SAFETY_WINDOW_MS) > now_millis() {
            return Ok(tokens.access);
        }

        if let Ok(refreshed) = refresh_access_token(&tokens.refresh).await {
            save_tokens(&refreshed)?;
            return Ok(refreshed.access);
        }
    }

    let (verifier, challenge) = generate_pkce();
    let state = random_hex(16);
    let authorize_url = build_authorize_url(&challenge, &state)?;

    eprintln!("OpenAI OAuth required. Opening browser for sign-in...");
    eprintln!("If the browser does not open, visit this URL:\n\n{}\n", authorize_url);
    try_open_browser(&authorize_url);

    // Preferred: localhost callback capture. Fallback: manual paste.
    let code = wait_for_oauth_code(state.clone(), Duration::from_secs(300))
        .await
        .ok()
        .flatten();

    let code = if let Some(code) = code {
        code
    } else {
        // Manual fallback: prompt the user to paste redirect URL/code.
        eprint!("Paste the full redirect URL (or just the code): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let (code, got_state) = parse_authorization_input(&input);
        if let Some(got_state) = got_state
            && got_state != state
        {
            anyhow::bail!("OAuth state mismatch");
        }
        code.context("No OAuth code provided")?
    };

    let tokens = exchange_authorization_code(&code, &verifier).await?;
    save_tokens(&tokens)?;
    Ok(tokens.access)
}
