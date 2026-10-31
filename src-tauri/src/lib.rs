//! Rebost: Private AI that works with your files.
//!
//! ```text
//! UI (Svelte) ──invoke──► commands ──► shelf / ingest / search / chat / engine
//! ```
//!
//! See `docs/architecture.md` in the repository root.

pub mod about;
pub mod api;
pub mod chat;
pub mod commands;
#[cfg(unix)]
pub mod compositor;
pub mod core;
pub mod distribution;
pub mod engine;
pub mod host;
pub mod i18n;
pub mod ids;
pub mod ingest;
pub mod instance;
pub mod limits;
pub mod menu;
pub mod models_folder;
pub mod paths;
pub mod pii;
pub mod recipes;
pub mod reset;
pub mod search;
pub mod settings;
pub mod shelf;
pub mod snapshot;
pub mod types;
pub mod updater;
pub mod window;

rust_i18n::i18n!("../locales", fallback = "en");

use std::sync::{Arc, Mutex};
use tauri::Manager;

use crate::api::ApiService;
use crate::chat::ChatService;
use crate::core::{Ctx, Events};
use crate::engine::{Engine, EngineState};
use crate::ingest::extract::ExtractorSettings;
use crate::ingest::Ingestor;
use crate::paths::Paths;
use crate::shelf::watcher::WatcherHub;

/// Forwards core events to the webview.
struct TauriEvents(tauri::AppHandle);

impl Events for TauriEvents {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        use tauri::Emitter;
        if let Err(error) = self.0.emit(event, payload) {
            log::warn!("emit {event}: {error}");
        }
    }
}

/// Bundled OCR packs — copied into app data on first extract, not at boot.
fn find_bundled_tessdata(app: &tauri::App) -> Option<std::path::PathBuf> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("resources").join("tessdata"));
    }
    candidates.push(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("tessdata"),
    );
    candidates
        .into_iter()
        .find(|dir| crate::ingest::extract::tessdata_has_packs(dir))
}

/// Folders that may hold the llama.cpp archives shipped with the app.
fn bundled_engine_dirs(app: &tauri::App) -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        roots.push(resource_dir.join("resources").join("engine"));
        roots.push(resource_dir.join("engine"));
    }
    roots.push(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("engine"),
    );
    roots
}

/// NVIDIA's proprietary driver and WebKitGTK's DMA-BUF renderer often leave
/// the window blank. Turn that renderer off on those machines unless the
/// user already chose. Must run before any thread or webview exists.
#[cfg(target_os = "linux")]
fn prepare_linux_webview() {
    const DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
    if std::env::var_os(DMABUF).is_none()
        && std::path::Path::new("/proc/driver/nvidia/version").exists()
    {
        std::env::set_var(DMABUF, "1");
    }
}

/// On Linux GTK reports the window size with its client-side decorations and
/// shadow but restores it as the content size, so a saved size grows by the
/// frame on every launch; Wayland has no window positions at all. Keep only
/// maximized and full-screen there.
fn window_state_flags() -> tauri_plugin_window_state::StateFlags {
    use tauri_plugin_window_state::StateFlags;
    if cfg!(target_os = "linux") {
        StateFlags::all() - StateFlags::SIZE - StateFlags::POSITION
    } else {
        StateFlags::all()
    }
}

