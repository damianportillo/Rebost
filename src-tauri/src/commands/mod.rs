//! Tauri commands: thin adapters between the webview and the core.
//! Heavy work stays in core modules; errors become friendly strings here.

mod api;
mod chat;
mod images;
mod model;
mod recipes;
mod settings;
mod shelves;

pub use api::*;
pub use chat::*;
pub use images::*;
pub use model::*;
pub use recipes::*;
pub use settings::*;
pub use shelves::*;

pub(crate) type CmdResult<T> = Result<T, String>;

pub(crate) fn require_id(id: &str) -> CmdResult<()> {
    crate::ids::require_safe_id(id).map_err(friendly)
}

pub(crate) fn require_optional_id(id: Option<&str>) -> CmdResult<()> {
    match id {
        Some(id) => require_id(id),
        None => Ok(()),
    }
}

fn user_error_fallback() -> String {
    rust_i18n::t!("errors.fallback").into()
}

pub(crate) fn friendly<E: std::fmt::Display>(error: E) -> String {
    let text = error.to_string();
    log::error!("command error: {text}");
    map_user_error(&text)
}

/// Two-beat product copy. Never pass machinery (GGUF, llama-server, SHA-256) through.
pub(crate) fn map_user_error(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return user_error_fallback();
    }
    let lower = trimmed.to_lowercase();
    for (code, key) in [
        ("image-start-failed", "images.startFailed"),
        ("image-unavailable", "images.unavailable"),
        ("image-too-large", "images.tooLarge"),
        ("image-format", "images.format"),
        ("image-count", "images.tooMany"),
        ("image-limits-changed", "images.limitsChanged"),
        ("image-missing", "images.missing"),
        ("image-storage-full", "images.storageFull"),
        ("image-invalid", "images.invalid"),
    ] {
        if lower.contains(code) {
            return rust_i18n::t!(key).into();
        }
    }

    for (code, key) in [
        ("ai-busy", "errors.aiBusy"),
        ("ai-in-use", "errors.aiInUse"),
        ("ai-missing", "errors.aiMissing"),
        ("models-folder-missing", "errors.modelsFolderMissing"),
        ("models-folder-full", "errors.modelsFolderFull"),
        (
            "models-folder-not-writable",
            "errors.modelsFolderNotWritable",
        ),
        ("models-folder-app-data", "errors.modelsFolderAppData"),
        ("models-folder-name-taken", "errors.modelsFolderNameTaken"),
        ("models-folder-no-ais", "errors.modelsFolderNoAis"),
        ("models-folder-nested", "errors.modelsFolderNested"),
    ] {
        if lower.contains(code) {
            return rust_i18n::t!(key).into();
        }
    }
    if lower.contains("invalid id") {
        return rust_i18n::t!("errors.invalidId").into();
    }
    if lower.contains("not in a shelf") || lower.contains("not allowed") {
        return rust_i18n::t!("errors.notInShelf").into();
    }
    if lower.contains("shelf not found")
        || lower.contains("file not found")
        || lower.contains("thread not found")
        || lower.contains("recipe not found")
    {
        return rust_i18n::t!("errors.notAvailable").into();
    }
    if lower.contains("no ai model")
        || lower.contains("no model installed")
        || lower.contains("model file missing")
    {
        return rust_i18n::t!("errors.needsAi").into();
    }
    if lower.contains("switch-failed") {
        return rust_i18n::t!("errors.switchFailed").into();
    }
    if lower.contains("incompatible-format") {
        return rust_i18n::t!("errors.incompatible").into();
    }
    if lower.contains("warmup-failed")
        || lower.contains("llama-server")
        || lower.contains("health timeout")
        || lower.contains("no free port")
        || lower.contains("engine archive")
        || lower.contains("engine binary")
    {
        return rust_i18n::t!("errors.notReady").into();
    }
    if lower.contains("verification failed")
        || lower.contains("sha-256")
        || lower.contains("sha256")
        || lower.contains("checksum")
    {
        return rust_i18n::t!("errors.verifyFailed").into();
    }
    if lower.contains("generation failed")
        || lower.contains("generation stalled")
        || lower.contains("empty generation")
        || lower.contains("chat stream")
    {
        return rust_i18n::t!("errors.generationFailed").into();
    }
    if lower.contains("stalled") {
        return rust_i18n::t!("errors.downloadStalled").into();
    }
    if lower.contains("rate-limited") || lower.contains("rate limited") {
        return rust_i18n::t!("errors.rateLimited").into();
    }
    if lower.contains("download failed")
        || lower.contains("incomplete range")
        || lower.contains("range wrote")
        || lower.contains("server ignored range")
    {
        return rust_i18n::t!("errors.downloadFailed").into();
    }
    if lower.contains(".gguf")
        || lower.contains("gguf")
        || lower.contains("invalid model")
        || lower.contains("unsupported model")
        || lower.contains("no usable model")
        || lower.contains("no model layer")
    {
        return rust_i18n::t!("errors.aiUnavailable").into();
    }
    if lower.contains("couldn't read any text") {
        return rust_i18n::t!("errors.noText").into();
    }
    if lower.contains("unsupported format") {
        return rust_i18n::t!("errors.unsupportedType").into();
    }
    if lower.contains("invalid file name") {
        return rust_i18n::t!("errors.badFileName").into();
    }

    if already_quiet(trimmed, &lower) {
        return trimmed.to_string();
    }
    user_error_fallback()
}

