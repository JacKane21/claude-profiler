//! Rust-native proxy for translating between Anthropic and OpenAI API formats.
//!
//! This proxy allows Claude Code (which expects Anthropic API) to communicate with
//! LM Studio (which provides OpenAI-compatible API) without requiring Python/LiteLLM.

use anyhow::Result;
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
}

/// Anthropic extended thinking configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ThinkingConfig {
    #[serde(rename = "enabled")]
    Enabled { budget_tokens: Option<u32> },
    #[serde(rename = "disabled")]
    Disabled,
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
    #[serde(rename = "redacted_thinking")]
    RedactedThinking { data: String },
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
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    #[serde(rename = "redacted_thinking")]
    RedactedThinking { data: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

// ============================================================================
// OpenAI Responses API Types
// ============================================================================

/// OpenAI Responses request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: Vec<ResponseInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponseTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponseReasoning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// Responses input item
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseInputItem {
    #[serde(rename = "message")]
    Message {
        role: String,
        content: Vec<ResponseInputContentPart>,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(rename = "call_id")]
        call_id: String,
        name: String,
        arguments: String,
    },
    #[serde(rename = "function_call_output")]
    FunctionCallOutput {
        #[serde(rename = "call_id")]
        call_id: String,
        output: String,
    },
}

/// Responses input content part
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseInputContentPart {
    #[serde(rename = "input_text")]
    InputText { text: String },
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "input_image")]
    InputImage { image_url: ResponseImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseTool {
    #[serde(rename = "function")]
    Function {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parameters: Option<Value>,
    },
    #[serde(rename = "mcp")]
    Mcp {
        server_label: String,
        server_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        allowed_tools: Option<Vec<String>>,
    },
}

/// OpenAI Responses response (partial)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub model: String,
    pub output: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Value>,
}

// ============================================================================
// Translation Logic
// ============================================================================

/// Convert Anthropic request to OpenAI Responses request
pub fn anthropic_to_responses(req: &AnthropicRequest, target_model: &str) -> ResponsesRequest {
    let mut input = Vec::new();

    // Convert messages
    for msg in &req.messages {
        input.extend(convert_anthropic_message(msg));
    }

    // Add system prompt as instructions if present
    let instructions = req.system.as_ref().map(|system| match system {
        SystemPrompt::Text(text) => text.clone(),
        SystemPrompt::Blocks(blocks) => blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    });

    // Convert tools
    let tools = req.tools.as_ref().and_then(|tools| {
        let mapped: Vec<ResponseTool> = tools
            .iter()
            .filter_map(|tool| {
                let name = tool.get("name")?.as_str()?;
                let description = tool.get("description").and_then(|d| d.as_str());
                let input_schema = tool.get("input_schema").cloned();

                Some(ResponseTool::Function {
                    name: name.to_string(),
                    description: description.map(String::from),
                    parameters: input_schema,
                })
            })
            .collect();

        if mapped.is_empty() {
            None
        } else {
            Some(mapped)
        }
    });

    let reasoning = match &req.thinking {
        Some(ThinkingConfig::Enabled { budget_tokens }) => {
            let effort = match budget_tokens {
                Some(budget) if *budget >= 4096 => "high",
                Some(budget) if *budget >= 1024 => "medium",
                Some(_) => "low",
                None => "medium",
            };
            Some(ResponseReasoning {
                effort: Some(effort.to_string()),
            })
        }
        _ => None,
    };

    ResponsesRequest {
        model: target_model.to_string(),
        input,
        instructions,
        max_output_tokens: req.max_tokens,
        temperature: req.temperature,
        top_p: req.top_p,
        stream: req.stream,
        tools,
        tool_choice: req.tool_choice.clone(),
        reasoning,
    }
}