pub fn run() {
    #[cfg(target_os = "linux")]
    prepare_linux_webview();

    let Some(data_dir) = crate::reset::app_data_dir(crate::reset::BUNDLE_IDENTIFIER) else {
        eprintln!("Rebost couldn't find its library folder.");
        return;
    };
    let lock = match crate::instance::acquire_for_launch(&data_dir) {
        Ok(lock) => lock,
        Err(crate::instance::AcquireError::Busy) => {
            crate::instance::request_focus(&data_dir);
            return;
        }
        Err(crate::instance::AcquireError::Io(error)) => {
            eprintln!("Rebost couldn't start. {error}");
            return;
        }
    };

    // Tauri creates the webview before `.setup()`. Wipe first so we do not
    // delete ~/Library/WebKit/<id> out from under a live page (white window).
    match crate::reset::apply_before_launch(crate::reset::BUNDLE_IDENTIFIER) {
        Ok(true) => eprintln!("Rebost: workspace reset to first-run"),
        Ok(false) => {}
        Err(error) => eprintln!("Rebost: workspace reset failed: {error}"),
    }

    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                // Index commits and a quiet update check (404, private repo,
                // offline) are not operator errors.
                .level_for("tantivy", log::LevelFilter::Warn)
                .level_for("tauri_plugin_updater", log::LevelFilter::Off)
                .max_file_size(80_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("rebost".into()),
                    }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .skip_initial_state("about")
                .skip_initial_state("update")
                .with_state_flags(window_state_flags())
                .build(),
        )
        .on_menu_event(|app, event| crate::menu::on_menu_event(app, &event))
        .manage(commands::PendingImports::new())
        .manage(commands::PendingModelsFolder::default())
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                if let Some(pending) = window.try_state::<commands::PendingImports>() {
                    pending.admit(paths.iter().cloned());
                }
            }
        });
    // GNOME and KDE apps do not carry a menu bar. The webview handles the
    // same shortcuts, and About is in Settings.
    #[cfg(not(target_os = "linux"))]
    let builder = builder.menu(crate::menu::build_menu);

    builder
        .setup(move |app| {
            app.manage(lock);

            let data_dir = app
                .path()
                .app_data_dir()
                .expect("app data dir must resolve");
            let tessdata_dir = data_dir.join("engine").join("tessdata");
            let mut paths = Paths::new(&data_dir);
            paths.set_bundled_engine_dirs(bundled_engine_dirs(app));
            let events: Arc<dyn Events> = Arc::new(TauriEvents(app.handle().clone()));
            let ctx = Ctx::new(
                paths,
                events,
                ExtractorSettings {
                    tessdata_dir: Some(tessdata_dir),
                    tessdata_bundle: find_bundled_tessdata(app),
                    timeout_secs: 300,
                },
            )?;
            let engine = Engine::new(ctx.clone());
            if let Err(error) = crate::chat::images::cleanup_drafts(&ctx.paths) {
                log::warn!("cleaning abandoned image drafts: {error}");
            }
            let chat = ChatService::new(ctx.clone(), engine.clone());
            let api = ApiService::new(ctx.clone(), engine.clone(), chat.clone());

            // Core workers live on the tokio runtime tauri drives.
            let runtime_guard = tauri::async_runtime::handle().inner().clone();
            let _enter = runtime_guard.enter();
            let ingestor = Ingestor::start(ctx.clone());
            let watcher = WatcherHub::start(ctx.clone(), ingestor.clone());

            // Startup: bring every Shelf source in sync (new/changed/removed
            // files while Rebost was closed).
            {
                let ingestor = ingestor.clone();
                let watcher = watcher.clone();
                tauri::async_runtime::spawn(async move {
                    ingestor.sync_all(false).await;
                    watcher.rebuild();
                });
            }

            if engine.status().state != EngineState::NoModel {
                let engine = engine.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = engine.ensure_ready().await {
                        log::error!("startup warmup failed: {error:#}");
                    }
                });
            }

            let ui_locale = crate::core::read_lock(&ctx.settings).ui_locale;
            crate::i18n::apply(ui_locale);
            if let Err(error) = crate::i18n::rebuild_menu(app.handle()) {
                log::warn!("rebuild menu after locale: {error}");
            }

            app.manage(crate::updater::PendingUpdate(Mutex::new(None)));
            app.manage(ctx);
            app.manage(engine);
            app.manage(chat);
            app.manage(api);
            app.manage(ingestor);
            app.manage(watcher);

            if let Some(main) = app.get_webview_window("main") {
                if crate::window::shot_park_enabled() {
                    crate::window::park_for_shots(&main);
                    #[cfg(target_os = "macos")]
                    {
                        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                    }
                } else {
                    crate::window::place_main_window(&main);
                }
            }
            #[cfg(target_os = "linux")]
            crate::window::follow_desktop_theme(app.handle());
            crate::snapshot::spawn_shot_watcher(app.handle().clone());

            let updater_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                crate::updater::check_silently(updater_handle).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::shelves_list,
            commands::shelf_get,
            commands::shelf_create,
            commands::shelf_rename,
            commands::shelf_remove,
            commands::shelf_set_think_level,
            commands::shelf_set_file_limit,
            commands::shelf_add_linked,
            commands::shelf_remove_source,
            commands::shelf_import_paths,
            commands::shelf_import_dialog,
            commands::pick_files,
            commands::shelf_documents,
            commands::document_card,
            commands::document_text,
            commands::document_reprocess,
            commands::shelf_retry_failed,
            commands::open_original,
            commands::reveal_item,
            commands::threads_list,
            commands::thread_create,
            commands::thread_messages,
            commands::thread_set_shelf,
            commands::thread_rename,
            commands::thread_export,
            commands::thread_ensure_upload_shelf,
            commands::thread_delete,
            commands::chat_send,
            commands::chat_image_add,
            commands::chat_image_read,
            commands::chat_image_remove,
            commands::model_vision_offer,
            commands::model_enable_vision,
            commands::chat_cancel,
            commands::chat_approve_web,
            commands::warm_engine,
            commands::engine_status,
            commands::engine_remeasure,
            commands::machine_profile,
            commands::models_search,
            commands::open_model_page,
            commands::model_install,
            commands::model_use,
            commands::model_remove,
            commands::model_reveal,
            commands::models_folder_pick,
            commands::models_folder_apply,
            commands::models_folder_use_default,
            commands::models_folder_recheck,
            commands::download_cancel,
            commands::download_skip_verify,
            commands::settings_get,
            commands::api_status,
            commands::api_configure,
            commands::api_key_generate,
            commands::api_start,
            commands::api_stop,
            commands::settings_set_house_rules,
            commands::settings_set_allow_online_research,
            commands::settings_set_text_size,
            commands::settings_set_ui_locale,
            commands::settings_finish_onboarding,
            commands::settings_reset_workspace,
            commands::redact_text,
            commands::text_has_pii,
            commands::diagnostics,
            commands::open_engine_log,
            commands::recipes_list,
            commands::recipe_create,
            commands::recipe_update,
            commands::recipe_delete,
            commands::recipes_restore_defaults,
            commands::recipe_reset_default,
            snapshot::dev_snapshot,
            snapshot::dev_shot_ready,
            snapshot::dev_shot_fail,
            about::about_info,
            about::show_about_window,
            window::desktop_color_scheme,
            about::close_about_window,
            about::open_external,
            updater::update_info,
            updater::install_update,
            updater::show_update_window,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Rebost")
        .run(|app, event| match event {
            tauri::RunEvent::Ready => {
                if let Some(lock) = app.try_state::<crate::instance::InstanceLock>() {
                    crate::instance::spawn_focus_listener(
                        app.clone(),
                        lock.data_dir().to_path_buf(),
                    );
                }
            }
            tauri::RunEvent::Exit => {
                // Stop the optional HTTP listener before releasing the local engine.
                if let Some(api) = app.try_state::<Arc<ApiService>>() {
                    api.stop_blocking();
                }
                // Closing Rebost stops the engine and releases its memory.
                if let Some(engine) = app.try_state::<Arc<Engine>>() {
                    engine.stop_blocking();
                }
            }
            // Dock click when no windows are open.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => crate::instance::focus_main(app),
            _ => {}
        });
}
