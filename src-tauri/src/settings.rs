//! `settings.json` — the small amount of durable configuration.

use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};

use crate::i18n::UiLocalePref;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TextSize {
    #[default]
    Default,
    Large,
    Larger,
}

impl<'de> Deserialize<'de> for TextSize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.as_str() {
            "large" => Self::Large,
            "larger" => Self::Larger,
            _ => Self::Default,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Standing instructions, sent with every Chat message and Recipe run.
    /// Never mixed with retrieved documents.
    #[serde(alias = "house_rules")]
    pub house_rules: String,
    /// The AI Chat uses. The previous file stays until a new process is Ready.
    #[serde(alias = "active_model")]
    pub active_model: Option<InstalledModel>,
    /// Installed AIs that are not in use, most recently used first. Each
    /// keeps its file in the models folder until the person removes it.
    #[serde(alias = "other_models", skip_serializing_if = "Vec::is_empty")]
    pub other_models: Vec<InstalledModel>,
    /// Measured prompt-processing budget: how many characters of local
    /// context this machine can comfortably feed to the model.
    #[serde(alias = "context_budget_chars")]
    pub context_budget_chars: Option<usize>,
    /// Result of the installation benchmark (kept for diagnostics).
    pub benchmark: Option<BenchmarkResult>,
    /// First-run onboarding finished.
    #[serde(alias = "onboarding_done")]
    pub onboarding_done: bool,
    /// When true, Chat may look up public web pages from this computer.
    #[serde(alias = "allow_online_research")]
    pub allow_online_research: bool,
    /// Window type size: the current default, then two larger steps.
    #[serde(alias = "text_size")]
    pub text_size: TextSize,
    /// UI language: follow the computer, or pin a shipped catalog.
    #[serde(alias = "ui_locale")]
    pub ui_locale: UiLocalePref,
    /// "<engine release>/<model file>" that only started on the CPU build
    /// after the bundled GPU build failed. A new release tries the GPU again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_engine_for: Option<String>,
    /// "<engine release>/<model file>" whose image file failed to start.
    /// Settings stops offering that download until either changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_failed_for: Option<String>,
    /// Folder the person chose for AI models. `None` is `<app-data>/models`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_folder: Option<PathBuf>,
    /// SHA-256 hash of the high-entropy API bearer token; the token itself is never stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_hash: Option<String>,
    /// Optional OpenAI-compatible API listener port (defaults to 43210).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_port: Option<u16>,
    /// Expose the API on the LAN instead of loopback only.
    pub api_bind_lan: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projector: Option<VisionProjector>,
    /// GGUF file name inside the models folder.
    pub file: String,
    /// User-facing model name, e.g. "Gemma 4 12B".
    pub name: String,
    /// Where it came from ("huggingface" | "ollama").
    pub source: String,
    /// Repo or library reference, for Settings display.
    pub reference: String,
    /// License identifier shown before download.
    pub license: Option<String>,
    /// Approximate download size in bytes.
    #[serde(alias = "size_bytes")]
    pub size_bytes: u64,
}

