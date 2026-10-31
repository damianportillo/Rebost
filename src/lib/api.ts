/** Typed surface over the Tauri commands and events. */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { dateLocale, t } from "./i18n.svelte";

/** Types mirroring the Rust structs. */

export interface PiiSummaryView {
  total: number;
  categories: Record<string, number>;
}

export interface ShelfStats {
  files: number;
  searchable: number;
  reading: number;
  errors: number;
  waiting: number;
  filesWithPii: number;
  pii: PiiSummaryView;
}

export interface LinkedView {
  sourceId: string;
  path: string;
  label: string;
  available?: boolean;
}

export type ThinkLevel = "off" | "light" | "deep";

export interface ShelfView {
  id: string;
  name: string;
  managedPath: string;
  linkedFolders: LinkedView[];
  stats: ShelfStats;
  threadId?: string | null;
  thinkLevel: ThinkLevel;
  /** How many files this Shelf reads. New files past it are left unread. */
  fileLimit: number;
}

export interface DocumentMeta {
  id: string;
  shelfId: string;
  sourceId: string;
  sourceType: "imported" | "linked";
  path: string;
  relPath: string;
  fileName: string;
  format: string;
  sizeBytes: number;
  hash: string;
  status: "reading" | "ready" | "error";
  error?: string;
  passageCount: number;
  pages?: number;
  piiTotal: number;
  piiCategories?: Record<string, number>;
  ocr: boolean;
  updatedAt: string;
  sourceLabel: string;
}

export interface OutlineEntry {
  title: string;
  page?: number;
}

export interface Card {
  schema: string;
  id: string;
  source: "imported" | "linked";
  path: string;
  hash: string;
  title: string;
  format: string;
  language?: string;
  summary: string;
  keywords: string[];
  outline: OutlineEntry[];
  quality: "full" | "ocr";
  privacy: { total: number; categories?: Record<string, number> };
}

export interface SourcePassage {
  anchor?: { hash: string; startChar?: number; endChar?: number; quote: string };
  sid: string;
  documentId: string;
  shelfId: string;
  title: string;
  section?: string;
  pageStart?: number;
  pageEnd?: number;
  body?: string;
  path: string;
  score: number;
}

export interface DocumentTextWindow {
  versionChanged?: boolean;
  text: string;
  startChar: number;
  endChar: number;
  totalChars: number;
  windowChars: number;
}

export interface ThreadMeta {
  id: string;
  title: string;
  shelfId?: string | null;
  uploadShelfId?: string | null;
  createdAt: string;
  updatedAt: string;
  messageCount: number;
  avatarId: string;
}

export interface ChatActivityStep {
  stage: ChatPrepareStage;
  file?: string | null;
}

export interface StoredMessage {
  images?: ChatImage[];
  id: string;
  role: "user" | "assistant";
  text: string;
  thinking?: string | null;
  activity?: ChatActivityStep[];
  ts: string;
  shelfId?: string | null;
  sources: SourcePassage[];
  status: "done" | "stopped" | "error" | "interrupted";
}

export interface ThreadPage {
  messages: StoredMessage[];
  hasOlder: boolean;
}

export interface EngineStatus {
  vision?: VisionLimits | null;
  state: "no-model" | "downloading" | "stopped" | "starting" | "ready" | "error";
  detail?: string;
  modelName?: string;
}

export interface VisionLimits {
  maxImages: number;
  maxEdge: number;
  tokensPerImage: number;
}

export interface ChatImage {
  id: string;
  name: string;
  width: number;
  height: number;
  bytes: number;
}

export interface MachineProfile {
  totalRamBytes: number;
  cpu: string;
  accelerator: string;
  freeDiskBytes: number;
  processArch: string;
  osArch: string;
}

export interface Recommendation {
  name: string;
  reference: string;
  provider: string;
  approxBytes: number;
  license: string;
  released: string;
  blurb: string;
}

export interface MachineView {
  profile: MachineProfile;
  recommendation: Recommendation;
  alternatives: Recommendation[];
  suggestions: Recommendation[];
}

export interface InstalledModel {
  projector?: { file: string; sizeBytes: number };
  file: string;
  name: string;
  source: string;
  reference: string;
  license?: string;
  sizeBytes: number;
}

export interface ModelSearchResult {
  id: string;
  name: string;
  source: string;
  reference: string;
  file?: string;
  sizeBytes?: number;
  license?: string;
  released?: string;
  downloads?: number;
  publisher?: string;
  official?: boolean;
  fits?: boolean;
}

