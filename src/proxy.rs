//! Rust-native proxy for translating between Anthropic and OpenAI API formats.
//!
//! This proxy allows Claude Code (which expects Anthropic API) to communicate with
//! LM Studio (which provides OpenAI-compatible API) without requiring Python/LiteLLM.

use anyhow::Result;
use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

/// Default port for the proxy server
pub const PROXY_PORT: u16 = 4000;

/// The base URL that Claude Code should use to connect to the proxy
pub const PROXY_ANTHROPIC_URL: &str = "http://localhost:4000/anthropic";

// ============================================================================
// Anthropic API Types
// ============================================================================

/// Anthropic Messages API request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

/// System prompt can be a string or array of content blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemPrompt {
    Text(String),
    Blocks(Vec<SystemBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
}

/// Anthropic message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

/// Content can be a string or array of content blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// Content block types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// Anthropic Messages API response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub role: String,
    pub content: Vec<ResponseContent>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

// ============================================================================
// OpenAI API Types
// ============================================================================

/// OpenAI Chat Completions request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

/// OpenAI message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<OpenAIContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// OpenAI content can be a string or array
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenAIContent {
    Text(String),
    Parts(Vec<OpenAIContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OpenAIContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIFunction {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: OpenAIFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// OpenAI Chat Completions response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<OpenAIChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAIUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChoice {
    pub index: u32,
    pub message: OpenAIChoiceMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChoiceMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// OpenAI streaming chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIStreamChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<OpenAIStreamChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIStreamChoice {
    pub index: u32,
    pub delta: OpenAIDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIDeltaToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIDeltaToolCall {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<OpenAIDeltaFunction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIDeltaFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ============================================================================
// Translation Logic
// ============================================================================

/// Convert Anthropic request to OpenAI request
pub fn anthropic_to_openai(req: &AnthropicRequest, target_model: &str) -> OpenAIRequest {
    let mut messages = Vec::new();

    // Add system message if present
    if let Some(system) = &req.system {
        let system_text = match system {
            SystemPrompt::Text(text) => text.clone(),
            SystemPrompt::Blocks(blocks) => blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        };
        messages.push(OpenAIMessage {
            role: "system".to_string(),
            content: Some(OpenAIContent::Text(system_text)),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

    // Convert messages
    for msg in &req.messages {
        messages.extend(convert_anthropic_message(msg));
    }

    // Convert tools
    let tools = req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .filter_map(|tool| {
                let name = tool.get("name")?.as_str()?;
                let description = tool.get("description").and_then(|d| d.as_str());
                let input_schema = tool.get("input_schema").cloned();

                Some(OpenAITool {
                    tool_type: "function".to_string(),
                    function: OpenAIFunction {
                        name: name.to_string(),
                        description: description.map(String::from),
                        parameters: input_schema,
                    },
                })
            })
            .collect()
    });

    OpenAIRequest {
        model: target_model.to_string(),
        messages,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        top_p: req.top_p,
        stop: req.stop_sequences.clone(),
        stream: req.stream,
        tools,
        tool_choice: req.tool_choice.clone(),
    }
}

/// Convert a single Anthropic message to OpenAI message(s)
fn convert_anthropic_message(msg: &AnthropicMessage) -> Vec<OpenAIMessage> {
    match &msg.content {
        AnthropicContent::Text(text) => {
            vec![OpenAIMessage {
                role: msg.role.clone(),
                content: Some(OpenAIContent::Text(text.clone())),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }]
        }
        AnthropicContent::Blocks(blocks) => {
            let mut messages = Vec::new();
            let mut text_parts = Vec::new();
            let mut tool_calls = Vec::new();

            for block in blocks {
                match block {
                    ContentBlock::Text { text } => {
                        text_parts.push(OpenAIContentPart::Text { text: text.clone() });
                    }
                    ContentBlock::Image { source } => {
                        let data_url =
                            format!("data:{};base64,{}", source.media_type, source.data);
                        text_parts.push(OpenAIContentPart::ImageUrl {
                            image_url: ImageUrl { url: data_url },
                        });
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(OpenAIToolCall {
                            id: id.clone(),
                            call_type: "function".to_string(),
                            function: OpenAIFunctionCall {
                                name: name.clone(),
                                arguments: serde_json::to_string(input).unwrap_or_default(),
                            },
                        });
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        // Tool results become separate messages
                        let content_str = match content {
                            Value::String(s) => s.clone(),
                            _ => serde_json::to_string(content).unwrap_or_default(),
                        };
                        messages.push(OpenAIMessage {
                            role: "tool".to_string(),
                            content: Some(OpenAIContent::Text(content_str)),
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id.clone()),
                            name: None,
                        });
                    }
                    ContentBlock::Thinking { .. } => {
                        // Skip thinking blocks
                    }
                }
            }

            // Add main message with text/images and tool calls
            if !text_parts.is_empty() || !tool_calls.is_empty() {
                let content = if text_parts.is_empty() {
                    None
                } else if text_parts.len() == 1 {
                    match &text_parts[0] {
                        OpenAIContentPart::Text { text } => Some(OpenAIContent::Text(text.clone())),
                        _ => Some(OpenAIContent::Parts(text_parts.clone())),
                    }
                } else {
                    Some(OpenAIContent::Parts(text_parts))
                };

                messages.insert(
                    0,
                    OpenAIMessage {
                        role: msg.role.clone(),
                        content,
                        tool_calls: if tool_calls.is_empty() {
                            None
                        } else {
                            Some(tool_calls)
                        },
                        tool_call_id: None,
                        name: None,
                    },
                );
            }

            messages
        }
    }
}

/// Convert OpenAI response to Anthropic response
pub fn openai_to_anthropic(resp: &OpenAIResponse, original_model: &str) -> AnthropicResponse {
    let choice = resp.choices.first();

    let mut content = Vec::new();
    let mut stop_reason = None;

    if let Some(c) = choice {
        // Add text content
        if let Some(text) = &c.message.content {
            if !text.is_empty() {
                content.push(ResponseContent::Text { text: text.clone() });
            }
        }

        // Add tool calls
        if let Some(tool_calls) = &c.message.tool_calls {
            for tc in tool_calls {
                let input: Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
                content.push(ResponseContent::ToolUse {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    input,
                });
            }
        }

        // Map finish reason
        stop_reason = c.finish_reason.as_ref().map(|r| match r.as_str() {
            "stop" => "end_turn".to_string(),
            "length" => "max_tokens".to_string(),
            "tool_calls" => "tool_use".to_string(),
            "content_filter" => "content_filter".to_string(),
            other => other.to_string(),
        });
    }

    let usage = resp.usage.as_ref().map_or(
        AnthropicUsage {
            input_tokens: 0,
            output_tokens: 0,
        },
        |u| AnthropicUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        },
    );

    AnthropicResponse {
        id: format!("msg_{}", resp.id),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: original_model.to_string(),
        stop_reason,
        stop_sequence: None,
        usage,
    }
}

// ============================================================================
// Proxy Server
// ============================================================================

/// Shared state for the proxy server
#[derive(Clone)]
pub struct ProxyState {
    pub client: reqwest::Client,
    pub lmstudio_url: String,
    pub target_model: String,
}

/// Start the proxy server
pub async fn start_server(lmstudio_model: String) -> Result<()> {
    let state = Arc::new(ProxyState {
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?,
        lmstudio_url: "http://localhost:1234/v1/chat/completions".to_string(),
        target_model: lmstudio_model,
    });

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/v1/messages", post(messages_handler))
        .route("/anthropic/v1/messages", post(messages_handler))
        .fallback(fallback_handler)
        .with_state(state);

    let addr = format!("127.0.0.1:{}", PROXY_PORT);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    axum::serve(listener, app).await?;

    Ok(())
}

/// Health check endpoint
async fn health_handler() -> &'static str {
    "OK"
}

/// Fallback handler to log unmatched routes
async fn fallback_handler(req: axum::extract::Request) -> Response {
    let uri = req.uri().clone();

    // Silently accept event logging requests (telemetry)
    if uri.path().contains("event_logging") {
        return StatusCode::OK.into_response();
    }

    (StatusCode::NOT_FOUND, format!("Not found: {}", uri)).into_response()
}

/// Main messages endpoint - handles Anthropic API requests
async fn messages_handler(
    State(state): State<Arc<ProxyState>>,
    Json(request): Json<AnthropicRequest>,
) -> Response {
    let original_model = request.model.clone();
    let is_streaming = request.stream.unwrap_or(false);

    // Convert to OpenAI format
    let openai_request = anthropic_to_openai(&request, &state.target_model);

    if is_streaming {
        handle_streaming_request(state, openai_request, original_model).await
    } else {
        handle_non_streaming_request(state, openai_request, original_model).await
    }
}

/// Handle non-streaming request
async fn handle_non_streaming_request(
    state: Arc<ProxyState>,
    request: OpenAIRequest,
    original_model: String,
) -> Response {
    let response = state
        .client
        .post(&state.lmstudio_url)
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await;

    match response {
        Ok(resp) => {
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return (
                    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    body,
                )
                    .into_response();
            }

            match resp.json::<OpenAIResponse>().await {
                Ok(openai_resp) => {
                    let anthropic_resp = openai_to_anthropic(&openai_resp, &original_model);
                    Json(anthropic_resp).into_response()
                }
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("Parse error: {}", e))
                        .into_response()
                }
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("Failed to connect to LM Studio: {}", e),
        )
            .into_response(),
    }
}