/// Convert a single Anthropic message to OpenAI Responses input items
fn convert_anthropic_message(msg: &AnthropicMessage) -> Vec<ResponseInputItem> {
    match &msg.content {
        AnthropicContent::Text(text) => {
            let part = if msg.role == "assistant" {
                ResponseInputContentPart::OutputText { text: text.clone() }
            } else {
                ResponseInputContentPart::InputText { text: text.clone() }
            };
            vec![ResponseInputItem::Message {
                role: msg.role.clone(),
                content: vec![part],
            }]
        }
        AnthropicContent::Blocks(blocks) => {
            let mut items = Vec::new();
            let mut content_parts = Vec::new();

            let flush_message =
                |items: &mut Vec<ResponseInputItem>,
                 content_parts: &mut Vec<ResponseInputContentPart>| {
                    if !content_parts.is_empty() {
                        let parts = std::mem::take(content_parts);
                        items.push(ResponseInputItem::Message {
                            role: msg.role.clone(),
                            content: parts,
                        });
                    }
                };

            for block in blocks {
                match block {
                    ContentBlock::Text { text } => {
                        let part = if msg.role == "assistant" {
                            ResponseInputContentPart::OutputText { text: text.clone() }
                        } else {
                            ResponseInputContentPart::InputText { text: text.clone() }
                        };
                        content_parts.push(part);
                    }
                    ContentBlock::Image { source } => {
                        let data_url = format!("data:{};base64,{}", source.media_type, source.data);
                        if msg.role != "assistant" {
                            content_parts.push(ResponseInputContentPart::InputImage {
                                image_url: ResponseImageUrl { url: data_url },
                            });
                        }
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        flush_message(&mut items, &mut content_parts);
                        items.push(ResponseInputItem::FunctionCall {
                            id: Some(id.clone()),
                            call_id: id.clone(),
                            name: name.clone(),
                            arguments: serde_json::to_string(input).unwrap_or_default(),
                        });
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        flush_message(&mut items, &mut content_parts);
                        let content_str = match content {
                            Value::String(s) => s.clone(),
                            _ => serde_json::to_string(content).unwrap_or_default(),
                        };
                        items.push(ResponseInputItem::FunctionCallOutput {
                            call_id: tool_use_id.clone(),
                            output: content_str,
                        });
                    }
                    ContentBlock::Thinking { .. } => {
                        // Skip thinking blocks
                    }
                    ContentBlock::RedactedThinking { .. } => {
                        // Skip redacted thinking blocks
                    }
                }
            }

            flush_message(&mut items, &mut content_parts);

            items
        }
    }
}

/// Convert OpenAI Responses response to Anthropic response
pub fn responses_to_anthropic(
    resp: &ResponsesResponse,
    original_model: &str,
    include_thinking: bool,
) -> AnthropicResponse {
    let mut content = Vec::new();

    for item in &resp.output {
        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

        if item_type == "message" {
            let role = item.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role != "assistant" {
                continue;
            }
            if let Some(parts) = item.get("content").and_then(|c| c.as_array()) {
                for part in parts {
                    if part.get("type").and_then(|t| t.as_str()) == Some("output_text")
                        && let Some(text) = part.get("text").and_then(|t| t.as_str())
                        && !text.is_empty()
                    {
                        content.push(ResponseContent::Text {
                            text: text.to_string(),
                        });
                    }
                }
            }
        } else if item_type == "function_call" {
            let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let call_id = item
                .get("call_id")
                .and_then(|c| c.as_str())
                .or_else(|| item.get("id").and_then(|c| c.as_str()))
                .unwrap_or("call");
            let arguments = item.get("arguments").and_then(|a| a.as_str()).unwrap_or("");
            let input: Value =
                serde_json::from_str(arguments).unwrap_or(Value::String(arguments.to_string()));
            content.push(ResponseContent::ToolUse {
                id: call_id.to_string(),
                name: name.to_string(),
                input,
            });
        } else if item_type == "reasoning"
            && include_thinking
            && let Some(thinking) = extract_reasoning_text(item)
            && !thinking.is_empty()
        {
            content.push(ResponseContent::Thinking {
                thinking,
                signature: None,
            });
        }
    }

    let usage = resp.usage.as_ref().map_or(
        AnthropicUsage {
            input_tokens: 0,
            output_tokens: 0,
        },
        |u| AnthropicUsage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        },
    );

    AnthropicResponse {
        id: format!("msg_{}", resp.id),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: original_model.to_string(),
        stop_reason: Some("end_turn".to_string()),
        stop_sequence: None,
        usage,
    }
}