export type TextSize = "default" | "large" | "larger";

export type AppLocale =
  | "en"
  | "es"
  | "ca"
  | "pt"
  | "fr"
  | "ja"
  | "de"
  | "it"
  | "sv"
  | "nb"
  | "nl"
  | "cs"
  | "el"
  | "da"
  | "fi";
export type LocalePref = "system" | AppLocale;

export interface LocalApiStatus {
  running: boolean;
  bind: string;
  port: number;
  baseUrl: string;
  keyConfigured: boolean;
}

export interface SettingsView {
  houseRules: string;
  onboardingDone: boolean;
  activeModel?: InstalledModel | null;
  /** Installed AIs that are not in use, most recently used first. */
  otherModels?: InstalledModel[];
  allowOnlineResearch: boolean;
  textSize: TextSize;
  uiLocale: LocalePref;
  resolvedLocale: AppLocale;
  modelsFolder: ModelsFolderView;
}

/** Where AIs are kept. */
export interface ModelsFolderView {
  path: string;
  /** A folder the person chose, not Rebost's own. */
  custom: boolean;
  /** The chosen folder can't be found (an unplugged drive). */
  missing: boolean;
  /** The latest folder change kept some old copies. */
  keptOldCopies: boolean;
}

/** A folder picked for AIs, and what using it would involve. */
export interface ModelsFolderChoice {
  path: string;
  isDefault: boolean;
  /** Already the folder in use. */
  unchanged: boolean;
  freeBytes: number | null;
  /** Bytes of installed AIs that would move there. */
  moveBytes: number;
  /** Error code when the folder can't be used. */
  problem: string | null;
}

/** `kept-copies`: moved, but some old copies stayed (never removed unless provably redundant). */
export type ModelsMoveOutcome = "done" | "cancelled" | "kept-copies";

export interface ModelsMoveEvent {
  done: number;
  total: number;
}

export interface ImportResult {
  queued: number;
  names: string[];
  cancelled: boolean;
  atLimit: boolean;
  skippedLong?: number;
}

export interface AddLinkedResult {
  shelf: ShelfView;
  queued: number;
  atLimit: boolean;
}

export interface Recipe {
  id: string;
  name: string;
  prompt: string;
  builtin: boolean;
  needsShelf?: boolean;
}

export type ExternalLink = "repository" | "releases";

export interface AboutInfo {
  version: string;
  repositoryUrl: string;
  /** Who installs updates: Rebost itself, a Flatpak software centre, or a package manager. */
  updates: "app" | "softwareCentre" | "packageManager";
}

/** One marketing frame for the debug screenshot runner. */
export interface ShotJob {
  id: string;
  path: string;
  locale?: string;
  theme?: "light" | "dark";
  /** Scroll the main pane before the snapshot: pixels, or "end". */
  scroll?: number | "end";
  onboarding?: boolean;
  onboard?: "promise" | "model";
  onboardMore?: boolean;
  view?: "chat" | "shelves" | "recipes" | "settings";
  thread?: number;
  source?: boolean;
  thinking?: boolean;
  shelf?: string;
  doc?: string;
  recipe?: string;
  explore?: boolean;
  about?: boolean;
  label?: string;
  settleMs?: number;
}

export interface AppUpdate {
  version: string;
  currentVersion: string;
  notes?: string | null;
  /** Replaces the x64 copy on an ARM PC with the ARM build. */
  switchesToArm?: boolean;
}

export type UpdateProgress =
  | { event: "started"; data: { contentLength?: number | null } }
  | { event: "progress"; data: { chunkLength: number } }
  | { event: "finished"; data?: null };

export interface Diagnostics {
  version: string;
  dataDir: string;
  engineBuild: string;
  engineState: EngineStatus;
  model?: InstalledModel | null;
  indexRecords: number;
  contextBudgetChars: number;
  benchmark?: {
    promptTokensPerSecond: number;
    generationTokensPerSecond: number;
    measuredAt: string;
    modelFile: string;
    runtime?: {
      engineBuild: string;
      accelerator: string;
      contextTokens: number;
      batch: number;
      ubatch: number;
      gpuLayers: number;
    } | null;
  } | null;
  machine: MachineProfile;
  engineLogPath: string;
  engineLogPresent: boolean;
  supportedFormats: string[];
  visionError?: string | null;
}