/// Handle streaming request
async fn handle_streaming_request(
    state: Arc<ProxyState>,
    request: OpenAIRequest,
    original_model: String,
) -> Response {
    let response = state
        .client
        .post(&state.lmstudio_url)
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await;

    match response {
        Ok(resp) => {
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return (
                    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    body,
                )
                    .into_response();
            }

            // Create SSE stream
            let byte_stream = resp.bytes_stream();
            let stream = create_anthropic_stream(byte_stream, original_model);

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .header(header::CONNECTION, "keep-alive")
                .body(Body::from_stream(stream))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("Failed to connect to LM Studio: {}", e),
        )
            .into_response(),
    }
}

/// Create an Anthropic-format SSE stream from OpenAI stream
fn create_anthropic_stream(
    byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    model: String,
) -> impl Stream<Item = Result<String, Infallible>> + Send + 'static {
    use futures::StreamExt;

    let mut message_started = false;
    let mut content_block_started = false;
    let mut buffer = String::new();
    let input_tokens = 0u32;
    let mut output_tokens = 0u32;

    async_stream::stream! {
        let model = model.clone();
        let msg_id = format!("msg_{}", uuid_simple());

        futures::pin_mut!(byte_stream);

        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(bytes) => {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                    // Process complete SSE lines
                    while let Some(line_end) = buffer.find('\n') {
                        let line = buffer[..line_end].trim().to_string();
                        buffer = buffer[line_end + 1..].to_string();

                        if line.is_empty() || !line.starts_with("data: ") {
                            continue;
                        }

                        let data = &line[6..];
                        if data == "[DONE]" {
                            // End content block
                            if content_block_started {
                                yield Ok(format!("event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n"));
                            }

                            // End message
                            yield Ok(format!(
                                "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":{}}}}}\n\n",
                                output_tokens
                            ));
                            yield Ok("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string());
                            continue;
                        }

                        // Parse OpenAI chunk
                        if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(data) {
                            // Send message_start on first chunk
                            if !message_started {
                                message_started = true;
                                yield Ok(format!(
                                    "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"{}\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"{}\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{{\"input_tokens\":{},\"output_tokens\":0}}}}}}\n\n",
                                    msg_id, model, input_tokens
                                ));
                            }

                            for choice in &chunk.choices {
                                // Handle text content
                                if let Some(content) = &choice.delta.content {
                                    if !content.is_empty() {
                                        // Start content block if needed
                                        if !content_block_started {
                                            content_block_started = true;
                                            yield Ok("event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string());
                                        }

                                        output_tokens += 1; // Rough estimate

                                        let escaped = escape_json_string(content);
                                        yield Ok(format!(
                                            "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{}\"}}}}\n\n",
                                            escaped
                                        ));
                                    }
                                }

                                // Handle tool calls
                                if let Some(tool_calls) = &choice.delta.tool_calls {
                                    for tc in tool_calls {
                                        if let (Some(id), Some(func)) = (&tc.id, &tc.function) {
                                            if let Some(name) = &func.name {
                                                // Start tool use block
                                                yield Ok(format!(
                                                    "event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":{},\"content_block\":{{\"type\":\"tool_use\",\"id\":\"{}\",\"name\":\"{}\",\"input\":{{}}}}}}\n\n",
                                                    tc.index, id, name
                                                ));
                                            }
                                        }
                                        if let Some(func) = &tc.function {
                                            if let Some(args) = &func.arguments {
                                                if !args.is_empty() {
                                                    let escaped = escape_json_string(args);
                                                    yield Ok(format!(
                                                        "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":{},\"delta\":{{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}}}\n\n",
                                                        tc.index, escaped
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }

                                // Handle finish reason
                                if choice.finish_reason.is_some() {
                                    if content_block_started {
                                        yield Ok(format!("event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n"));
                                        content_block_started = false;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    break;
                }
            }
        }
    }
}

/// Simple UUID-like string generator
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:x}{:x}", now.as_secs(), now.subsec_nanos())
}

/// Escape a string for JSON embedding
fn escape_json_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