fn extract_reasoning_text(item: &Value) -> Option<String> {
    if let Some(parts) = item.get("content").and_then(|c| c.as_array()) {
        let mut combined = String::new();
        for part in parts {
            let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if part_type == "reasoning_text"
                && let Some(text) = part.get("text").and_then(|t| t.as_str())
            {
                combined.push_str(text);
            }
        }
        if !combined.is_empty() {
            return Some(combined);
        }
    }

    if let Some(summary) = item.get("summary").and_then(|s| s.as_str())
        && !summary.is_empty()
    {
        return Some(summary.to_string());
    }

    None
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
    /// Optional auxiliary model for handling lightweight requests
    /// (token counting, suggestions, etc.)
    pub auxiliary_model: Option<String>,
}

/// Detect if a request is an auxiliary request that should use a smaller/faster model
fn is_auxiliary_request(request: &AnthropicRequest) -> bool {
    // Check for token counting (max_tokens: 1 is a strong signal)
    if request.max_tokens == Some(1) {
        return true;
    }

    // Check for suggestion mode or other auxiliary patterns in message content
    for msg in &request.messages {
        match &msg.content {
            AnthropicContent::Blocks(blocks) => {
                for block in blocks {
                    if let ContentBlock::Text { text } = block
                        && text.contains("[SUGGESTION MODE:")
                    {
                        return true;
                    }
                }
            }
            AnthropicContent::Text(text) => {
                if text.contains("[SUGGESTION MODE:") {
                    return true;
                }
            }
        }
    }

    // Check for JSON prefill (assistant starts with '{' without tools)
    // This indicates structured output parsing which is typically lightweight
    let has_no_tools =
        request.tools.is_none() || request.tools.as_ref().map(|t| t.is_empty()).unwrap_or(true);

    if has_no_tools
        && let Some(last_msg) = request.messages.last()
        && last_msg.role == "assistant"
    {
        let starts_with_brace = match &last_msg.content {
            AnthropicContent::Text(text) => text.trim_start().starts_with('{'),
            AnthropicContent::Blocks(blocks) => blocks.iter().any(|b| {
                if let ContentBlock::Text { text } = b {
                    text.trim_start().starts_with('{')
                } else {
                    false
                }
            }),
        };
        if starts_with_brace {
            return true;
        }
    }

    false
}

/// Start the proxy server
pub async fn start_server(lmstudio_model: String, auxiliary_model: Option<String>) -> Result<()> {
    if let Some(ref aux) = auxiliary_model {
        eprintln!(
            "Proxy: Using auxiliary model '{}' for lightweight requests",
            aux
        );
    }

    let state = Arc::new(ProxyState {
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?,
        lmstudio_url: "http://localhost:1234/v1/responses".to_string(),
        target_model: lmstudio_model,
        auxiliary_model,
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
    let include_thinking = matches!(request.thinking, Some(ThinkingConfig::Enabled { .. }));

    // Determine which model to use based on request type
    let target_model = if is_auxiliary_request(&request) {
        state
            .auxiliary_model
            .as_ref()
            .unwrap_or(&state.target_model)
    } else {
        &state.target_model
    };

    // Convert to OpenAI Responses format
    let openai_request = anthropic_to_responses(&request, target_model);

    if is_streaming {
        handle_streaming_request(state, openai_request, original_model, include_thinking).await
    } else {
        handle_non_streaming_request(state, openai_request, original_model, include_thinking).await
    }
}

/// Handle non-streaming request
async fn handle_non_streaming_request(
    state: Arc<ProxyState>,
    request: ResponsesRequest,
    original_model: String,
    include_thinking: bool,
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
                    StatusCode::from_u16(status.as_u16())
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    body,
                )
                    .into_response();
            }

            match resp.json::<ResponsesResponse>().await {
                Ok(openai_resp) => {
                    let anthropic_resp =
                        responses_to_anthropic(&openai_resp, &original_model, include_thinking);
                    Json(anthropic_resp).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Parse error: {}", e),
                )
                    .into_response(),
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
    request: ResponsesRequest,
    original_model: String,
    include_thinking: bool,
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
                    StatusCode::from_u16(status.as_u16())
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    body,
                )
                    .into_response();
            }

            // Create SSE stream
            let byte_stream = resp.bytes_stream();
            let stream = create_anthropic_stream(byte_stream, original_model, include_thinking);

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