/** The x64 Windows build running under emulation on an ARM PC. */
export function runsEmulatedOnArm(profile: Pick<MachineProfile, "processArch" | "osArch">) {
  return profile.processArch === "x86_64" && profile.osArch === "aarch64";
}

/** Turn a Tauri or JS failure into a string the UI can show or log. */
export function invokeError(error: unknown): string {
  if (typeof error === "string" && error.trim()) return error;
  if (error instanceof Error && error.message.trim()) return error.message;
  return String(error);
}

const USER_ERROR_FALLBACK = () => t("errors.fallback");

const BANNED_ERROR_TERMS = [
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
] as const;

/** Two-beat toast copy. Pins stay behind Diagnostics. */
export function userFacingError(error: unknown): string {
  const trimmed = invokeError(error).trim();
  if (!trimmed) return USER_ERROR_FALLBACK();
  const imageErrors = [
    "startFailed",
    "unavailable",
    "tooLarge",
    "format",
    "tooMany",
    "limitsChanged",
    "missing",
    "storageFull",
    "invalid",
  ];
  for (const key of imageErrors) if (trimmed === t(`images.${key}`)) return trimmed;
  const imageCodes: Record<string, string> = {
    "image-start-failed": "startFailed",
    "image-unavailable": "unavailable",
    "image-too-large": "tooLarge",
    "image-format": "format",
    "image-count": "tooMany",
    "image-limits-changed": "limitsChanged",
    "image-missing": "missing",
    "image-storage-full": "storageFull",
    "image-invalid": "invalid",
  };
  if (imageCodes[trimmed]) return t(`images.${imageCodes[trimmed]}`);
  if (["errors.promptTooLong", "errors.attachmentsFailed"].some((key) => trimmed === t(key)))
    return trimmed;
  const lower = trimmed.toLowerCase();

  if (lower.includes("invalid id")) return t("errors.invalidId");
  if (lower.includes("models-folder-missing")) return t("errors.modelsFolderMissing");
  if (lower.includes("not in a shelf") || lower.includes("not allowed")) {
    return t("errors.notInShelf");
  }
  if (
    lower.includes("shelf not found") ||
    lower.includes("file not found") ||
    lower.includes("thread not found") ||
    lower.includes("recipe not found")
  ) {
    return t("errors.notAvailable");
  }
  if (
    lower.includes("no ai model") ||
    lower.includes("no model installed") ||
    lower.includes("model file missing")
  ) {
    return t("errors.needsAi");
  }
  if (lower.includes("switch-failed")) {
    return t("errors.switchFailed");
  }
  if (lower.includes("warmup-failed")) {
    return t("errors.warmupFailed");
  }
  if (lower.includes("incompatible-format")) {
    return t("errors.incompatible");
  }
  if (
    lower.includes("llama-server") ||
    lower.includes("health timeout") ||
    lower.includes("no free port") ||
    lower.includes("engine archive") ||
    lower.includes("engine binary")
  ) {
    return t("errors.notReady");
  }
  if (
    lower.includes("verification failed") ||
    lower.includes("sha-256") ||
    lower.includes("sha256") ||
    lower.includes("checksum")
  ) {
    return t("errors.verifyFailed");
  }
  if (
    lower.includes("generation failed") ||
    lower.includes("generation stalled") ||
    lower.includes("empty generation") ||
    lower.includes("chat stream")
  ) {
    return t("errors.generationFailed");
  }
  if (lower.includes("stalled")) {
    return t("errors.downloadStalled");
  }
  if (lower.includes("rate-limited") || lower.includes("rate limited")) {
    return t("errors.rateLimited");
  }
  if (
    lower.includes("download failed") ||
    lower.includes("incomplete range") ||
    lower.includes("range wrote") ||
    lower.includes("server ignored range")
  ) {
    return t("errors.downloadFailed");
  }
  if (
    lower.includes(".gguf") ||
    lower.includes("gguf") ||
    lower.includes("invalid model") ||
    lower.includes("unsupported model") ||
    lower.includes("no usable model") ||
    lower.includes("no model layer")
  ) {
    return t("errors.aiUnavailable");
  }
  if (lower.includes("couldn't read any text")) {
    return t("errors.noText");
  }
  if (lower.includes("unsupported format")) {
    return t("errors.unsupportedType");
  }
  if (lower.includes("invalid file name")) {
    return t("errors.badFileName");
  }

  if (alreadyQuietError(trimmed, lower)) return trimmed;
  return USER_ERROR_FALLBACK();
}

