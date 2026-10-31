//! Tauri commands for the optional OpenAI-compatible local API.

use std::sync::Arc;
use tauri::State;

use super::CmdResult;
use crate::api::{ApiService, ApiStatus};

#[tauri::command]
pub fn api_status(api: State<'_, Arc<ApiService>>) -> ApiStatus {
    api.status()
}

#[tauri::command]
pub fn api_configure(api: State<'_, Arc<ApiService>>, port: u16, bind_lan: bool) -> CmdResult<ApiStatus> {
    api.configure(port, bind_lan).map_err(|error| error.to_string())?;
    Ok(api.status())
}

#[tauri::command]
pub fn api_key_generate(api: State<'_, Arc<ApiService>>) -> CmdResult<String> {
    api.generate_key().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn api_start(api: State<'_, Arc<ApiService>>) -> CmdResult<ApiStatus> {
    api.start().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub fn api_stop(api: State<'_, Arc<ApiService>>) -> ApiStatus {
    api.stop()
}