/// Create an Anthropic-format SSE stream from OpenAI Responses stream
fn create_anthropic_stream(
    byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    model: String,
    include_thinking: bool,
) -> impl Stream<Item = Result<String, Infallible>> + Send + 'static {
    use futures::StreamExt;

    let mut buffer = String::new();
    let mut state = StreamState::new();

    async_stream::stream! {
        let msg_id = format!("msg_{}", uuid_simple());
        let model = model.clone();

        futures::pin_mut!(byte_stream);

        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(bytes) => {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                    // Process complete SSE lines
                    while let Some(line) = drain_sse_line(&mut buffer) {
                        let line = line.trim_end_matches('\r');

                        if line.is_empty() || line.starts_with("event:") {
                            continue;
                        }
                        let data = match line.strip_prefix("data: ") {
                            Some(data) => data,
                            None => continue,
                        };
                        if data == "[DONE]" {
                            if let Some(start) = state.ensure_message_started(&msg_id, &model) {
                                yield Ok(start);
                            }
                            for event in state.finish_message() {
                                yield Ok(event);
                            }
                            continue;
                        }

                        let event: Value = match serde_json::from_str(data) {
                            Ok(value) => value,
                            Err(_) => continue,
                        };

                        let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

                        match event_type {
                            "response.output_text.delta" => {
                                if let Some(start) = state.ensure_message_started(&msg_id, &model) {
                                    yield Ok(start);
                                }
                                if let Some(content) = event.get("delta").and_then(|d| d.as_str())
                                    && !content.is_empty()
                                {
                                    if let Some(stop) = state.close_thinking_block() {
                                        yield Ok(stop);
                                    }
                                    if let Some(start) = state.ensure_text_block_started() {
                                        yield Ok(start);
                                    }

                                    state.output_tokens += 1;
                                    let escaped = escape_json_string(content);
                                    if let Some(index) = state.text_block_index {
                                        yield Ok(event_text_delta(index, &escaped));
                                    }
                                }
                            }
                            "response.reasoning_text.delta" if include_thinking => {
                                if let Some(start) = state.ensure_message_started(&msg_id, &model) {
                                    yield Ok(start);
                                }
                                if let Some(reasoning) = event.get("delta").and_then(|d| d.as_str())
                                    && !reasoning.is_empty()
                                {
                                    if let Some(start) = state.ensure_thinking_block_started() {
                                        yield Ok(start);
                                    }
                                    if state.thinking_block_open {
                                        state.output_tokens += 1;
                                        let escaped = escape_json_string(reasoning);
                                        if let Some(index) = state.thinking_block_index {
                                            yield Ok(event_thinking_delta(index, &escaped));
                                        }
                                    }
                                }
                            }
                            "response.output_item.added" => {
                                if let Some(start) = state.ensure_message_started(&msg_id, &model) {
                                    yield Ok(start);
                                }

                                if let Some(output_index) = output_index(&event)
                                    && let Some(item) = event.get("item")
                                {
                                    let item_type =
                                        item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    if item_type == "function_call" {
                                        state.capture_tool_metadata(output_index, item);
                                        if let Some(start) = state.ensure_tool_block_open(output_index)
                                        {
                                            yield Ok(start);
                                        }
                                        let block_index = state.tool_block_index(output_index);

                                        if let Some(arguments) =
                                            item.get("arguments").and_then(|v| v.as_str())
                                            && !arguments.is_empty()
                                        {
                                            let escaped = escape_json_string(arguments);
                                            yield Ok(event_tool_args_delta(block_index, &escaped));
                                            state.tool_args_emitted.insert(output_index);
                                        }

                                        if let Some(pending) =
                                            state.pending_tool_args.remove(&output_index)
                                            && !pending.is_empty()
                                        {
                                            let escaped = escape_json_string(&pending);
                                            yield Ok(event_tool_args_delta(block_index, &escaped));
                                            state.tool_args_emitted.insert(output_index);
                                        }
                                    }
                                }
                            }
                            "response.function_call_arguments.delta" => {
                                if let Some(start) = state.ensure_message_started(&msg_id, &model) {
                                    yield Ok(start);
                                }
                                if let (Some(output_index), Some(delta)) = (
                                    output_index(&event),
                                    event.get("delta").and_then(|d| d.as_str()),
                                )
                                    && !delta.is_empty()
                                {
                                    if let Some(block_index) =
                                        state.tool_block_indices.get(&output_index)
                                    {
                                        if state.tool_blocks_open.contains(&output_index) {
                                            let escaped = escape_json_string(delta);
                                            yield Ok(event_tool_args_delta(*block_index, &escaped));
                                            state.tool_args_emitted.insert(output_index);
                                        } else {
                                            state.pending_tool_args
                                                .entry(output_index)
                                                .and_modify(|s| s.push_str(delta))
                                                .or_insert_with(|| delta.to_string());
                                        }
                                    } else {
                                        state.pending_tool_args
                                            .entry(output_index)
                                            .and_modify(|s| s.push_str(delta))
                                            .or_insert_with(|| delta.to_string());
                                    }
                                }
                            }
                            "response.function_call_arguments.done" => {
                                if let Some(start) = state.ensure_message_started(&msg_id, &model) {
                                    yield Ok(start);
                                }
                                if let Some(output_index) = output_index(&event) {
                                    state.capture_tool_metadata(output_index, &event);
                                    if let Some(start) = state.ensure_tool_block_open(output_index) {
                                        yield Ok(start);
                                    }
                                    let block_index = state.tool_block_index(output_index);

                                    if !state.tool_args_emitted.contains(&output_index)
                                        && let Some(arguments) =
                                            event.get("arguments").and_then(|a| a.as_str())
                                        && !arguments.is_empty()
                                    {
                                        let escaped = escape_json_string(arguments);
                                        yield Ok(event_tool_args_delta(block_index, &escaped));
                                    }

                                    if let Some(pending) =
                                        state.pending_tool_args.remove(&output_index)
                                        && !pending.is_empty()
                                    {
                                        let escaped = escape_json_string(&pending);
                                        yield Ok(event_tool_args_delta(block_index, &escaped));
                                    }
                                }
                            }
                            "response.output_item.done" => {
                                if let (Some(output_index), Some(item)) = (output_index(&event), event.get("item")) {
                                    let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    if item_type == "function_call" {
                                        if let Some(index) = state.tool_block_indices.get(&output_index) {
                                            yield Ok(event_content_block_stop(*index));
                                        }
                                        state.tool_blocks_open.remove(&output_index);
                                    }
                                }
                            }
                            "response.completed" | "response.failed" => {
                                if let Some(start) = state.ensure_message_started(&msg_id, &model) {
                                    yield Ok(start);
                                }
                                for event in state.finish_message() {
                                    yield Ok(event);
                                }
                            }
                            _ => {}
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

fn output_index(event: &Value) -> Option<u32> {
    event
        .get("output_index")
        .and_then(|i| i.as_u64())
        .map(|v| v as u32)
}

fn drain_sse_line(buffer: &mut String) -> Option<String> {
    let newline = buffer.find('\n')?;
    let line = buffer[..newline].to_string();
    buffer.drain(..=newline);
    Some(line)
}

fn event_content_block_stop(index: usize) -> String {
    format!(
        "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n",
        index
    )
}

fn event_text_block_start(index: usize) -> String {
    format!(
        "event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":{},\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n",
        index
    )
}

fn event_thinking_block_start(index: usize) -> String {
    format!(
        "event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":{},\"content_block\":{{\"type\":\"thinking\",\"thinking\":\"\"}}}}\n\n",
        index
    )
}

fn event_tool_block_start(index: usize, id: &str, name: &str) -> String {
    format!(
        "event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":{},\"content_block\":{{\"type\":\"tool_use\",\"id\":\"{}\",\"name\":\"{}\",\"input\":{{}}}}}}\n\n",
        index, id, name
    )
}

fn event_text_delta(index: usize, text: &str) -> String {
    format!(
        "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":{},\"delta\":{{\"type\":\"text_delta\",\"text\":\"{}\"}}}}\n\n",
        index, text
    )
}

fn event_thinking_delta(index: usize, thinking: &str) -> String {
    format!(
        "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":{},\"delta\":{{\"type\":\"thinking_delta\",\"thinking\":\"{}\"}}}}\n\n",
        index, thinking
    )
}

fn event_tool_args_delta(index: usize, args: &str) -> String {
    format!(
        "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":{},\"delta\":{{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}}}\n\n",
        index, args
    )
}

fn event_message_delta(output_tokens: u32) -> String {
    format!(
        "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":{}}}}}\n\n",
        output_tokens
    )
}