function alreadyQuietError(text: string, lower: string): boolean {
  if ([...text].length > 180) return false;
  if (BANNED_ERROR_TERMS.some((pin) => lower.includes(pin))) return false;
  if (lower.includes("::") || lower.includes(".rs") || lower.includes("anyhow")) return false;
  return text.endsWith(".") || text.endsWith("?");
}

// Commands

export const api = {
  shelvesList: () => invoke<ShelfView[]>("shelves_list"),
  shelfGet: (shelfId: string) => invoke<ShelfView>("shelf_get", { shelfId }),
  shelfCreate: (name: string) => invoke<ShelfView>("shelf_create", { name }),
  shelfRename: (shelfId: string, name: string) =>
    invoke<ShelfView>("shelf_rename", { shelfId, name }),
  shelfRemove: (shelfId: string) => invoke<void>("shelf_remove", { shelfId }),
  shelfSetThinkLevel: (shelfId: string, thinkLevel: ThinkLevel) =>
    invoke<ShelfView>("shelf_set_think_level", { shelfId, thinkLevel }),
  shelfSetFileLimit: (shelfId: string, fileLimit: number) =>
    invoke<ShelfView>("shelf_set_file_limit", { shelfId, fileLimit }),
  shelfAddLinked: (shelfId: string) =>
    invoke<AddLinkedResult | null>("shelf_add_linked", { shelfId }),
  shelfRemoveSource: (shelfId: string, source: { sourceId?: string; path?: string }) => {
    const payload: Record<string, string> = { shelfId };
    if (source.sourceId) payload.sourceId = source.sourceId;
    if (source.path) payload.path = source.path;
    return invoke<void>("shelf_remove_source", payload);
  },
  shelfImportPaths: (shelfId: string, paths: string[]) =>
    invoke<ImportResult>("shelf_import_paths", { shelfId, paths }),
  shelfImportDialog: (shelfId: string) => invoke<ImportResult>("shelf_import_dialog", { shelfId }),
  pickFiles: (imagesOnly = false) => invoke<string[] | null>("pick_files", { imagesOnly }),
  shelfDocuments: (shelfId: string) => invoke<DocumentMeta[]>("shelf_documents", { shelfId }),
  documentCard: (shelfId: string, docId: string) =>
    invoke<Card>("document_card", { shelfId, docId }),
  documentText: (
    shelfId: string,
    docId: string,
    opts?: {
      startChar?: number;
      page?: number;
      section?: string;
      around?: string;
      sourceHash?: string;
    },
  ) =>
    invoke<DocumentTextWindow>("document_text", {
      shelfId,
      docId,
      startChar: opts?.startChar ?? null,
      page: opts?.page ?? null,
      section: opts?.section ?? null,
      around: opts?.around ?? null,
      sourceHash: opts?.sourceHash ?? null,
    }),
  documentReprocess: (shelfId: string, docId: string) =>
    invoke<void>("document_reprocess", { shelfId, docId }),
  shelfRetryFailed: (shelfId: string) => invoke<number>("shelf_retry_failed", { shelfId }),
  openOriginal: (path: string) => invoke<void>("open_original", { path }),
  revealItem: (path: string) => invoke<void>("reveal_item", { path }),

  threadsList: () => invoke<ThreadMeta[]>("threads_list"),
  threadCreate: (shelfId?: string | null) => invoke<ThreadMeta>("thread_create", { shelfId }),
  threadMessages: (threadId: string, beforeId?: string | null) =>
    invoke<ThreadPage>("thread_messages", { threadId, beforeId: beforeId ?? null }),
  threadSetShelf: (threadId: string, shelfId?: string | null) =>
    invoke<void>("thread_set_shelf", { threadId, shelfId }),
  threadRename: (threadId: string, title: string) =>
    invoke<void>("thread_rename", { threadId, title }),
  threadExport: (threadId: string) => invoke<boolean>("thread_export", { threadId }),
  threadEnsureUploadShelf: (threadId: string) =>
    invoke<ShelfView>("thread_ensure_upload_shelf", { threadId }),
  threadDelete: (threadId: string) => invoke<void>("thread_delete", { threadId }),
  chatSend: (threadId: string, text: string, shelfId?: string | null, imageIds: string[] = []) =>
    invoke<void>("chat_send", { threadId, text, shelfId, imageIds }),
  chatImageAdd: (threadId: string, name: string, input: { data?: string; path?: string }) =>
    invoke<ChatImage>("chat_image_add", { threadId, name, ...input }),
  chatImageRead: (threadId: string, imageId: string, thumbnail = true) =>
    invoke<string>("chat_image_read", { threadId, imageId, thumbnail }),
  chatImageRemove: (threadId: string, imageId: string) =>
    invoke<void>("chat_image_remove", { threadId, imageId }),
  chatApproveWeb: (requestId: string, allowed: boolean) =>
    invoke<void>("chat_approve_web", { requestId, allowed }),
  chatCancel: (messageId: string) => invoke<void>("chat_cancel", { messageId }),
  warmEngine: () => invoke<void>("warm_engine"),

  engineStatus: () => invoke<EngineStatus>("engine_status"),
  modelVisionOffer: () => invoke<number | null>("model_vision_offer"),
  modelEnableVision: () => invoke<void>("model_enable_vision"),
  engineRemeasure: () => invoke<void>("engine_remeasure"),
  machineProfile: () => invoke<MachineView>("machine_profile"),
  modelsSearch: (query: string) => invoke<ModelSearchResult[]>("models_search", { query }),
  modelInstall: (source: string, reference: string, name: string, license?: string) =>
    invoke<void>("model_install", { source, reference, name, license }),
  modelUse: (file: string) => invoke<void>("model_use", { file }),
  modelRemove: (file: string) => invoke<void>("model_remove", { file }),
  modelReveal: (file?: string) => invoke<void>("model_reveal", { file: file ?? null }),
  openModelPage: (source: string, reference: string) =>
    invoke<void>("open_model_page", { source, reference }),
  downloadCancel: (id: string) => invoke<void>("download_cancel", { id }),
  modelsFolderPick: () => invoke<ModelsFolderChoice | null>("models_folder_pick"),
  modelsFolderApply: () => invoke<ModelsMoveOutcome>("models_folder_apply"),
  modelsFolderUseDefault: () => invoke<ModelsMoveOutcome>("models_folder_use_default"),
  modelsFolderRecheck: () => invoke<boolean>("models_folder_recheck"),
  downloadSkipVerify: (id: string) => invoke<void>("download_skip_verify", { id }),

  settingsGet: () => invoke<SettingsView>("settings_get"),
  apiStatus: () => invoke<LocalApiStatus>("api_status"),
  apiConfigure: (port: number, bindLan: boolean) =>
    invoke<LocalApiStatus>("api_configure", { port, bindLan }),
  apiKeyGenerate: () => invoke<string>("api_key_generate"),
  apiStart: () => invoke<LocalApiStatus>("api_start"),
  apiStop: () => invoke<LocalApiStatus>("api_stop"),
  setHouseRules: (text: string) => invoke<void>("settings_set_house_rules", { text }),
  setAllowOnlineResearch: (enabled: boolean) =>
    invoke<void>("settings_set_allow_online_research", { enabled }),
  setTextSize: (size: TextSize) => invoke<void>("settings_set_text_size", { size }),
  setUiLocale: (locale: LocalePref) => invoke<SettingsView>("settings_set_ui_locale", { locale }),
  finishOnboarding: () => invoke<void>("settings_finish_onboarding"),
  resetWorkspace: (confirmation: string) =>
    invoke<void>("settings_reset_workspace", { confirmation }),
  redactText: (text: string) => invoke<string>("redact_text", { text }),
  textHasPii: (text: string) => invoke<boolean>("text_has_pii", { text }),
  diagnostics: () => invoke<Diagnostics>("diagnostics"),
  openEngineLog: () => invoke<void>("open_engine_log"),

  showAboutWindow: () => invoke<void>("show_about_window"),
  closeAboutWindow: () => invoke<void>("close_about_window"),
  devSnapshot: (path: string, label?: string, settleMs?: number) =>
    invoke<void>("dev_snapshot", {
      path,
      label: label ?? null,
      settleMs: settleMs ?? null,
    }),
  devShotReady: () => invoke<void>("dev_shot_ready"),
  devShotFail: (message: string) => invoke<void>("dev_shot_fail", { message }),
  aboutInfo: () => invoke<AboutInfo>("about_info"),
  openExternal: (link: ExternalLink) => invoke<void>("open_external", { link }),

  updateInfo: () => invoke<AppUpdate | null>("update_info"),
  installUpdate: () => invoke<void>("install_update"),
  showUpdateWindow: () => invoke<void>("show_update_window"),

  recipesList: () => invoke<Recipe[]>("recipes_list"),
  recipeCreate: (name: string, prompt: string) => invoke<Recipe>("recipe_create", { name, prompt }),
  recipeUpdate: (id: string, name: string, prompt: string) =>
    invoke<Recipe>("recipe_update", { id, name, prompt }),
  recipeDelete: (id: string) => invoke<void>("recipe_delete", { id }),
  recipesRestoreDefaults: () => invoke<Recipe[]>("recipes_restore_defaults"),
  recipeResetDefault: (id: string) => invoke<Recipe>("recipe_reset_default", { id }),
};