fn already_quiet(text: &str, lower: &str) -> bool {
    if text.chars().count() > 180 {
        return false;
    }
    const BANNED: &[&str] = &[
        "gguf",
        "llama",
        "sha-256",
        "sha256",
        "vulkan",
        "tantivy",
        "ocr",
        "tessdata",
        "loopback",
        "llama-server",
        "aarch64",
        "x86_64",
        "tok/s",
        "checksum",
    ];
    if BANNED.iter().any(|pin| lower.contains(pin)) {
        return false;
    }
    if lower.contains("::") || lower.contains(".rs") || lower.contains("anyhow") {
        return false;
    }
    text.ends_with('.') || text.ends_with('?')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_id_rejects_path_traversal() {
        assert!(require_id("s_ab12").is_ok());
        assert!(require_id("../etc").is_err());
        assert!(require_id("a/b").is_err());
        assert_eq!(require_optional_id(None), Ok(()));
        assert!(require_optional_id(Some("..")).is_err());
    }

    #[test]
    fn friendly_maps_known_errors() {
        assert_eq!(friendly("invalid id"), "That request was not valid.");
        assert_eq!(
            friendly("not in a Shelf"),
            "That file isn't in any of Rebost's Shelves."
        );
        assert_eq!(
            friendly("shelf not found"),
            "That item is no longer available."
        );
        assert_eq!(
            friendly("x".repeat(200)),
            "Something went wrong. Try again."
        );
        assert_eq!(
            friendly("no AI model installed yet"),
            "Rebost needs an AI before it can answer."
        );
        assert_eq!(
            friendly("model file must be .gguf"),
            "That AI isn't available. Try another."
        );
        assert_eq!(
            friendly("ai-in-use"),
            "Switch to another AI before removing this one."
        );
        assert_eq!(
            friendly("incompatible-format"),
            "This AI uses a format Rebost can't run. Pick another."
        );
        assert_eq!(
            friendly("SHA-256 mismatch: got abcd, expected efgh"),
            "The download couldn't be verified. Try again."
        );
        assert_eq!(
            friendly("llama-server exited early (1)"),
            "Rebost isn't ready yet. Try again in a moment."
        );
        assert_eq!(friendly("A Shelf needs a name."), "A Shelf needs a name.");
        assert_eq!(
            friendly("engine archive did not contain llama-server"),
            "Rebost isn't ready yet. Try again in a moment."
        );
        assert_eq!(
            friendly("generation stalled"),
            "Rebost couldn't finish that answer. Try again."
        );
        assert_eq!(
            friendly("empty generation"),
            "Rebost couldn't finish that answer. Try again."
        );
        assert_eq!(
            friendly("The download stalled. Check your connection and try again."),
            "The download stalled. Check your connection and try again."
        );
    }
}