/// Vision weights installed alongside the language model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionProjector {
    pub file: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkResult {
    /// Prompt tokens processed per second, as measured on this machine.
    #[serde(alias = "prompt_tokens_per_second")]
    pub prompt_tokens_per_second: f64,
    /// Generation tokens per second.
    #[serde(alias = "generation_tokens_per_second")]
    pub generation_tokens_per_second: f64,
    /// When the benchmark ran (RFC 3339).
    #[serde(alias = "measured_at")]
    pub measured_at: String,
    /// Model file that was measured.
    #[serde(alias = "model_file")]
    pub model_file: String,
    /// Missing on older measurements, which must be recalibrated.
    #[serde(default)]
    pub runtime: Option<BenchmarkRuntime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkRuntime {
    pub engine_build: String,
    pub accelerator: String,
    pub context_tokens: u32,
    pub batch: u32,
    pub ubatch: u32,
    pub gpu_layers: u32,
}

impl Settings {
    /// Load `settings.json`, or defaults when the file is missing or unreadable.
    pub fn load(path: &Path) -> Self {
        let mut settings: Self = crate::paths::read_json(path).unwrap_or_default();
        settings.house_rules =
            crate::limits::clip_chars(&settings.house_rules, crate::limits::HOUSE_RULES_MAX_CHARS);
        settings
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        crate::paths::write_json(path, self)
    }

    /// Whether the active AI or another installed AI points at this file.
    pub fn uses_model_file(&self, file: &str) -> bool {
        self.active_model
            .iter()
            .chain(&self.other_models)
            .any(|model| {
                model.file == file
                    || model
                        .projector
                        .as_ref()
                        .is_some_and(|projector| projector.file == file)
            })
    }

    /// Every file installed AIs use (weights and image files), once each.
    pub fn model_files(&self) -> Vec<String> {
        let mut files = Vec::new();
        for model in self.active_model.iter().chain(&self.other_models) {
            let projector = model.projector.as_ref().map(|p| &p.file);
            for file in std::iter::once(&model.file).chain(projector) {
                if !files.contains(file) {
                    files.push(file.clone());
                }
            }
        }
        files
    }

    /// Forget other AIs whose weights are no longer in `models_dir`, or that
    /// duplicate the active AI. Returns true when anything changed.
    pub fn forget_missing_models(&mut self, models_dir: &Path) -> bool {
        let before = self.other_models.len();
        let active = self.active_model.as_ref().map(|model| model.file.clone());
        let mut seen = std::collections::HashSet::new();
        self.other_models.retain(|model| {
            active.as_deref() != Some(model.file.as_str())
                && seen.insert(model.file.clone())
                && models_dir.join(&model.file).is_file()
        });
        self.other_models.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_json_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{not json").unwrap();
        let settings = Settings::load(&path);
        assert!(!settings.onboarding_done);
        assert!(settings.house_rules.is_empty());
        assert!(!settings.allow_online_research);
    }

    #[test]
    fn roundtrip_and_snake_case_alias() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"house_rules":"Answer in Catalan.","onboarding_done":true}"#,
        )
        .unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.house_rules, "Answer in Catalan.");
        assert!(loaded.onboarding_done);
        loaded.save(&path).unwrap();
        let again = Settings::load(&path);
        assert_eq!(again.house_rules, "Answer in Catalan.");
        assert!(!again.allow_online_research);
    }

    #[test]
    fn online_research_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = Settings {
            allow_online_research: true,
            ..Default::default()
        };
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert!(loaded.allow_online_research);
    }

    #[test]
    fn text_size_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = Settings {
            text_size: TextSize::Larger,
            ..Default::default()
        };
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.text_size, TextSize::Larger);
    }

    #[test]
    fn unknown_text_size_becomes_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"houseRules":"Keep it short.","textSize":"huge"}"#,
        )
        .unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.house_rules, "Keep it short.");
        assert_eq!(loaded.text_size, TextSize::Default);
    }

    #[test]
    fn ui_locale_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = Settings {
            ui_locale: UiLocalePref::Ca,
            ..Default::default()
        };
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.ui_locale, UiLocalePref::Ca);
    }

    #[test]
    fn unknown_ui_locale_becomes_system() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"uiLocale":"klingon"}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.ui_locale, UiLocalePref::System);
    }

    fn model(file: &str) -> InstalledModel {
        InstalledModel {
            projector: None,
            file: file.into(),
            name: file.into(),
            source: "huggingface".into(),
            reference: format!("org/{file}"),
            license: None,
            size_bytes: 1,
        }
    }

    #[test]
    fn other_models_roundtrip_and_are_omitted_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        Settings::default().save(&path).unwrap();
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("otherModels"));
        let settings = Settings {
            active_model: Some(model("a.gguf")),
            other_models: vec![model("b.gguf")],
            ..Default::default()
        };
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.other_models.len(), 1);
        assert_eq!(loaded.other_models[0].file, "b.gguf");
    }

    #[test]
    fn missing_and_duplicate_other_models_are_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        for file in ["a.gguf", "b.gguf"] {
            std::fs::write(dir.path().join(file), b"x").unwrap();
        }
        let mut settings = Settings {
            active_model: Some(model("a.gguf")),
            other_models: vec![
                model("a.gguf"),
                model("b.gguf"),
                model("b.gguf"),
                model("gone.gguf"),
            ],
            ..Default::default()
        };
        assert!(settings.forget_missing_models(dir.path()));
        let files: Vec<_> = settings
            .other_models
            .iter()
            .map(|m| m.file.as_str())
            .collect();
        assert_eq!(files, ["b.gguf"]);
        assert!(!settings.forget_missing_models(dir.path()));
    }

    #[test]
    fn model_files_in_use_include_projectors() {
        let mut other = model("b.gguf");
        other.projector = Some(VisionProjector {
            file: "vision-1.gguf".into(),
            size_bytes: 1,
        });
        let settings = Settings {
            active_model: Some(model("a.gguf")),
            other_models: vec![other],
            ..Default::default()
        };
        assert!(settings.uses_model_file("a.gguf"));
        assert!(settings.uses_model_file("vision-1.gguf"));
        assert!(!settings.uses_model_file("c.gguf"));
    }

    #[test]
    fn model_files_list_each_file_once() {
        let mut active = model("a.gguf");
        active.projector = Some(VisionProjector {
            file: "vision-1.gguf".into(),
            size_bytes: 1,
        });
        let mut other = model("b.gguf");
        other.projector = active.projector.clone();
        let settings = Settings {
            active_model: Some(active),
            other_models: vec![other, model("a.gguf")],
            ..Default::default()
        };
        assert_eq!(
            settings.model_files(),
            ["a.gguf", "vision-1.gguf", "b.gguf"]
        );
    }

    #[test]
    fn models_folder_roundtrips_and_defaults_to_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        Settings::default().save(&path).unwrap();
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("modelsFolder"));
        let chosen = dir.path().join("AI");
        Settings {
            models_folder: Some(chosen.clone()),
            ..Default::default()
        }
        .save(&path)
        .unwrap();
        assert_eq!(Settings::load(&path).models_folder, Some(chosen));
    }

    #[test]
    fn house_rules_are_clipped_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let long = "x".repeat(crate::limits::HOUSE_RULES_MAX_CHARS + 50);
        std::fs::write(&path, serde_json::json!({ "houseRules": long }).to_string()).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(
            loaded.house_rules.chars().count(),
            crate::limits::HOUSE_RULES_MAX_CHARS
        );
    }
}