// Events

export interface IngestEvent {
  document?: DocumentMeta;
  shelfId: string;
  documentId: string;
  fileName?: string;
  status: "reading" | "ready" | "error" | "removed";
  error?: string;
  piiTotal?: number;
  passages?: number;
}

export type DownloadPhase = "downloading" | "verifying" | "preparing";

export interface DownloadEvent {
  kind: "engine" | "model";
  id: string;
  name: string;
  received?: number;
  total?: number | null;
  done: boolean;
  error?: string | null;
  phase?: DownloadPhase;
}

export type ChatPrepareStage =
  "waiting" | "looking" | "reading" | "opening" | "around" | "chats" | "web" | "page" | "thinking";

export interface ChatEvent {
  threadId: string;
  messageId: string;
  kind:
    "queued" | "started" | "delta" | "thinking" | "promote" | "done" | "error" | "status" | "clear";
  userMessageId?: string;
  text?: string;
  message?: StoredMessage;
  status?: string;
  error?: string;
  stage?: ChatPrepareStage;
  file?: string;
}

export interface ShelfStatsEvent {
  shelfId: string;
  stats: ShelfStats;
}

export function onEvent<T>(name: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(name, (event) => handler(event.payload));
}

export type MenuAction =
  | "new-conversation"
  | "view-chat"
  | "view-shelves"
  | "view-recipes"
  | "view-settings"
  | "text-larger"
  | "text-smaller";

