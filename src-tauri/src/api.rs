//! OpenAI-compatible HTTP API for direct local-model inference.
//!
//! This server is opt-in and starts only when the user requests it. The
//! llama.cpp process remains bound to loopback and is never exposed directly.

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
};
use tokio::{net::TcpListener, task::JoinHandle};
use uuid::Uuid;

use crate::{
    chat::ChatService,
    core::Ctx,
    engine::{ChatMessage, ChatOptions, Engine, StreamEvent},
};

const DEFAULT_PORT: u16 = 43210;
const MAX_BODY_BYTES: usize = 1_048_576;
const MAX_MESSAGES: usize = 64;
const MAX_TOTAL_CHARS: usize = 65_536;
const MAX_COMPLETION_TOKENS: u32 = 8192;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiStatus {
    pub running: bool,
    pub bind: String,
    pub port: u16,
    pub base_url: String,
    pub key_configured: bool,
}

struct Runtime {
    task: Option<JoinHandle<()>>,
    bind: String,
    port: u16,
}

pub struct ApiService {
    ctx: Arc<Ctx>,
    engine: Arc<Engine>,
    chat: Arc<ChatService>,
    runtime: Mutex<Runtime>,
}

#[derive(Clone)]
struct ApiState {
    ctx: Arc<Ctx>,
    engine: Arc<Engine>,
    chat: Arc<ChatService>,
}

#[derive(Deserialize)]
struct CompletionRequest {
    model: Option<String>,
    messages: Vec<RequestMessage>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    max_completion_tokens: Option<u32>,
    stream: Option<bool>,
}

#[derive(Deserialize)]
struct RequestMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ApiError {
    error: ApiErrorDetail,
}

#[derive(Serialize)]
struct ApiErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: &'static str,
    code: Option<&'static str>,
}

impl ApiService {
    pub fn new(ctx: Arc<Ctx>, engine: Arc<Engine>, chat: Arc<ChatService>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            engine,
            chat,
            runtime: Mutex::new(Runtime {
                task: None,
                bind: "127.0.0.1".into(),
                port: DEFAULT_PORT,
            }),
        })
    }

    pub fn status(&self) -> ApiStatus {
        let settings = crate::core::read_lock(&self.ctx.settings);
        let runtime = self.runtime.lock().unwrap_or_else(|p| p.into_inner());
        let running = runtime.task.as_ref().is_some_and(|task| !task.is_finished());
        let bind = if running {
            runtime.bind.clone()
        } else if settings.api_bind_lan {
            "0.0.0.0".into()
        } else {
            "127.0.0.1".into()
        };
        let port = if running { runtime.port } else { settings.api_port };
        ApiStatus {
            running,
            bind: bind.clone(),
            port,
            base_url: format!(
                "http://{}:{}",
                if bind == "0.0.0.0" { "<IP-DE-TU-PC>" } else { &bind },
                port
            ),
            key_configured: settings.api_key_hash.is_some(),
        }
    }

    pub fn generate_key(&self) -> anyhow::Result<String> {
        let key = format!("rb_{}", Uuid::new_v4().simple());
        let hash = hex_digest(key.as_bytes());
        self.ctx.update_settings(|settings| settings.api_key_hash = Some(hash))?;
        Ok(key)
    }

    pub fn configure(&self, port: u16, bind_lan: bool) -> anyhow::Result<()> {
        anyhow::ensure!(port != 0, "port must be between 1 and 65535");
        anyhow::ensure!(
            self.runtime.lock().unwrap_or_else(|p| p.into_inner()).task.is_none(),
            "stop the API before changing its network settings"
        );
        self.ctx.update_settings(|settings| {
            settings.api_port = port;
            settings.api_bind_lan = bind_lan;
        })
    }

    pub async fn start(self: &Arc<Self>) -> anyhow::Result<ApiStatus> {
        {
            let runtime = self.runtime.lock().unwrap_or_else(|p| p.into_inner());
            if runtime.task.as_ref().is_some_and(|task| !task.is_finished()) {
                return Ok(self.status());
            }
        }
        let settings = crate::core::read_lock(&self.ctx.settings).clone();
        anyhow::ensure!(settings.api_key_hash.is_some(), "generate an API key before starting");
        let ip = if settings.api_bind_lan {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        };
        let address = SocketAddr::new(ip, settings.api_port);
        let listener = TcpListener::bind(address).await?;
        let actual_port = listener.local_addr()?.port();
        let state = ApiState {
            ctx: self.ctx.clone(),
            engine: self.engine.clone(),
            chat: self.chat.clone(),
        };
        let app = Router::new()
            .route("/health", get(health))
            .route("/v1/models", get(models))
            .route("/v1/chat/completions", post(chat_completions))
            .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_BYTES))
            .with_state(state);
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                log::error!("OpenAI-compatible API stopped: {error}");
            }
        });
        let mut runtime = self.runtime.lock().unwrap_or_else(|p| p.into_inner());
        runtime.bind = ip.to_string();
        runtime.port = actual_port;
        runtime.task = Some(task);
        drop(runtime);
        Ok(self.status())
    }

    pub fn stop(&self) -> ApiStatus {
        let mut runtime = self.runtime.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(task) = runtime.task.take() {
            task.abort();
        }
        drop(runtime);
        self.status()
    }

    pub fn stop_blocking(&self) {
        let mut runtime = self.runtime.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(task) = runtime.task.take() {
            task.abort();
        }
    }
}