fn event_message_stop() -> String {
    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string()
}

#[derive(Debug, Default)]
struct StreamState {
    message_started: bool,
    input_tokens: u32,
    output_tokens: u32,
    next_block_index: usize,
    thinking_block_index: Option<usize>,
    thinking_block_open: bool,
    text_block_index: Option<usize>,
    text_block_open: bool,
    tool_block_indices: HashMap<u32, usize>,
    tool_blocks_open: HashSet<u32>,
    tool_call_ids: HashMap<u32, String>,
    tool_call_names: HashMap<u32, String>,
    pending_tool_args: HashMap<u32, String>,
    tool_args_emitted: HashSet<u32>,
}

impl StreamState {
    fn new() -> Self {
        Self {
            input_tokens: 0,
            ..Self::default()
        }
    }

    fn ensure_message_started(&mut self, msg_id: &str, model: &str) -> Option<String> {
        if self.message_started {
            return None;
        }
        self.message_started = true;
        Some(format!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"{}\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"{}\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{{\"input_tokens\":{},\"output_tokens\":0}}}}}}\n\n",
            msg_id, model, self.input_tokens
        ))
    }

    fn ensure_text_block_started(&mut self) -> Option<String> {
        if self.text_block_index.is_none() {
            let index = self.next_block_index;
            self.next_block_index += 1;
            self.text_block_index = Some(index);
            self.text_block_open = true;
            return Some(event_text_block_start(index));
        }
        None
    }

    fn ensure_thinking_block_started(&mut self) -> Option<String> {
        if self.thinking_block_index.is_none() {
            let index = self.next_block_index;
            self.next_block_index += 1;
            self.thinking_block_index = Some(index);
            self.thinking_block_open = true;
            return Some(event_thinking_block_start(index));
        }
        None
    }

    fn close_thinking_block(&mut self) -> Option<String> {
        if self.thinking_block_open {
            self.thinking_block_open = false;
            if let Some(index) = self.thinking_block_index {
                return Some(event_content_block_stop(index));
            }
        }
        None
    }

    fn close_text_block(&mut self) -> Option<String> {
        if self.text_block_open {
            self.text_block_open = false;
            if let Some(index) = self.text_block_index {
                return Some(event_content_block_stop(index));
            }
        }
        None
    }

    fn close_open_tool_blocks(&mut self) -> Vec<String> {
        let mut events = Vec::new();
        for slot in &self.tool_blocks_open {
            if let Some(index) = self.tool_block_indices.get(slot) {
                events.push(event_content_block_stop(*index));
            }
        }
        self.tool_blocks_open.clear();
        events
    }

    fn finish_message(&mut self) -> Vec<String> {
        let mut events = self.close_open_tool_blocks();
        if let Some(stop) = self.close_text_block() {
            events.push(stop);
        }
        if let Some(stop) = self.close_thinking_block() {
            events.push(stop);
        }
        events.push(event_message_delta(self.output_tokens));
        events.push(event_message_stop());
        events
    }

    fn tool_block_index(&mut self, output_index: u32) -> usize {
        *self
            .tool_block_indices
            .entry(output_index)
            .or_insert_with(|| {
                let index = self.next_block_index;
                self.next_block_index += 1;
                index
            })
    }

    fn capture_tool_metadata(&mut self, output_index: u32, item: &Value) {
        if let Some(id) = item
            .get("call_id")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("item_id").and_then(|v| v.as_str()))
            .or_else(|| item.get("id").and_then(|v| v.as_str()))
        {
            self.tool_call_ids
                .entry(output_index)
                .or_insert_with(|| id.to_string());
        }
        if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
            self.tool_call_names
                .entry(output_index)
                .or_insert_with(|| name.to_string());
        }
    }

    fn ensure_tool_block_open(&mut self, output_index: u32) -> Option<String> {
        if self.tool_blocks_open.contains(&output_index) {
            return None;
        }
        let (id, name) = match (
            self.tool_call_ids.get(&output_index),
            self.tool_call_names.get(&output_index),
        ) {
            (Some(id), Some(name)) => (id.to_string(), name.to_string()),
            _ => return None,
        };
        let index = self.tool_block_index(output_index);
        self.tool_blocks_open.insert(output_index);
        Some(event_tool_block_start(index, &id, &name))
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
    let mut out = String::with_capacity(s.len() + s.len() / 4);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures::{StreamExt, stream};
    use serde_json::json;

    fn base_request(messages: Vec<AnthropicMessage>) -> AnthropicRequest {
        AnthropicRequest {
            model: "claude".to_string(),
            messages,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            stream: None,
            system: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        }
    }

    #[test]
    fn escape_json_string_escapes_control_chars() {
        let input = "a\"b\\c\n\r\t\u{0001}";
        let escaped = escape_json_string(input);
        assert_eq!(escaped, "a\\\"b\\\\c\\n\\r\\t\\u0001");
    }

    #[test]
    fn is_auxiliary_request_detects_patterns() {
        let req = base_request(vec![AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicContent::Text("hello".to_string()),
        }]);
        assert!(!is_auxiliary_request(&req));

        let req = AnthropicRequest {
            max_tokens: Some(1),
            ..base_request(vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("hello".to_string()),
            }])
        };
        assert!(is_auxiliary_request(&req));

        let req = base_request(vec![AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicContent::Text("[SUGGESTION MODE: ON]".to_string()),
        }]);
        assert!(is_auxiliary_request(&req));

        let req = base_request(vec![AnthropicMessage {
            role: "assistant".to_string(),
            content: AnthropicContent::Text("{\"ok\":true}".to_string()),
        }]);
        assert!(is_auxiliary_request(&req));
    }

    #[test]
    fn anthropic_to_responses_maps_system_and_tools() {
        let req = AnthropicRequest {
            model: "claude".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("hi".to_string()),
            }],
            max_tokens: Some(10),
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: None,
            stop_sequences: None,
            stream: Some(false),
            system: Some(SystemPrompt::Blocks(vec![
                SystemBlock {
                    block_type: "text".to_string(),
                    text: "sys1".to_string(),
                },
                SystemBlock {
                    block_type: "text".to_string(),
                    text: "sys2".to_string(),
                },
            ])),
            tools: Some(vec![json!({
                "name": "tool1",
                "description": "desc",
                "input_schema": {"type": "object"}
            })]),
            tool_choice: Some(json!("auto")),
            thinking: Some(ThinkingConfig::Enabled {
                budget_tokens: Some(1500),
            }),
        };

        let mapped = anthropic_to_responses(&req, "target");
        assert_eq!(mapped.model, "target");
        assert_eq!(mapped.instructions.as_deref(), Some("sys1\nsys2"));
        assert_eq!(mapped.max_output_tokens, Some(10));
        assert_eq!(mapped.temperature, Some(0.5));
        assert_eq!(mapped.top_p, Some(0.9));

        let tools = mapped.tools.expect("tools mapped");
        assert_eq!(tools.len(), 1);
        match &tools[0] {
            ResponseTool::Function {
                name,
                description,
                parameters,
            } => {
                assert_eq!(name, "tool1");
                assert_eq!(description.as_deref(), Some("desc"));
                assert!(parameters.is_some());
            }
            _ => panic!("unexpected tool type"),
        }

        let reasoning = mapped.reasoning.expect("reasoning mapped");
        assert_eq!(reasoning.effort.as_deref(), Some("medium"));
    }

    #[test]
    fn responses_to_anthropic_maps_text_and_tool() {
        let resp = ResponsesResponse {
            id: "resp_1".to_string(),
            model: "gpt".to_string(),
            output: vec![
                json!({
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "hello"}]
                }),
                json!({
                    "type": "function_call",
                    "name": "tool",
                    "call_id": "call_1",
                    "arguments": "{\"x\":1}"
                }),
            ],
            usage: Some(json!({"input_tokens": 3, "output_tokens": 5})),
        };

        let mapped = responses_to_anthropic(&resp, "orig", false);
        assert_eq!(mapped.model, "orig");
        assert_eq!(mapped.usage.input_tokens, 3);
        assert_eq!(mapped.usage.output_tokens, 5);
        assert_eq!(mapped.content.len(), 2);

        match &mapped.content[0] {
            ResponseContent::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected text content"),
        }

        match &mapped.content[1] {
            ResponseContent::ToolUse { id, name, input } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "tool");
                assert!(input.is_object());
            }
            _ => panic!("expected tool_use content"),
        }
    }

    #[test]
    fn extract_reasoning_text_prefers_content_then_summary() {
        let item = json!({
            "content": [{"type": "reasoning_text", "text": "thinking"}]
        });
        assert_eq!(extract_reasoning_text(&item).as_deref(), Some("thinking"));

        let item = json!({"summary": "short"});
        assert_eq!(extract_reasoning_text(&item).as_deref(), Some("short"));
    }

    #[tokio::test]
    async fn create_anthropic_stream_emits_text_events() {
        let payload = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
            "data: [DONE]\n\n"
        );
        let stream = create_anthropic_stream(
            stream::iter(vec![Ok(Bytes::from(payload))]),
            "model".to_string(),
            false,
        );
        let events: Vec<String> = stream.map(|r| r.unwrap()).collect().await;

        assert!(events.iter().any(|e| e.contains("message_start")));
        assert!(events.iter().any(|e| e.contains("\"type\":\"text_delta\"")));
        assert!(events.iter().any(|e| e.contains("Hello")));
        assert!(events.iter().any(|e| e.contains("message_stop")));
    }

    #[tokio::test]
    async fn create_anthropic_stream_emits_tool_events() {
        let payload = concat!(
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"tool\",\"arguments\":\"{\\\"x\\\":1\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"delta\":\"}\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\"}}\n\n",
            "data: [DONE]\n\n"
        );
        let stream = create_anthropic_stream(
            stream::iter(vec![Ok(Bytes::from(payload))]),
            "model".to_string(),
            false,
        );
        let events: Vec<String> = stream.map(|r| r.unwrap()).collect().await;

        assert!(events.iter().any(|e| e.contains("\"type\":\"tool_use\"")));
        assert!(events.iter().any(|e| e.contains("\"type\":\"input_json_delta\"")));
        assert!(events.iter().any(|e| e.contains("content_block_stop")));
    }
}