export interface MenuEvent {
  action: MenuAction;
}

export interface WebApproval {
  id: string;
  threadId: string;
  action?: string;
  value?: string;
  resolved?: boolean;
}

export const events = {
  webApproval: (h: (event: WebApproval) => void) => onEvent("rebost://web-approval", h),
  engine: (h: (s: EngineStatus) => void) => onEvent("rebost://engine", h),
  download: (h: (d: DownloadEvent) => void) => onEvent("rebost://download", h),
  modelsMove: (h: (m: ModelsMoveEvent) => void) => onEvent("rebost://models-move", h),
  ingest: (h: (i: IngestEvent) => void) => onEvent("rebost://ingest", h),
  shelfStats: (h: (s: ShelfStatsEvent) => void) => onEvent("rebost://shelf-stats", h),
  shelves: (h: () => void) => onEvent("rebost://shelves", h),
  chat: (h: (c: ChatEvent) => void) => onEvent("rebost://chat", h),
  menu: (h: (m: MenuEvent) => void) => onEvent("rebost://menu", h),
  update: (h: (u: AppUpdate) => void) => onEvent("rebost://update", h),
  updateProgress: (h: (p: UpdateProgress) => void) => onEvent("rebost://update-progress", h),
  shotJob: (h: (job: ShotJob) => void) => onEvent("rebost://shot-job", h),
};

// Formatting helpers

export function formatBytes(bytes?: number | null): string {
  return formatByteAmount(bytes, false);
}

/** One decimal for GB so a transfer cannot look finished while it is still moving. */
export function formatTransferBytes(bytes?: number | null): string {
  return formatByteAmount(bytes, true);
}

function formatByteAmount(bytes: number | null | undefined, transfer: boolean): string {
  if (bytes == null) return "—";
  if (bytes <= 0) return transfer ? "0 GB" : "—";
  const gb = bytes / (1024 * 1024 * 1024);
  if (gb >= 1) {
    const digits = transfer || gb < 10 ? 1 : 0;
    return `${gb.toFixed(digits)} GB`;
  }
  const mb = bytes / (1024 * 1024);
  if (mb >= 1) return `${mb.toFixed(transfer ? 1 : 0)} MB`;
  return `${(bytes / 1024).toFixed(0)} KB`;
}

