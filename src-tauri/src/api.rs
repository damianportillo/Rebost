//! OpenAI-compatible HTTP API for direct local-model inference.
//!
//! The listener is opt-in and starts only when requested. The llama.cpp
//! process remains bound to loopback and is never exposed directly.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use uuid::Uuid;

use crate::{
    chat::ChatService,
    core::Ctx,
    engine::{ChatMessage, ChatOptions, Engine, StreamEvent},
};

const DEFAULT_PORT: u16 = 43210;
const MAX_HEADER_BYTES: usize = 16_384;
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

#[derive(Clone)]
struct ApiState {
    ctx: Arc<Ctx>,
    engine: Arc<Engine>,
    chat: Arc<ChatService>,
}

pub struct ApiService {
    ctx: Arc<Ctx>,
    engine: Arc<Engine>,
    chat: Arc<ChatService>,
    runtime: Mutex<Runtime>,
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
        let (configured_port, bind_lan, key_configured) = {
            let settings = crate::core::read_lock(&self.ctx.settings);
            (
                settings.api_port.unwrap_or(DEFAULT_PORT),
                settings.api_bind_lan,
                settings.api_key_hash.is_some(),
            )
        };
        let runtime = self.runtime.lock().unwrap_or_else(|p| p.into_inner());
        let running = runtime.task.as_ref().is_some_and(|task| !task.is_finished());
        let bind = if running {
            runtime.bind.clone()
        } else if bind_lan {
            "0.0.0.0".into()
        } else {
            "127.0.0.1".into()
        };
        let port = if running { runtime.port } else { configured_port };
        ApiStatus {
            running,
            bind: bind.clone(),
            port,
            base_url: format!(
                "http://{}:{}",
                if bind == "0.0.0.0" { "<IP-DE-TU-PC>" } else { &bind },
                port
            ),
            key_configured,
        }
    }

    /// Generate a high-entropy bearer token; only its SHA-256 hash is persisted.
    pub fn generate_key(&self) -> anyhow::Result<String> {
        let key = format!("rb_{}", Uuid::new_v4().simple());
        let hash = hex_digest(key.as_bytes());
        self.ctx.update_settings(|settings| settings.api_key_hash = Some(hash))?;
        Ok(key)
    }

    pub fn revoke_key(&self) -> anyhow::Result<()> {
        self.ctx.update_settings(|settings| settings.api_key_hash = None)
    }

    pub fn configure(&self, port: u16, bind_lan: bool) -> anyhow::Result<()> {
        anyhow::ensure!(port != 0, "port must be between 1 and 65535");
        let runtime = self.runtime.lock().unwrap_or_else(|p| p.into_inner());
        anyhow::ensure!(
            runtime.task.as_ref().is_none_or(|task| task.is_finished()),
            "stop the API before changing its network settings"
        );
        drop(runtime);
        self.ctx.update_settings(|settings| {
            settings.api_port = Some(port);
            settings.api_bind_lan = bind_lan;
        })
    }

    pub async fn start(self: &Arc<Self>) -> anyhow::Result<ApiStatus> {
        {
            let runtime = self.runtime.lock().unwrap_or_else(|p| p.into_inner());
            if runtime.task.as_ref().is_some_and(|task| !task.is_finished()) {
                drop(runtime);
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
        let listener = TcpListener::bind(SocketAddr::new(
            ip,
            settings.api_port.unwrap_or(DEFAULT_PORT),
        ))
        .await?;
        let actual_port = listener.local_addr()?.port();
        let state = ApiState {
            ctx: self.ctx.clone(),
            engine: self.engine.clone(),
            chat: self.chat.clone(),
        };
        let task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, _)) => {
                        let state = state.clone();
                        tokio::spawn(async move {
                            if let Err(error) = handle_connection(socket, state).await {
                                log::debug!("local API connection ended: {error}");
                            }
                        });
                    }
                    Err(error) => {
                        log::error!("local API listener failed: {error}");
                        break;
                    }
                }
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
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn authorized(ctx: &Ctx, headers: &HashMap<String, String>) -> bool {
    let Some(header) = headers.get("authorization") else {
        return false;
    };
    let Some(token) = header.strip_prefix("Bearer ") else {
        return false;
    };
    let expected = crate::core::read_lock(&ctx.settings).api_key_hash.clone();
    let Some(expected) = expected else {
        return false;
    };
    let actual = hex_digest(token.as_bytes());
    if actual.len() != expected.len() {
        return false;
    }
    actual
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

async fn handle_connection(mut socket: TcpStream, state: ApiState) -> anyhow::Result<()> {
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
        anyhow::ensure!(bytes.len() <= MAX_HEADER_BYTES, "HTTP headers too large");
        let count = socket.read(&mut chunk).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&chunk[..count]);
    };
    let header_text = std::str::from_utf8(&bytes[..header_end])?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default();
    let path = request_parts.next().unwrap_or_default().split('?').next().unwrap_or_default();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    if headers.contains_key("transfer-encoding") {
        write_json(&mut socket, 411, json_error("Chunked request bodies are not supported; send Content-Length.", "length_required")).await?;
        return Ok(());
    }
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        write_json(&mut socket, 413, json_error("Request body exceeds the 1 MiB limit.", "payload_too_large")).await?;
        return Ok(());
    }
    let body_start = header_end + 4;
    while bytes.len() < body_start + content_length {
        let count = socket.read(&mut chunk).await?;
        if count == 0 {
            write_json(&mut socket, 400, json_error("Incomplete request body.", "invalid_request")).await?;
            return Ok(());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = &bytes[body_start..body_start + content_length];

    match (method, path) {
        ("GET", "/health") => {
            let engine = state.engine.status();
            write_json(
                &mut socket,
                200,
                json!({
                    "status": "ok",
                    "engine": engine.state,
                    "model": engine.model_name
                }),
            )
            .await?;
        }
        ("GET", "/v1/models") => {
            if !authorized(&state.ctx, &headers) {
                write_json(&mut socket, 401, json_error("Invalid or missing API key.", "invalid_api_key")).await?;
                return Ok(());
            }
            let model = state.engine.status().model_name.unwrap_or_else(|| "rebost-local".into());
            write_json(
                &mut socket,
                200,
                json!({
                    "object": "list",
                    "data": [{
                        "id": model,
                        "object": "model",
                        "created": 0,
                        "owned_by": "rebost"
                    }]
                }),
            )
            .await?;
        }
        ("POST", "/v1/chat/completions") => {
            if !authorized(&state.ctx, &headers) {
                write_json(&mut socket, 401, json_error("Invalid or missing API key.", "invalid_api_key")).await?;
                return Ok(());
            }
            let request: CompletionRequest = match serde_json::from_slice(body) {
                Ok(request) => request,
                Err(_) => {
                    write_json(&mut socket, 400, json_error("Invalid JSON request body.", "invalid_request")).await?;
                    return Ok(());
                }
            };
            handle_completion(&mut socket, state, request).await?;
        }
        _ => {
            write_json(&mut socket, 404, json_error("Route not found.", "not_found")).await?;
        }
    }
    Ok(())
}

fn json_error(message: &str, code: &str) -> Value {
    json!({
        "error": {
            "message": message,
            "type": "invalid_request_error",
            "code": code
        }
    })
}

async fn write_json(socket: &mut TcpStream, status: u16, value: Value) -> anyhow::Result<()> {
    let body = serde_json::to_vec(&value)?;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        411 => "Length Required",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await?;
    socket.write_all(&body).await?;
    Ok(())
}

async fn write_sse_chunk(socket: &mut TcpStream, data: &str) -> anyhow::Result<()> {
    socket
        .write_all(format!("{:X}\r\n", data.len()).as_bytes())
        .await?;
    socket.write_all(data.as_bytes()).await?;
    socket.write_all(b"\r\n").await?;
    Ok(())
}

async fn handle_completion(
    socket: &mut TcpStream,
    state: ApiState,
    request: CompletionRequest,
) -> anyhow::Result<()> {
    if request.messages.is_empty() || request.messages.len() > MAX_MESSAGES {
        write_json(socket, 400, json_error("messages must contain between 1 and 64 entries.", "invalid_messages")).await?;
        return Ok(());
    }
    let total_chars: usize = request.messages.iter().map(|message| message.content.chars().count()).sum();
    if total_chars > MAX_TOTAL_CHARS {
        write_json(socket, 413, json_error("Combined message content is too large.", "context_length_exceeded")).await?;
        return Ok(());
    }
    let mut messages = Vec::with_capacity(request.messages.len());
    for message in request.messages {
        if !matches!(message.role.as_str(), "system" | "user" | "assistant") {
            write_json(socket, 400, json_error("Only system, user, and assistant roles are supported.", "unsupported_value")).await?;
            return Ok(());
        }
        messages.push(ChatMessage::text(message.role, message.content));
    }
    let temperature = request.temperature.unwrap_or(0.7);
    if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
        write_json(socket, 400, json_error("temperature must be between 0 and 2.", "invalid_temperature")).await?;
        return Ok(());
    }
    let max_tokens = request.max_completion_tokens.or(request.max_tokens).unwrap_or(1024);
    if max_tokens == 0 || max_tokens > MAX_COMPLETION_TOKENS {
        write_json(socket, 400, json_error("max_tokens must be between 1 and 8192.", "invalid_max_tokens")).await?;
        return Ok(());
    }
    let active_model = state.engine.status().model_name.unwrap_or_else(|| "rebost-local".into());
    if let Some(requested) = request.model.as_deref() {
        if requested != active_model && requested != "rebost-local" && requested != "local" {
            write_json(socket, 404, json_error("The requested model is not available.", "model_not_found")).await?;
            return Ok(());
        }
    }

    let completion_id = format!("chatcmpl-{}", Uuid::new_v4().simple());
    let created = chrono::Utc::now().timestamp();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if request.stream.unwrap_or(false) {
        let header = concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Type: text/event-stream\r\n",
            "Cache-Control: no-cache\r\n",
            "Transfer-Encoding: chunked\r\n",
            "Connection: close\r\n\r\n"
        );
        socket.write_all(header.as_bytes()).await?;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
        let task_state = state.clone();
        let id = completion_id.clone();
        let model = active_model.clone();
        let task_cancel = cancel.clone();
        let generation = tokio::spawn(async move {
            let _generation = task_state.chat.acquire_generation().await;
            let event_sender = sender.clone();
            let result = task_state.engine.complete(
                &messages,
                {
                    let mut options = ChatOptions::stream(temperature, max_tokens);
                    options.cache_prompt = false;
                    options
                },
                &task_cancel,
                move |event| {
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
                    let _ = event_sender.send(format!("data: {chunk}\n\n"));
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
        });
        while let Some(data) = receiver.recv().await {
            if let Err(error) = write_sse_chunk(socket, &data).await {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                generation.abort();
                return Err(error.into());
            }
        }
        socket.write_all(b"0\r\n\r\n").await?;
    } else {
        let _generation = state.chat.acquire_generation().await;
        match state.engine.complete(
            &messages,
            ChatOptions::once(temperature, max_tokens),
            &cancel,
            |_| {},
        ).await {
            Ok(output) => {
                write_json(
                    socket,
                    200,
                    json!({
                        "id": completion_id,
                        "object": "chat.completion",
                        "created": created,
                        "model": active_model,
                        "choices": [{
                            "index": 0,
                            "message": {"role": "assistant", "content": output.answer},
                            "finish_reason": output.finish_reason.as_deref().unwrap_or("stop")
                        }]
                    }),
                )
                .await?;
            }
            Err(_) => {
                write_json(socket, 500, json_error("Local model generation failed.", "server_error")).await?;
            }
        }
    }
    Ok(())
}
