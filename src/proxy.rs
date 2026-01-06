//! Rust-native proxy for translating between Anthropic and OpenAI API formats.
//!
//! This proxy allows Claude Code (which expects Anthropic API) to communicate with
//! OpenAI-compatible endpoints (Responses or Completions) without requiring Python/LiteLLM.

use anyhow::Result;
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;

use crate::codex_instructions::{get_codex_instructions, CLAUDE_CODE_BRIDGE};
use crate::openai_oauth;

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

impl AnthropicUsage {
    fn from_prompt_completion(prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            input_tokens: prompt_tokens,
            output_tokens: completion_tokens,
        }
    }

    fn from_openai_usage_value(value: &Value) -> Self {
        Self {
            input_tokens: value
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: value
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        }
    }
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
    /// ChatGPT Codex backend requires `store: false` (and it is safe for normal OpenAI Responses API usage).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
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

/// Tool format for OpenAI Responses API
/// Note: Codex API expects the flat structure with name/description/parameters at top level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
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
// OpenAI Chat Completions API Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChatContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ChatImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ChatMessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ChatToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolFunction {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ChatToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub choices: Vec<ChatChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

// ============================================================================
// OpenAI Completions API Types (legacy)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionsRequest {
    pub model: String,
    pub prompt: String,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompletionsResponse {
    pub id: String,
    pub choices: Vec<CompletionChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<CompletionUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompletionChoice {
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

// ============================================================================
// Translation Logic
// ============================================================================

fn system_prompt_text(system: &SystemPrompt) -> String {
    match system {
        SystemPrompt::Text(text) => text.clone(),
        SystemPrompt::Blocks(blocks) => blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn system_prompt_text_opt(system: Option<&SystemPrompt>) -> Option<String> {
    let text = system.map(system_prompt_text)?;
    if text.is_empty() { None } else { Some(text) }
}

fn stringify_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn response_text_part(text: &str) -> ResponseInputContentPart {
    // For Responses *input*, content parts should be `input_text` regardless of role.
    // `output_text` is used in Responses *output* payloads, and can cause upstream validation errors.
    ResponseInputContentPart::InputText {
        text: text.to_string(),
    }
}

fn map_tool_choice_for_openai(value: &Value) -> Option<Value> {
    if let Some(s) = value.as_str() {
        let lower = s.trim().to_ascii_lowercase();
        return match lower.as_str() {
            "auto" => Some(Value::String("auto".to_string())),
            "none" => Some(Value::String("none".to_string())),
            "required" | "any" => Some(Value::String("required".to_string())),
            _ => None,
        };
    }

    let obj = value.as_object()?;
    let ty = obj.get("type")?.as_str()?.to_ascii_lowercase();
    match ty.as_str() {
        "auto" => Some(Value::String("auto".to_string())),
        "none" => Some(Value::String("none".to_string())),
        "any" => Some(Value::String("required".to_string())),
        "tool" => {
            let name = obj.get("name")?.as_str()?;
            Some(serde_json::json!({
                "type": "function",
                "function": { "name": name }
            }))
        }
        _ => None,
    }
}

fn base_anthropic_response(
    response_id: &str,
    model: &str,
    content: Vec<ResponseContent>,
    usage: AnthropicUsage,
) -> AnthropicResponse {
    AnthropicResponse {
        id: format!("msg_{}", response_id),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: model.to_string(),
        stop_reason: Some("end_turn".to_string()),
        stop_sequence: None,
        usage,
    }
}

fn usage_or_default<T>(value: Option<T>, map: impl FnOnce(T) -> AnthropicUsage) -> AnthropicUsage {
    value.map_or(
        AnthropicUsage {
            input_tokens: 0,
            output_tokens: 0,
        },
        map,
    )
}

fn push_text_content(content: &mut Vec<ResponseContent>, text: &str) {
    if !text.is_empty() {
        content.push(ResponseContent::Text {
            text: text.to_string(),
        });
    }
}

fn push_tool_use(content: &mut Vec<ResponseContent>, id: &str, name: &str, arguments: &str) {
    let input: Value =
        serde_json::from_str(arguments).unwrap_or(Value::String(arguments.to_string()));
    content.push(ResponseContent::ToolUse {
        id: id.to_string(),
        name: name.to_string(),
        input,
    });
}

// ============================================================================
// Reasoning Effort Helpers (Codex model suffix parsing)
// ============================================================================

/// Reasoning effort suffixes in order of specificity (longest first to avoid partial matches)
const REASONING_SUFFIXES: [&str; 5] = ["-xhigh", "-high", "-medium", "-low", "-none"];

/// Extract reasoning effort from model suffix (e.g., "gpt-5.1-codex-high" → Some("high"))
fn parse_reasoning_effort(model: &str) -> Option<&'static str> {
    for suffix in REASONING_SUFFIXES {
        if model.ends_with(suffix) {
            return Some(&suffix[1..]); // Strip the leading dash
        }
    }
    None
}

/// Strip reasoning suffix to get base model name for API call
/// (e.g., "gpt-5.1-codex-high" → "gpt-5.1-codex")
fn normalize_model_for_api(model: &str) -> &str {
    for suffix in REASONING_SUFFIXES {
        if model.ends_with(suffix) {
            return &model[..model.len() - suffix.len()];
        }
    }
    model
}

/// Convert Anthropic request to OpenAI Responses request
pub fn anthropic_to_responses(req: &AnthropicRequest, target_model: &str) -> ResponsesRequest {
    let mut input = Vec::new();

    // Convert messages
    for msg in &req.messages {
        input.extend(convert_anthropic_message(msg));
    }

    // Add system prompt as instructions if present (for non-Codex backends)
    // Note: For Codex backend, this gets overridden in handle_responses_request
    let instructions = system_prompt_text_opt(req.system.as_ref());

    // Convert tools
    let tools = req.tools.as_ref().and_then(|tools| {
        let mapped: Vec<ResponseTool> = tools
            .iter()
            .filter_map(|tool| {
                let name = tool.get("name")?.as_str()?;
                let description = tool.get("description").and_then(|d| d.as_str());
                let input_schema = tool.get("input_schema").cloned();

                Some(ResponseTool {
                    tool_type: "function".to_string(),
                    name: name.to_string(),
                    description: description.map(String::from),
                    parameters: input_schema,
                })
            })
            .collect();

        (!mapped.is_empty()).then_some(mapped)
    });

    // Determine reasoning effort: model suffix takes precedence, then thinking config
    let reasoning = if let Some(effort) = parse_reasoning_effort(target_model) {
        // Model suffix specifies reasoning effort (e.g., gpt-5.1-codex-high)
        Some(ResponseReasoning {
            effort: Some(effort.to_string()),
        })
    } else {
        // Fall back to thinking config mapping
        match &req.thinking {
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
        }
    };

    // Normalize model name for API (strip reasoning suffix)
    let api_model = normalize_model_for_api(target_model);

    ResponsesRequest {
        model: api_model.to_string(),
        input,
        instructions,
        store: None,
        max_output_tokens: req.max_tokens,
        temperature: req.temperature,
        top_p: req.top_p,
        stream: req.stream,
        tools,
        tool_choice: req.tool_choice.as_ref().and_then(map_tool_choice_for_openai),
        reasoning,
        include: None,
    }
}

/// Convert a single Anthropic message to OpenAI Responses input items
fn convert_anthropic_message(msg: &AnthropicMessage) -> Vec<ResponseInputItem> {
    match &msg.content {
        AnthropicContent::Text(text) => vec![ResponseInputItem::Message {
            role: msg.role.clone(),
            content: vec![response_text_part(text)],
        }],
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
                        content_parts.push(response_text_part(text));
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
                        let content_str = stringify_value(content);
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

/// Convert Anthropic request to OpenAI Chat Completions request
pub fn anthropic_to_chat(req: &AnthropicRequest, target_model: &str) -> ChatCompletionRequest {
    let mut messages = Vec::new();

    if let Some(system_text) = system_prompt_text_opt(req.system.as_ref()) {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: Some(ChatMessageContent::Text(system_text)),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for msg in &req.messages {
        convert_anthropic_message_to_chat(msg, &mut messages);
    }

    // Convert tools
    let tools = req.tools.as_ref().and_then(|tools| {
        let mapped: Vec<ChatTool> = tools
            .iter()
            .filter_map(|tool| {
                let name = tool.get("name")?.as_str()?;
                let description = tool.get("description").and_then(|d| d.as_str());
                let input_schema = tool.get("input_schema").cloned();

                Some(ChatTool {
                    tool_type: "function".to_string(),
                    function: ChatToolFunction {
                        name: name.to_string(),
                        description: description.map(String::from),
                        parameters: input_schema,
                    },
                })
            })
            .collect();

        (!mapped.is_empty()).then_some(mapped)
    });

    // Normalize model name for API (strip reasoning suffix)
    let api_model = normalize_model_for_api(target_model);

    ChatCompletionRequest {
        model: api_model.to_string(),
        messages,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        top_p: req.top_p,
        stream: req.stream,
        tools,
        tool_choice: req.tool_choice.as_ref().and_then(map_tool_choice_for_openai),
    }
}

fn convert_anthropic_message_to_chat(msg: &AnthropicMessage, out: &mut Vec<ChatMessage>) {
    match &msg.content {
        AnthropicContent::Text(text) => out.push(ChatMessage {
            role: msg.role.clone(),
            content: Some(ChatMessageContent::Text(text.clone())),
            tool_calls: None,
            tool_call_id: None,
        }),
        AnthropicContent::Blocks(blocks) => {
            let mut parts: Vec<ChatContentPart> = Vec::new();

            let flush_message =
                |out: &mut Vec<ChatMessage>, role: &str, parts: &mut Vec<ChatContentPart>| {
                    if !parts.is_empty() {
                        let content = ChatMessageContent::Parts(std::mem::take(parts));
                        out.push(ChatMessage {
                            role: role.to_string(),
                            content: Some(content),
                            tool_calls: None,
                            tool_call_id: None,
                        });
                    }
                };

            for block in blocks {
                match block {
                    ContentBlock::Text { text } => {
                        parts.push(ChatContentPart::Text { text: text.clone() });
                    }
                    ContentBlock::Image { source } => {
                        if msg.role != "assistant" {
                            let data_url =
                                format!("data:{};base64,{}", source.media_type, source.data);
                            parts.push(ChatContentPart::ImageUrl {
                                image_url: ChatImageUrl { url: data_url },
                            });
                        }
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        flush_message(out, &msg.role, &mut parts);
                        out.push(ChatMessage {
                            role: "assistant".to_string(),
                            content: None,
                            tool_calls: Some(vec![ChatToolCall {
                                id: id.clone(),
                                tool_type: "function".to_string(),
                                function: ChatToolCallFunction {
                                    name: name.clone(),
                                    arguments: serde_json::to_string(input).unwrap_or_default(),
                                },
                            }]),
                            tool_call_id: None,
                        });
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        flush_message(out, &msg.role, &mut parts);
                        let content_str = stringify_value(content);
                        out.push(ChatMessage {
                            role: "tool".to_string(),
                            content: Some(ChatMessageContent::Text(content_str)),
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id.clone()),
                        });
                    }
                    ContentBlock::Thinking { .. } => {}
                    ContentBlock::RedactedThinking { .. } => {}
                }
            }

            flush_message(out, &msg.role, &mut parts);
        }
    }
}

/// Convert Anthropic request to OpenAI legacy Completions request
pub fn anthropic_to_completions(req: &AnthropicRequest, target_model: &str) -> CompletionsRequest {
    let mut prompt = String::new();

    if let Some(system_text) = system_prompt_text_opt(req.system.as_ref()) {
        prompt.push_str("System: ");
        prompt.push_str(&system_text);
        prompt.push_str("\n\n");
    }

    for msg in &req.messages {
        let content = flatten_anthropic_message_text(msg);
        if !content.is_empty() {
            let role = if msg.role.is_empty() {
                "User"
            } else if msg.role == "assistant" {
                "Assistant"
            } else {
                msg.role.as_str()
            };
            prompt.push_str(role);
            prompt.push_str(": ");
            prompt.push_str(&content);
            prompt.push_str("\n\n");
        }
    }

    if let Some(last) = req.messages.last()
        && last.role != "assistant"
    {
        prompt.push_str("Assistant: ");
    }

    // Normalize model name for API (strip reasoning suffix)
    let api_model = normalize_model_for_api(target_model);

    CompletionsRequest {
        model: api_model.to_string(),
        prompt,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        top_p: req.top_p,
        stop: req.stop_sequences.clone(),
        stream: req.stream,
    }
}

fn flatten_anthropic_message_text(msg: &AnthropicMessage) -> String {
    match &msg.content {
        AnthropicContent::Text(text) => text.clone(),
        AnthropicContent::Blocks(blocks) => {
            let mut out = String::new();
            for block in blocks {
                match block {
                    ContentBlock::Text { text } => {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(text);
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        let content_str = stringify_value(content);
                        if !content_str.is_empty() {
                            if !out.is_empty() {
                                out.push('\n');
                            }
                            out.push_str(&content_str);
                        }
                    }
                    _ => {}
                }
            }
            out
        }
    }
}

/// Convert OpenAI Chat Completions response to Anthropic response
pub fn chat_to_anthropic(resp: &ChatCompletionResponse, original_model: &str) -> AnthropicResponse {
    let mut content = Vec::new();

    if let Some(choice) = resp.choices.first() {
        if let Some(message_content) = &choice.message.content {
            match message_content {
                ChatMessageContent::Text(text) => push_text_content(&mut content, text),
                ChatMessageContent::Parts(parts) => {
                    for part in parts {
                        if let ChatContentPart::Text { text } = part {
                            push_text_content(&mut content, text);
                        }
                    }
                }
            }
        }

        if let Some(tool_calls) = &choice.message.tool_calls {
            for call in tool_calls {
                push_tool_use(
                    &mut content,
                    &call.id,
                    &call.function.name,
                    &call.function.arguments,
                );
            }
        }
    }

    let usage = usage_or_default(resp.usage.as_ref(), |u| {
        AnthropicUsage::from_prompt_completion(u.prompt_tokens, u.completion_tokens)
    });

    base_anthropic_response(&resp.id, original_model, content, usage)
}

/// Convert OpenAI Completions response to Anthropic response
pub fn completions_to_anthropic(
    resp: &CompletionsResponse,
    original_model: &str,
) -> AnthropicResponse {
    let mut content = Vec::new();
    if let Some(choice) = resp.choices.first() {
        push_text_content(&mut content, &choice.text);
    }

    let usage = usage_or_default(resp.usage.as_ref(), |u| {
        AnthropicUsage::from_prompt_completion(u.prompt_tokens, u.completion_tokens)
    });

    base_anthropic_response(&resp.id, original_model, content, usage)
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
                    {
                        push_text_content(&mut content, text);
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
            push_tool_use(&mut content, call_id, name, arguments);
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

    let usage = usage_or_default(resp.usage.as_ref(), AnthropicUsage::from_openai_usage_value);

    base_anthropic_response(&resp.id, original_model, content, usage)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpstreamMode {
    Auto,
    Responses,
    ChatCompletions,
    Completions,
}

/// Shared state for the proxy server
pub struct ProxyState {
    pub client: reqwest::Client,
    pub responses_url: String,
    pub chat_completions_url: String,
    pub completions_url: String,
    upstream_mode: tokio::sync::RwLock<UpstreamMode>,
    /// Optional model override for main requests
    pub model_override: Option<String>,
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

fn with_v1(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{}/v1", trimmed)
    }
}

fn build_upstream_urls(target_url: &str) -> (String, String, String, UpstreamMode) {
    let trimmed = target_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        let base = trimmed.trim_end_matches("/chat/completions").to_string();
        return (
            format!("{}/responses", with_v1(&base)),
            trimmed.to_string(),
            format!("{}/completions", with_v1(&base)),
            UpstreamMode::ChatCompletions,
        );
    }
    if trimmed.ends_with("/completions") && !trimmed.ends_with("/chat/completions") {
        let base = trimmed.trim_end_matches("/completions").to_string();
        return (
            format!("{}/responses", with_v1(&base)),
            format!("{}/chat/completions", with_v1(&base)),
            trimmed.to_string(),
            UpstreamMode::Completions,
        );
    }
    if trimmed.ends_with("/responses") {
        let base = trimmed.trim_end_matches("/responses").to_string();
        return (
            trimmed.to_string(),
            format!("{}/chat/completions", with_v1(&base)),
            format!("{}/completions", with_v1(&base)),
            UpstreamMode::Responses,
        );
    }

    let base = trimmed.to_string();
    (
        format!("{}/responses", with_v1(&base)),
        format!("{}/chat/completions", with_v1(&base)),
        format!("{}/completions", with_v1(&base)),
        UpstreamMode::Auto,
    )
}

/// Start the proxy server
pub async fn start_server(
    proxy_target_url: String,
    model_override: Option<String>,
    auxiliary_model: Option<String>,
) -> Result<()> {
    if let Some(ref aux) = auxiliary_model {
        eprintln!(
            "Proxy: Using auxiliary model '{}' for lightweight requests",
            aux
        );
    }

    let (responses_url, chat_completions_url, completions_url, mode) =
        build_upstream_urls(&proxy_target_url);

    let state = Arc::new(ProxyState {
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?,
        responses_url,
        chat_completions_url,
        completions_url,
        upstream_mode: tokio::sync::RwLock::new(mode),
        model_override,
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

#[derive(Debug)]
struct UpstreamError {
    status: StatusCode,
    body: String,
}

fn should_fallback(err: &UpstreamError) -> bool {
    if matches!(
        err.status,
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
    ) {
        return true;
    }
    if err.status == StatusCode::BAD_REQUEST {
        let body = err.body.to_ascii_lowercase();
        return body.contains("not found")
            || body.contains("unknown endpoint")
            || body.contains("unsupported")
            || body.contains("unrecognized");
    }
    false
}

fn select_target_model(state: &ProxyState, request: &AnthropicRequest) -> String {
    if is_auxiliary_request(request) {
        if let Some(aux) = &state.auxiliary_model {
            return aux.clone();
        }
    }
    state
        .model_override
        .clone()
        .unwrap_or_else(|| request.model.clone())
}

fn extract_auth_header(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        if let Ok(text) = value.to_str() {
            if !text.trim().is_empty() {
                return Some(text.to_string());
            }
        }
    }

    if let Some(value) = headers.get("x-api-key") {
        if let Ok(text) = value.to_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                if trimmed.to_ascii_lowercase().starts_with("bearer ") {
                    return Some(trimmed.to_string());
                }
                return Some(format!("Bearer {}", trimmed));
            }
        }
    }

    None
}

fn is_chatgpt_codex_backend(url: &str) -> bool {
    // Minimal heuristic: the Codex backend lives under chatgpt.com/backend-api/codex.
    url.contains("://chatgpt.com/backend-api/codex/")
}

fn strip_bearer_prefix(auth: &str) -> Option<&str> {
    let trimmed = auth.trim();
    if trimmed.len() < 7 {
        return None;
    }
    if trimmed[..6].eq_ignore_ascii_case("bearer") && trimmed.as_bytes()[6].is_ascii_whitespace() {
        return Some(trimmed[7..].trim());
    }
    None
}

async fn send_json_request<T: Serialize>(
    client: &reqwest::Client,
    url: &str,
    body: &T,
    auth_header: Option<&str>,
) -> Result<reqwest::Response, UpstreamError> {
    let mut builder = client.post(url).header("Content-Type", "application/json");
    if let Some(auth) = auth_header {
        builder = builder.header(header::AUTHORIZATION, auth);
    }

    // ChatGPT Codex backend requires extra headers (Codex CLI parity).
    if is_chatgpt_codex_backend(url) {
        builder = builder
            .header("accept", "text/event-stream")
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "codex_cli_rs");

        if let Some(auth) = auth_header
            && let Some(token) = strip_bearer_prefix(auth)
            && let Some(account_id) = openai_oauth::decode_chatgpt_account_id(token)
        {
            builder = builder.header("chatgpt-account-id", account_id);
        }
    }

    builder.json(body).send().await.map_err(|e| UpstreamError {
        status: StatusCode::BAD_GATEWAY,
        body: format!("Failed to connect to upstream: {}", e),
    })
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, UpstreamError> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(UpstreamError { status, body })
}

async fn parse_json<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, UpstreamError> {
    response.json::<T>().await.map_err(|e| UpstreamError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        body: format!("Parse error: {}", e),
    })
}

fn sse_response(
    stream: impl Stream<Item = Result<String, Infallible>> + Send + 'static,
) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn result_to_response(result: Result<Response, UpstreamError>) -> Response {
    result.map_or_else(|err| (err.status, err.body).into_response(), |resp| resp)
}

async fn attempt_upstream(
    state: &Arc<ProxyState>,
    mode: UpstreamMode,
    result: Result<Response, UpstreamError>,
) -> Result<Response, UpstreamError> {
    match result {
        Ok(resp) => {
            *state.upstream_mode.write().await = mode;
            Ok(resp)
        }
        Err(err) => Err(err),
    }
}

async fn attempt_or_fallback(
    state: &Arc<ProxyState>,
    mode: UpstreamMode,
    result: Result<Response, UpstreamError>,
) -> Result<Option<Response>, Response> {
    match attempt_upstream(state, mode, result).await {
        Ok(resp) => Ok(Some(resp)),
        Err(err) if should_fallback(&err) => Ok(None),
        Err(err) => Err((err.status, err.body).into_response()),
    }
}

fn handle_attempt_result(result: Result<Option<Response>, Response>) -> ControlFlow<Response, ()> {
    match result {
        Ok(Some(resp)) => ControlFlow::Break(resp),
        Ok(None) => ControlFlow::Continue(()),
        Err(resp) => ControlFlow::Break(resp),
    }
}

/// Main messages endpoint - handles Anthropic API requests
async fn messages_handler(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    Json(request): Json<AnthropicRequest>,
) -> Response {
    let original_model = request.model.clone();
    let is_streaming = request.stream.unwrap_or(false);
    let include_thinking = matches!(request.thinking, Some(ThinkingConfig::Enabled { .. }));
    let target_model = select_target_model(&state, &request);
    let auth_header = extract_auth_header(&headers);

    let mode = { *state.upstream_mode.read().await };

    match mode {
        UpstreamMode::Responses => {
            let openai_request = anthropic_to_responses(&request, &target_model);
            result_to_response(
                handle_responses_request(
                    state,
                    openai_request,
                    original_model,
                    include_thinking,
                    is_streaming,
                    auth_header.clone(),
                )
                .await,
            )
        }
        UpstreamMode::ChatCompletions => {
            let openai_request = anthropic_to_chat(&request, &target_model);
            result_to_response(
                handle_chat_request(
                    state,
                    openai_request,
                    original_model,
                    is_streaming,
                    auth_header.clone(),
                )
                .await,
            )
        }
        UpstreamMode::Completions => {
            let openai_request = anthropic_to_completions(&request, &target_model);
            result_to_response(
                handle_completions_request(
                    state,
                    openai_request,
                    original_model,
                    is_streaming,
                    auth_header.clone(),
                )
                .await,
            )
        }
        UpstreamMode::Auto => {
            handle_auto_request(
                state,
                request,
                target_model,
                original_model,
                is_streaming,
                include_thinking,
                auth_header,
            )
            .await
        }
    }
}

async fn handle_responses_request(
    state: Arc<ProxyState>,
    mut request: ResponsesRequest,
    original_model: String,
    include_thinking: bool,
    is_streaming: bool,
    auth_header: Option<String>,
) -> Result<Response, UpstreamError> {
    if is_chatgpt_codex_backend(&state.responses_url) {
        request.store = Some(false);
        request.stream = Some(true);
        request.include = Some(vec!["reasoning.encrypted_content".to_string()]);

        // Fetch official Codex instructions from GitHub (required by Codex API)
        match get_codex_instructions(&request.model).await {
            Ok(instructions) => {
                request.instructions = Some(instructions);
            }
            Err(e) => {
                eprintln!("[proxy] Failed to fetch Codex instructions: {}", e);
                return Err(UpstreamError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    body: format!("Failed to fetch Codex instructions: {}", e),
                });
            }
        }

        // Add Claude Code bridge prompt as the developer message
        let bridge_message = ResponseInputItem::Message {
            role: "developer".to_string(),
            content: vec![ResponseInputContentPart::InputText {
                text: CLAUDE_CODE_BRIDGE.to_string(),
            }],
        };
        request.input.insert(0, bridge_message);

        // Remove unsupported parameters for Codex API
        // The Codex API only supports: model, store, stream, instructions, input, tools, reasoning, text, include
        request.max_output_tokens = None;
        request.temperature = None;
        request.top_p = None;
        request.tool_choice = None;
    }

    let response = send_json_request(
        &state.client,
        &state.responses_url,
        &request,
        auth_header.as_deref(),
    )
    .await?;

    let response = ensure_success(response).await?;
    if is_streaming {
        let byte_stream = response.bytes_stream();
        let stream = create_anthropic_stream(byte_stream, original_model, include_thinking);
        return Ok(sse_response(stream));
    }

    // The ChatGPT Codex backend can return SSE even when stream=false.
    // When that happens, extract the final `response` object from the SSE and treat it as JSON.
    let openai_resp = match response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    {
        Some(ct) if ct.to_ascii_lowercase().contains("text/event-stream") => {
            let full = response.text().await.unwrap_or_default();
            let mut final_response: Option<Value> = None;
            for line in full.lines() {
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data == "[DONE]" {
                    continue;
                }
                let Ok(event) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if event_type == "response.done" || event_type == "response.completed" {
                    final_response = event.get("response").cloned();
                }
            }
            let final_response = final_response.ok_or_else(|| UpstreamError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: "Could not find final response in SSE stream".to_string(),
            })?;
            serde_json::from_value::<ResponsesResponse>(final_response).map_err(|e| UpstreamError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: format!("Parse error: {}", e),
            })?
        }
        _ => parse_json::<ResponsesResponse>(response).await?,
    };

    let anthropic_resp = responses_to_anthropic(&openai_resp, &original_model, include_thinking);
    Ok(Json(anthropic_resp).into_response())
}

async fn handle_chat_request(
    state: Arc<ProxyState>,
    request: ChatCompletionRequest,
    original_model: String,
    is_streaming: bool,
    auth_header: Option<String>,
) -> Result<Response, UpstreamError> {
    let response = send_json_request(
        &state.client,
        &state.chat_completions_url,
        &request,
        auth_header.as_deref(),
    )
    .await?;

    let response = ensure_success(response).await?;
    if is_streaming {
        let byte_stream = response.bytes_stream();
        let stream = create_anthropic_stream_from_chat(byte_stream, original_model);
        return Ok(sse_response(stream));
    }
    let openai_resp = parse_json::<ChatCompletionResponse>(response).await?;

    let anthropic_resp = chat_to_anthropic(&openai_resp, &original_model);
    Ok(Json(anthropic_resp).into_response())
}

async fn handle_completions_request(
    state: Arc<ProxyState>,
    request: CompletionsRequest,
    original_model: String,
    is_streaming: bool,
    auth_header: Option<String>,
) -> Result<Response, UpstreamError> {
    let response = send_json_request(
        &state.client,
        &state.completions_url,
        &request,
        auth_header.as_deref(),
    )
    .await?;

    let response = ensure_success(response).await?;
    if is_streaming {
        let byte_stream = response.bytes_stream();
        let stream = create_anthropic_stream_from_completions(byte_stream, original_model);
        return Ok(sse_response(stream));
    }
    let openai_resp = parse_json::<CompletionsResponse>(response).await?;

    let anthropic_resp = completions_to_anthropic(&openai_resp, &original_model);
    Ok(Json(anthropic_resp).into_response())
}

async fn handle_auto_request(
    state: Arc<ProxyState>,
    request: AnthropicRequest,
    target_model: String,
    original_model: String,
    is_streaming: bool,
    include_thinking: bool,
    auth_header: Option<String>,
) -> Response {
    let response_request = anthropic_to_responses(&request, &target_model);
    if let ControlFlow::Break(resp) = handle_attempt_result(
        attempt_or_fallback(
            &state,
            UpstreamMode::Responses,
            handle_responses_request(
                state.clone(),
                response_request,
                original_model.clone(),
                include_thinking,
                is_streaming,
                auth_header.clone(),
            )
            .await,
        )
        .await,
    ) {
        return resp;
    }

    let chat_request = anthropic_to_chat(&request, &target_model);
    if let ControlFlow::Break(resp) = handle_attempt_result(
        attempt_or_fallback(
            &state,
            UpstreamMode::ChatCompletions,
            handle_chat_request(
                state.clone(),
                chat_request,
                original_model.clone(),
                is_streaming,
                auth_header.clone(),
            )
            .await,
        )
        .await,
    ) {
        return resp;
    }

    let completion_request = anthropic_to_completions(&request, &target_model);
    result_to_response(
        attempt_upstream(
            &state,
            UpstreamMode::Completions,
            handle_completions_request(
                state.clone(),
                completion_request,
                original_model,
                is_streaming,
                auth_header,
            )
            .await,
        )
        .await,
    )
}

enum SseLine {
    Done,
    Json(Value),
}

fn parse_sse_line(line: &str) -> Option<SseLine> {
    let line = line.trim_end_matches('\r');
    if line.is_empty() || line.starts_with("event:") {
        return None;
    }
    let data = line.strip_prefix("data: ")?;
    if data == "[DONE]" {
        return Some(SseLine::Done);
    }
    let event: Value = serde_json::from_str(data).ok()?;
    Some(SseLine::Json(event))
}

fn finish_stream_message(state: &mut StreamState, msg_id: &str, model: &str) -> Vec<String> {
    let mut events = Vec::new();
    if let Some(start) = state.ensure_message_started(msg_id, model) {
        events.push(start);
    }
    events.extend(state.finish_message());
    events
}

fn text_delta_events(
    state: &mut StreamState,
    msg_id: &str,
    model: &str,
    content: &str,
) -> Vec<String> {
    let mut events = Vec::new();
    if let Some(start) = state.ensure_message_started(msg_id, model) {
        events.push(start);
    }
    if let Some(stop) = state.close_thinking_block() {
        events.push(stop);
    }
    if let Some(start) = state.ensure_text_block_started() {
        events.push(start);
    }

    state.output_tokens += 1;
    let escaped = escape_json_string(content);
    if let Some(index) = state.text_block_index {
        events.push(event_text_delta(index, &escaped));
    }
    events
}

fn thinking_delta_events(
    state: &mut StreamState,
    msg_id: &str,
    model: &str,
    content: &str,
) -> Vec<String> {
    let mut events = Vec::new();
    if let Some(start) = state.ensure_message_started(msg_id, model) {
        events.push(start);
    }
    if let Some(start) = state.ensure_thinking_block_started() {
        events.push(start);
    }
    if state.thinking_block_open {
        state.output_tokens += 1;
        let escaped = escape_json_string(content);
        if let Some(index) = state.thinking_block_index {
            events.push(event_thinking_delta(index, &escaped));
        }
    }
    events
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
        let model = model;

        futures::pin_mut!(byte_stream);

        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(bytes) => {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                    // Process complete SSE lines
                    while let Some(line) = drain_sse_line(&mut buffer) {
                        let line = match parse_sse_line(&line) {
                            Some(line) => line,
                            None => continue,
                        };

                        match line {
                            SseLine::Done => {
                                for event in finish_stream_message(&mut state, &msg_id, &model) {
                                    yield Ok(event);
                                }
                            }
                            SseLine::Json(event) => {
                                let event_type =
                                    event.get("type").and_then(|t| t.as_str()).unwrap_or("");

                                match event_type {
                            "response.output_text.delta" => {
                                if let Some(content) = event.get("delta").and_then(|d| d.as_str())
                                    && !content.is_empty()
                                {
                                    for event in
                                        text_delta_events(&mut state, &msg_id, &model, content)
                                    {
                                        yield Ok(event);
                                    }
                                }
                            }
                            "response.reasoning_text.delta" if include_thinking => {
                                if let Some(reasoning) = event.get("delta").and_then(|d| d.as_str())
                                    && !reasoning.is_empty()
                                {
                                    for event in thinking_delta_events(
                                        &mut state,
                                        &msg_id,
                                        &model,
                                        reasoning,
                                    ) {
                                        yield Ok(event);
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
                                for event in finish_stream_message(&mut state, &msg_id, &model) {
                                    yield Ok(event);
                                }
                            }
                            _ => {}
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

/// Create an Anthropic-format SSE stream from OpenAI Chat Completions stream
fn create_anthropic_stream_from_chat(
    byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    model: String,
) -> impl Stream<Item = Result<String, Infallible>> + Send + 'static {
    use futures::StreamExt;

    let mut buffer = String::new();
    let mut state = StreamState::new();

    async_stream::stream! {
        let msg_id = format!("msg_{}", uuid_simple());
        let model = model;

        futures::pin_mut!(byte_stream);

        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(bytes) => {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                    while let Some(line) = drain_sse_line(&mut buffer) {
                        let line = match parse_sse_line(&line) {
                            Some(line) => line,
                            None => continue,
                        };

                        match line {
                            SseLine::Done => {
                                for event in finish_stream_message(&mut state, &msg_id, &model) {
                                    yield Ok(event);
                                }
                            }
                            SseLine::Json(event) => {
                                if let Some(choices) = event.get("choices").and_then(|c| c.as_array()) {
                                    for choice in choices {
                                        if let Some(delta) = choice.get("delta") {
                                            if let Some(content) = delta.get("content").and_then(|c| c.as_str())
                                                && !content.is_empty()
                                            {
                                                for event in text_delta_events(
                                                    &mut state,
                                                    &msg_id,
                                                    &model,
                                                    content,
                                                ) {
                                                    yield Ok(event);
                                                }
                                            }

                                            if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                                                for (tool_idx, tool_call) in tool_calls.iter().enumerate() {
                                                    let output_index = tool_call
                                                        .get("index")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(tool_idx as u64) as u32;

                                                    let call_id = tool_call.get("id").and_then(|v| v.as_str());
                                                    let function = tool_call.get("function");
                                                    let name = function.and_then(|f| f.get("name").and_then(|v| v.as_str()));
                                                    let arguments = function.and_then(|f| f.get("arguments").and_then(|v| v.as_str()));

                                                    if call_id.is_some() || name.is_some() {
                                                        let mut map = serde_json::Map::new();
                                                        if let Some(id) = call_id {
                                                            map.insert("id".to_string(), Value::String(id.to_string()));
                                                        }
                                                        if let Some(name) = name {
                                                            map.insert("name".to_string(), Value::String(name.to_string()));
                                                        }
                                                        if !map.is_empty() {
                                                            state.capture_tool_metadata(output_index, &Value::Object(map));
                                                        }
                                                    }

                                                    if let Some(start) = state.ensure_message_started(&msg_id, &model) {
                                                        yield Ok(start);
                                                    }
                                                    if let Some(start) = state.ensure_tool_block_open(output_index) {
                                                        yield Ok(start);
                                                    }
                                                    let block_index = state.tool_block_index(output_index);

                                                    if let Some(args) = arguments
                                                        && !args.is_empty()
                                                    {
                                                        let escaped = escape_json_string(args);
                                                        if state.tool_blocks_open.contains(&output_index) {
                                                            yield Ok(event_tool_args_delta(block_index, &escaped));
                                                            state.tool_args_emitted.insert(output_index);
                                                        } else {
                                                            state.pending_tool_args
                                                                .entry(output_index)
                                                                .and_modify(|s| s.push_str(args))
                                                                .or_insert_with(|| args.to_string());
                                                        }
                                                    }

                                                    if state.tool_blocks_open.contains(&output_index) {
                                                        if let Some(pending) =
                                                            state.pending_tool_args.remove(&output_index)
                                                            && !pending.is_empty()
                                                        {
                                                            let escaped = escape_json_string(&pending);
                                                            yield Ok(event_tool_args_delta(
                                                                block_index,
                                                                &escaped,
                                                            ));
                                                            state.tool_args_emitted.insert(output_index);
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        if let Some(finish) = choice.get("finish_reason").and_then(|f| f.as_str())
                                            && !finish.is_empty()
                                        {
                                            for event in finish_stream_message(&mut state, &msg_id, &model) {
                                                yield Ok(event);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }
}

/// Create an Anthropic-format SSE stream from OpenAI Completions stream
fn create_anthropic_stream_from_completions(
    byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    model: String,
) -> impl Stream<Item = Result<String, Infallible>> + Send + 'static {
    use futures::StreamExt;

    let mut buffer = String::new();
    let mut state = StreamState::new();

    async_stream::stream! {
        let msg_id = format!("msg_{}", uuid_simple());
        let model = model;

        futures::pin_mut!(byte_stream);

        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(bytes) => {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                    while let Some(line) = drain_sse_line(&mut buffer) {
                        let line = match parse_sse_line(&line) {
                            Some(line) => line,
                            None => continue,
                        };

                        match line {
                            SseLine::Done => {
                                for event in finish_stream_message(&mut state, &msg_id, &model) {
                                    yield Ok(event);
                                }
                            }
                            SseLine::Json(event) => {
                                if let Some(choices) = event.get("choices").and_then(|c| c.as_array()) {
                                    for choice in choices {
                                        let text = choice
                                            .get("text")
                                            .and_then(|t| t.as_str())
                                            .or_else(|| {
                                                choice
                                                    .get("delta")
                                                    .and_then(|d| d.get("content"))
                                                    .and_then(|t| t.as_str())
                                            });

                                        if let Some(content) = text
                                            && !content.is_empty()
                                        {
                                            for event in text_delta_events(
                                                &mut state,
                                                &msg_id,
                                                &model,
                                                content,
                                            ) {
                                                yield Ok(event);
                                            }
                                        }

                                        if let Some(finish) = choice.get("finish_reason").and_then(|f| f.as_str())
                                            && !finish.is_empty()
                                        {
                                            for event in finish_stream_message(&mut state, &msg_id, &model) {
                                                yield Ok(event);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => break,
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
        assert_eq!(tools[0].tool_type, "function");
        assert_eq!(tools[0].name, "tool1");
        assert_eq!(tools[0].description.as_deref(), Some("desc"));
        assert!(tools[0].parameters.is_some());

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
    fn anthropic_to_chat_maps_system_and_tools() {
        let req = AnthropicRequest {
            model: "claude".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("hi".to_string()),
            }],
            max_tokens: Some(10),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            stream: None,
            system: Some(SystemPrompt::Text("sys".to_string())),
            tools: Some(vec![json!({
                "name": "tool1",
                "description": "desc",
                "input_schema": {"type": "object"}
            })]),
            tool_choice: None,
            thinking: None,
        };

        let mapped = anthropic_to_chat(&req, "target");
        assert_eq!(mapped.model, "target");
        assert_eq!(mapped.messages[0].role, "system");
        match mapped.messages[0].content.as_ref().unwrap() {
            ChatMessageContent::Text(text) => assert_eq!(text, "sys"),
            _ => panic!("expected system text"),
        }
        let tools = mapped.tools.expect("tools mapped");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name, "tool1");
    }

    #[test]
    fn chat_to_anthropic_maps_text_and_tool() {
        let resp = ChatCompletionResponse {
            id: "chat_1".to_string(),
            choices: vec![ChatChoice {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(ChatMessageContent::Text("hello".to_string())),
                    tool_calls: Some(vec![ChatToolCall {
                        id: "call_1".to_string(),
                        tool_type: "function".to_string(),
                        function: ChatToolCallFunction {
                            name: "tool".to_string(),
                            arguments: "{\"x\":1}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
            usage: Some(ChatUsage {
                prompt_tokens: 3,
                completion_tokens: 5,
            }),
        };

        let mapped = chat_to_anthropic(&resp, "orig");
        assert_eq!(mapped.model, "orig");
        assert_eq!(mapped.usage.input_tokens, 3);
        assert_eq!(mapped.usage.output_tokens, 5);
        assert_eq!(mapped.content.len(), 2);
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
        assert!(
            events
                .iter()
                .any(|e| e.contains("\"type\":\"input_json_delta\""))
        );
        assert!(events.iter().any(|e| e.contains("content_block_stop")));
    }
}