/** Compact download counts from Hugging Face (`1200` → `1.2k`). */
export function formatCount(n?: number | null): string {
  if (n == null || n < 0) return "—";
  if (n < 1000) return String(n);
  const compact = (value: number, suffix: string) => {
    const digits = value >= 10 ? 0 : 1;
    return `${value.toFixed(digits).replace(/\.0$/, "")}${suffix}`;
  };
  if (n < 1_000_000) return compact(n / 1000, "k");
  return compact(n / 1_000_000, "M");
}

export function downloadHeadline(download: DownloadEvent): string {
  const phase: DownloadPhase = download.phase ?? "downloading";
  switch (phase) {
    case "preparing":
      return t("downloads.preparing");
    case "verifying":
      return t("downloads.checking");
    case "downloading":
      return download.kind === "engine"
        ? t("downloads.preparing")
        : t("downloads.downloading", { name: download.name });
    default: {
      const _exhaustive: never = phase;
      return _exhaustive;
    }
  }
}

/** Toast for a failed model or engine download. Null when the user cancelled. */
export function downloadErrorMessage(error: string): string | null {
  if (error === "cancelled") return null;
  switch (error) {
    case "verification failed":
      return t("errors.verifyFailed");
    case "stalled":
      return t("errors.downloadStalled");
    case "switch-failed":
      return t("errors.switchFailed");
    case "warmup-failed":
      return t("errors.warmupFailed");
    case "incompatible-format":
      return t("errors.incompatible");
    case "models-folder-missing":
      return t("errors.modelsFolderMissing");
    case "models-folder-full":
      return t("errors.modelsFolderFull");
    default:
      return t("errors.downloadFailed");
  }
}

const MONTH_KEYS = [
  "january",
  "february",
  "march",
  "april",
  "may",
  "june",
  "july",
  "august",
  "september",
  "october",
  "november",
  "december",
] as const;

/** Catalog the AI is downloaded from. Hugging Face wins when both list it. */
export function catalogHostLabel(source: string): string {
  switch (source) {
    case "huggingface":
    case "huggingface+ollama":
      return t("explore.huggingface");
    case "ollama":
      return t("explore.ollama");
    default:
      return t("explore.theCatalog");
  }
}

/** Catalog dates are `YYYY-MM`. */
export function formatReleased(ym: string): string {
  const [year, month] = ym.split("-");
  const idx = Number(month) - 1;
  if (!year || Number.isNaN(idx) || idx < 0 || idx > 11) return ym;
  const monthKey = MONTH_KEYS[idx];
  if (!monthKey) return ym;
  return `${t(`calendar.${monthKey}`)} ${year}`;
}

export function formatWhen(iso: string): string {
  const then = new Date(iso);
  if (Number.isNaN(then.getTime())) return "—";
  const now = new Date();
  const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((startOfDay(now) - startOfDay(then)) / 86_400_000);
  if (days <= 0) return t("when.today");
  if (days === 1) return t("when.yesterday");
  if (days < 7) return t("when.daysAgo", { count: days });
  return then.toLocaleDateString(dateLocale(), { day: "numeric", month: "short", year: "numeric" });
}

export const PII_CATEGORY_ORDER = [
  "email",
  "name",
  "ssn",
  "phone",
  "nif",
  "nie",
  "iban",
  "credit_card",
  "ip_address",
] as const;

/** Describe what Rebost detected without promising the file has no personal information. */
export function piiEmptyHint(): string {
  return t("pii.empty");
}

/** @deprecated Use piiEmptyHint(). Kept for tests that read the English string. */
export const PII_EMPTY_HINT = piiEmptyHint;

export function piiLabel(category: string, count: number): string {
  const plural = new Intl.PluralRules(dateLocale()).select(count);
  const kind = plural === "one" || plural === "few" ? plural : "other";
  const key = `pii.${category}_${kind}`;
  const label = t(key);
  return label === key ? category : label;
}

export function fileTypeLabel(doc: DocumentMeta): string {
  const format = doc.format.toUpperCase();
  if (
    doc.pages &&
    doc.pages > 0 &&
    ["PDF", "DOCX", "DOC", "PPTX", "PPT", "ODT", "ODP"].includes(format)
  ) {
    return `${format} · ${doc.pages}p`;
  }
  return format;
}