fn hex_digest(value: &[u8]) -> String {
    Sha256::digest(value).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn authorized(ctx: &Ctx, headers: &HeaderMap) -> bool {
    let Some(header) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(token) = header.strip_prefix("Bearer ") else {
        return false;
    };
    let expected = crate::core::read_lock(&ctx.settings).api_key_hash.clone();
    let Some(expected) = expected else { return false };
    let actual = hex_digest(token.as_bytes());
    if actual.len() != expected.len() {
        return false;
    }
    actual.bytes().zip(expected.bytes()).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

fn error(status: StatusCode, message: impl Into<String>, code: Option<&'static str>) -> Response {
    (
        status,
        Json(ApiError {
            error: ApiErrorDetail {
                message: message.into(),
                error_type: "invalid_request_error",
                code,
            },
        }),
    )
        .into_response()
}

async fn health(State(state): State<ApiState>) -> Json<Value> {
    let engine = state.engine.status();
    Json(json!({
        "status": "ok",
        "engine": engine.state,
        "model": engine.model_name
    }))
}

async fn models(State(state): State<ApiState>, headers: HeaderMap) -> Response {
    if !authorized(&state.ctx, &headers) {
        return error(StatusCode::UNAUTHORIZED, "Invalid or missing API key.", Some("invalid_api_key"));
    }
    let model = state.engine.status().model_name.unwrap_or_else(|| "rebost-local".into());
    Json(json!({
        "object": "list",
        "data": [{
            "id": model,
            "object": "model",
            "created": 0,
            "owned_by": "rebost"
        }]
    }))
    .into_response()
}

async fn chat_completions(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CompletionRequest>,
) -> Response {
    if !authorized(&state.ctx, &headers) {
        return error(StatusCode::UNAUTHORIZED, "Invalid or missing API key.", Some("invalid_api_key"));
    }
    if request.messages.is_empty() || request.messages.len() > MAX_MESSAGES {
        return error(StatusCode::BAD_REQUEST, "messages must contain between 1 and 64 entries.", Some("invalid_messages"));
    }
    let total_chars: usize = request.messages.iter().map(|m| m.content.chars().count()).sum();
    if total_chars > MAX_TOTAL_CHARS {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "The combined message content is too large.", Some("context_length_exceeded"));
    }
    let mut messages = Vec::with_capacity(request.messages.len());
    for message in request.messages {
        if !matches!(message.role.as_str(), "system" | "user" | "assistant") {
            return error(StatusCode::BAD_REQUEST, "Only system, user, and assistant roles are supported.", Some("unsupported_value"));
        }
        messages.push(ChatMessage::text(message.role, message.content));
    }
    let temperature = request.temperature.unwrap_or(0.7);
    if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
        return error(StatusCode::BAD_REQUEST, "temperature must be between 0 and 2.", Some("invalid_temperature"));
    }
    let max_tokens = request.max_completion_tokens.or(request.max_tokens).unwrap_or(1024);
    if max_tokens == 0 || max_tokens > MAX_COMPLETION_TOKENS {
        return error(StatusCode::BAD_REQUEST, "max_tokens must be between 1 and 8192.", Some("invalid_max_tokens"));
    }

    let active_model = state.engine.status().model_name.unwrap_or_else(|| "rebost-local".into());
    if let Some(requested) = request.model.as_deref() {
        if requested != active_model && requested != "rebost-local" && requested != "local" {
            return error(StatusCode::NOT_FOUND, "The requested model is not available.", Some("model_not_found"));
        }
    }

    let _generation = state.chat.acquire_generation().await;
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let completion_id = format!("chatcmpl-{}", Uuid::new_v4().simple());
    let created = chrono::Utc::now().timestamp();
    if request.stream.unwrap_or(false) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
        let model = active_model.clone();
        let id = completion_id.clone();
        let result = state.engine.complete(
            &messages,
            ChatOptions::stream(temperature, max_tokens),
            &cancel,
            |event| {
                let delta = match event {
                    StreamEvent::Answer(content) => json!({"content": content}),
                    StreamEvent::Thinking(_) | StreamEvent::ResetAnswer | StreamEvent::PromoteAnswerToThinking => return,
                };
                let chunk = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": model,
                    "choices": [{"index": 0, "delta": delta, "finish_reason": null}]
                });
                let _ = sender.send(format!("data: {chunk}\n\n"));
            },
        ).await;
        match result {
            Ok(output) => {
                let finish = output.finish_reason.as_deref().unwrap_or("stop");
                let _ = sender.send(format!("data: {}\n\n", json!({
                    "id": completion_id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": active_model,
                    "choices": [{"index": 0, "delta": {}, "finish_reason": finish}]
                })));
                let _ = sender.send("data: [DONE]\n\n".into());
            }
            Err(_) => {
                let _ = sender.send(format!("data: {}\n\n", json!({
                    "error": {"message": "Local model generation failed.", "type": "server_error"}
                })));
                let _ = sender.send("data: [DONE]\n\n".into());
            }
        }
        drop(sender);
        let body_stream = stream::unfold(receiver, |mut rx| async {
            rx.recv().await.map(|chunk| (Ok::<Bytes, std::io::Error>(Bytes::from(chunk)), rx))
        });
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .body(Body::from_stream(body_stream))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
    } else {
        match state.engine.complete(
            &messages,
            ChatOptions::once(temperature, max_tokens),
            &cancel,
            |_| {},
        ).await {
            Ok(output) => Json(json!({
                "id": completion_id,
                "object": "chat.completion",
                "created": created,
                "model": active_model,
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": output.answer},
                    "finish_reason": output.finish_reason.as_deref().unwrap_or("stop")
                }],
                "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
            })).into_response(),
            Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Local model generation failed.", Some("server_error")),
        }
    }
}
