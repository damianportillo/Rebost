<script lang="ts">
  import {
    api,
    userFacingError,
    formatBytes,
    runsEmulatedOnArm,
    type Diagnostics,
    type LocalApiStatus,
    type MachineView,
    type ModelSearchResult,
    type Recommendation,
    type TextSize,
  } from "$lib/api";
  import {
    app,
    chatState,
    invalidateSettingsLoads,
    beginModelInstall,
    notifyInvokeError,
    refreshSettings,
    logInvokeError,
    setTextSize,
    setUiLocale,
  } from "$lib/stores.svelte";
  import { isMac } from "$lib/platform";
  import { parseTextSize, textSizeLabel, TEXT_SIZES } from "$lib/text-size";
  import { shot } from "$lib/shot-control.svelte";
  import { HOUSE_RULES_MAX_CHARS, clipChars } from "$lib/text-cap";
  import {
    APP_LOCALES,
    parseAppLocale,
    parseLocalePref,
    t,
    type LocalePref,
  } from "$lib/i18n.svelte";
  import { Cpu, Search, Stethoscope, Save } from "@lucide/svelte";
  import DownloadProgress from "$lib/components/DownloadProgress.svelte";
  import ResetWorkspaceModal from "$lib/components/ResetWorkspaceModal.svelte";
  import ExploreModelsModal from "$lib/components/ExploreModelsModal.svelte";
  import ModelSuggestionCards from "$lib/components/ModelSuggestionCards.svelte";
  import InstalledModels from "$lib/components/InstalledModels.svelte";
  import ModelsFolderCard from "$lib/components/ModelsFolderCard.svelte";
  import { installedBytes } from "$lib/installed-models";
  import icon from "../../assets/rebost-icon.png";

  let machine = $state<MachineView | null>(null);
  let visionOffer = $state<number | null>(null);
  let enablingVision = $state(false);

  $effect(() => {
    void app.settings?.activeModel;
    let cancelled = false;
    visionOffer = null;
    void api
      .modelVisionOffer()
      .then((bytes) => {
        if (!cancelled) visionOffer = bytes;
      })
      .catch((error) => logInvokeError(error, "image support"));
    return () => {
      cancelled = true;
    };
  });

  async function enableVision() {
    if (enablingVision) return;
    enablingVision = true;
    try {
      await api.modelEnableVision();
      await refreshSettings();
      app.engine = await api.engineStatus();
    } catch (error) {
      notifyInvokeError(error);
      // A start that failed is not offered again; settings dropped the file.
      await refreshSettings();
      visionOffer = await api.modelVisionOffer().catch(() => null);
    } finally {
      enablingVision = false;
    }
  }
  let houseRules = $state(app.rulesDraft ?? app.settings?.houseRules ?? "");
  let rulesBaseline = $state(app.settings?.houseRules ?? "");
  let rulesError = $state("");
  let rulesSaving = $state(false);
  let onlineResearch = $state(app.settings?.allowOnlineResearch ?? false);
  let localApiStatus = $state<LocalApiStatus | null>(null);
  let apiPort = $state(43210);
  let apiLan = $state(false);
  let apiKey = $state("");
  let apiBusy = $state(false);
  let apiError = $state("");
  let textSize = $state<TextSize>(parseTextSize(app.settings?.textSize));
  let uiLocale = $state<LocalePref>(parseLocalePref(app.settings?.uiLocale));
  const mac = isMac();
  let rulesSaved = $state(false);
  let diag = $state<Diagnostics | null>(null);
  let showDiag = $state(import.meta.env.VITE_START_DIAG === "1");
  let showReset = $state(false);
  let showExplore = $state(shot.explore || import.meta.env.VITE_START_EXPLORE === "1");
  let openedAbout = $state(false);
  let resetting = $state(false);
  let measuring = $state(false);
  let measureStatus = $state("");
  const chatBusy = $derived(
    Object.keys(chatState.pending).length > 0 || Object.keys(chatState.outbound).length > 0,
  );

  async function measureAgain() {
    if (measuring) return;
    measuring = true;
    measureStatus = "";
    try {
      await api.engineRemeasure();
      diag = await api.diagnostics();
      measureStatus = t("settings.measureDone");
    } catch {
      measureStatus = t("settings.measureFailed");
    } finally {
      measuring = false;
    }
  }
  const hasAi = $derived(!!app.settings?.activeModel);
  const aiBytes = $derived(
    installedBytes([
      ...(app.settings?.activeModel ? [app.settings.activeModel] : []),
      ...(app.settings?.otherModels ?? []),
    ]),
  );
  let machineLoad = 0;

  function loadMachine() {
    const load = ++machineLoad;
    api
      .machineProfile()
      .then((m) => {
        if (load === machineLoad) machine = m;
      })
      .catch(notifyInvokeError);
  }

  $effect(() => {
    void app.settings?.activeModel?.reference;
    void app.settings?.otherModels?.length;
    void app.settings?.modelsFolder?.path;
    loadMachine();
  });

  $effect(() => {
    const currentRules = clipChars(app.settings?.houseRules ?? "", HOUSE_RULES_MAX_CHARS);
    if (houseRules === rulesBaseline) houseRules = currentRules;
    rulesBaseline = currentRules;
    onlineResearch = app.settings?.allowOnlineResearch ?? false;
    textSize = parseTextSize(app.settings?.textSize);
    uiLocale = parseLocalePref(app.settings?.uiLocale);
  });

  $effect(() => {
    void api.apiStatus().then((status) => {
      localApiStatus = status;
      apiPort = status.port;
      apiLan = status.bind === "0.0.0.0";
    }).catch((error) => logInvokeError(error, "local API status"));
  });

  $effect(() => {
    if (openedAbout) return;
    if (import.meta.env.VITE_START_ABOUT !== "1") return;
    openedAbout = true;
    api.showAboutWindow().catch(notifyInvokeError);
  });

  const modelDownload = $derived(
    Object.values(app.downloads).find((d) => d.kind === "model" && !d.done && !d.error),
  );
  const engineDownload = $derived(
    Object.values(app.downloads).find((d) => d.kind === "engine" && !d.done && !d.error),
  );

  function installFrom(result: ModelSearchResult) {
    void install(
      result.source.includes("huggingface") ? "huggingface" : "ollama",
      result.reference,
      result.name,
      result.license,
      result.sizeBytes,
    );
  }

  async function install(
    source: string,
    reference: string,
    name: string,
    license?: string,
    total?: number | null,
  ) {
    await beginModelInstall(source, reference, name, license, total);
  }

  function installRec(rec: Recommendation) {
    void install("huggingface", rec.reference, rec.name, rec.license, rec.approxBytes);
  }

  async function saveRules() {
    if (rulesSaving) return;
    const saved = clipChars(houseRules, HOUSE_RULES_MAX_CHARS);
    rulesSaving = true;
    rulesError = "";
    rulesSaved = false;
    try {
      invalidateSettingsLoads();
      await api.setHouseRules(saved);
      invalidateSettingsLoads();
      rulesBaseline = saved;
      if (app.settings) app.settings.houseRules = saved;
      rulesSaved = houseRules === saved;
      app.rulesDraft = rulesSaved ? null : houseRules;
    } catch (error) {
      rulesError = userFacingError(error);
    } finally {
      rulesSaving = false;
    }
  }

  function saveTextSize(next: TextSize) {
    textSize = next;
    void setTextSize(next);
  }

  function saveUiLocale(next: LocalePref) {
    uiLocale = next;
    void setUiLocale(next);
  }

  const localeHelp = $derived(
    uiLocale === "system"
      ? t("locale.helpUsing", {
          language: t(`locale.name_${parseAppLocale(app.settings?.resolvedLocale)}`),
        })
      : t("locale.help"),
  );

  async function saveOnline() {
    const next = onlineResearch;
    try {
      invalidateSettingsLoads();
      await api.setAllowOnlineResearch(next);
      invalidateSettingsLoads();
      await refreshSettings();
    } catch (error) {
      onlineResearch = app.settings?.allowOnlineResearch ?? false;
      notifyInvokeError(error);
    }
  }

  async function saveApiNetwork() {
    if (apiBusy) return;
    apiBusy = true;
    apiError = "";
    try {
      localApiStatus = await api.apiConfigure(apiPort, apiLan);
    } catch (error) {
      apiError = userFacingError(error);
    } finally {
      apiBusy = false;
    }
  }

  async function generateApiKey() {
    if (apiBusy) return;
    apiBusy = true;
    apiError = "";
    try {
      apiKey = await api.apiKeyGenerate();
      localApiStatus = await api.apiStatus();
    } catch (error) {
      apiError = userFacingError(error);
    } finally {
      apiBusy = false;
    }
  }

  async function revokeApiKey() {
    if (apiBusy) return;
    apiBusy = true;
    apiError = "";
    try {
      localApiStatus = await api.apiKeyRevoke();
      apiKey = "";
    } catch (error) {
      apiError = userFacingError(error);
    } finally {
      apiBusy = false;
    }
  }

  async function startLocalApi() {
    if (apiBusy) return;
    apiBusy = true;
    apiError = "";
    try {
      localApiStatus = await api.apiStart();
    } catch (error) {
      apiError = userFacingError(error);
    } finally {
      apiBusy = false;
    }
  }

  async function stopLocalApi() {
    if (apiBusy) return;
    apiBusy = true;
    apiError = "";
    try {
      localApiStatus = await api.apiStop();
    } catch (error) {
      apiError = userFacingError(error);
    } finally {
      apiBusy = false;
    }
  }

  async function openDiagnostics() {
    showDiag = !showDiag;
    if (showDiag) diag = await api.diagnostics();
  }

  async function resetWorkspace() {
    resetting = true;
    try {
      await api.resetWorkspace("DELETE");
    } catch (error) {
      resetting = false;
      notifyInvokeError(error);
    }
  }
</script>

<div class="mx-auto max-w-[760px] px-4 py-5 min-[900px]:px-8 min-[900px]:py-8">
  <h1 class="mb-6 text-[22px] font-semibold text-ink">{t("settings.title")}</h1>

  <section class="card mb-6 px-4 py-5 min-[900px]:px-6">
    <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-3">
      <div class="min-w-0 flex-1 basis-64">
        <h2 id="ui-locale-heading" class="text-[15px] font-semibold text-ink">
          {t("locale.heading")}
        </h2>
        <p id="ui-locale-help" class="mt-0.5 text-[12.5px] leading-snug text-ink-soft">
          {localeHelp}
        </p>
      </div>
      <label class="sr-only" for="ui-locale">{t("locale.heading")}</label>
      <span class="select-wrap w-auto min-w-[12rem] shrink-0">
        <select
          id="ui-locale"
          name="ui-locale"
          class="input select h-9 cursor-default py-0"
          aria-labelledby="ui-locale-heading"
          aria-describedby="ui-locale-help"
          bind:value={uiLocale}
          onchange={() => saveUiLocale(uiLocale)}
        >
          <option value="system">{t("locale.system")}</option>
          {#each APP_LOCALES as code (code)}
            <option value={code}>{t(`locale.${code}`)}</option>
          {/each}
        </select>
        <svg
          viewBox="0 0 8 5"
          width="8"
          height="5"
          fill="none"
          class="pointer-events-none col-start-2 row-start-1 place-self-center text-ink-faint"
          aria-hidden="true"
        >
          <path d="M.5.5 4 4 7.5.5" stroke="currentcolor" stroke-linecap="round" />
        </svg>
      </span>
    </div>
  </section>

  <section class="card mb-6 px-4 py-5 min-[900px]:px-6">
    <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-3">
      <div class="min-w-0 flex-1 basis-64">
        <h2 id="text-size-heading" class="text-[15px] font-semibold text-ink">
          {t("settings.textSize")}
        </h2>
        <p id="text-size-help" class="mt-0.5 text-[12.5px] leading-snug text-ink-soft">
          {t("settings.textSizeHelp")}
          {mac ? t("settings.textSizeKeysMac") : t("settings.textSizeKeysWin")}
        </p>
      </div>
      <div
        role="radiogroup"
        aria-labelledby="text-size-heading"
        aria-describedby="text-size-help"
        class="segmented"
      >
        {#each TEXT_SIZES as size (size)}
          <label
            class="segmented-item {size === 'default'
              ? 'text-[12px]'
              : size === 'large'
                ? 'text-[13px]'
                : 'text-[14px]'}"
          >
            <span
              class="absolute top-1/2 left-1/2 size-[max(100%,3rem)] -translate-1/2 pointer-fine:hidden"
              aria-hidden="true"
            ></span>
            <input
              id="text-size-{size}"
              name="text-size"
              type="radio"
              value={size}
              class="sr-only"
              checked={textSize === size}
              onchange={() => saveTextSize(size)}
            />
            {textSizeLabel(size)}
          </label>
        {/each}
      </div>
    </div>
  </section>

  {#if hasAi && !app.settings?.modelsFolder?.missing}
    {@render houseRulesSection()}
    {@render onlineSection()}
    {@render aiSection()}
  {:else}
    {@render aiSection()}
    {@render houseRulesSection()}
    {@render onlineSection()}
  {/if}

  {#snippet houseRulesSection()}
    <section class="card mb-6 px-4 py-5 min-[900px]:px-6">
      <h2 class="mb-1 text-[15px] font-semibold text-ink">{t("settings.houseRules")}</h2>
      <p class="mb-3 text-[12.5px] leading-snug text-ink-soft">
        {t("settings.houseRulesHelp")}
      </p>
      <label class="sr-only" for="house-rules">{t("settings.houseRules")}</label>
      <textarea
        id="house-rules"
        name="house-rules"
        class="input min-h-32 cursor-text resize-y select-text"
        placeholder={t("settings.houseRulesPlaceholder")}
        maxlength={HOUSE_RULES_MAX_CHARS}
        bind:value={houseRules}
        oninput={(event) => {
          app.rulesDraft = event.currentTarget.value;
          rulesSaved = false;
        }}
        aria-describedby={rulesError ? "rules-error" : undefined}></textarea>
      {#if rulesError}<p
          id="rules-error"
          role="alert"
          class="mt-2 text-sm text-red-700 dark:text-red-400"
        >
          {rulesError}
        </p>{/if}
      <div class="mt-2.5 flex justify-end">
        <button type="button" class="btn-primary" onclick={saveRules} disabled={rulesSaving}>
          <Save size={13.5} />
          {rulesSaved ? t("settings.saved") : t("settings.saveHouseRules")}
        </button>
      </div>
    </section>
  {/snippet}

  {#snippet onlineSection()}
    <section class="card mb-6 px-4 py-5 min-[900px]:px-6">
      <label class="flex cursor-default items-start gap-3" for="online-research">
        <input
          id="online-research"
          name="online-research"
          type="checkbox"
          aria-describedby="online-research-help"
          class="mt-1.5 size-4 shrink-0 rounded border-paper-line accent-navy-500 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-navy-800"
          bind:checked={onlineResearch}
          onchange={() => void saveOnline()}
        />
        <span>
          <h2 class="text-[15px] font-semibold text-ink">{t("settings.online")}</h2>
          <p id="online-research-help" class="mt-1 text-[12.5px] leading-snug text-ink-soft">
            {t("settings.onlineHelp")}
          </p>
        </span>
      </label>
    </section>
  {/snippet}

  {#snippet aiSection()}
    {#if app.settings?.modelsFolder?.missing}
      {@render folderSection()}
    {/if}
    <section class="card mb-6 px-4 py-5 min-[900px]:px-6">
      <h2 class="mb-1 text-[15px] font-semibold text-ink">{t("settings.ai")}</h2>
      {#if machine}
        <p class="mb-4 flex flex-wrap items-center gap-1.5 text-[12px] text-ink-faint">
          <Cpu size={12.5} />
          {machine.profile.cpu} · {formatBytes(machine.profile.totalRamBytes)} memory
        </p>
        {#if runsEmulatedOnArm(machine.profile)}
          <div
            class="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-paper-line bg-paper-soft px-4 py-3 text-sm"
          >
            <p class="min-w-0 flex-1 text-ink-soft">{t("settings.armBuildNotice")}</p>
            <button
              type="button"
              class="btn-outline shrink-0"
              onclick={() =>
                (app.update?.switchesToArm
                  ? api.showUpdateWindow()
                  : api.openExternal("releases")
                ).catch(notifyInvokeError)}
            >
              {t("settings.armBuildDownload")}
            </button>
          </div>
        {/if}
      {/if}

      {#if modelDownload}
        <div class="mb-4 rounded-xl border border-paper-line bg-paper-soft px-4 py-3">
          <DownloadProgress
            download={modelDownload}
            cancelable
            note={app.settings?.activeModel ? t("settings.keepUsingChat") : undefined}
          />
        </div>
      {:else if engineDownload}
        <div class="mb-4 rounded-xl border border-paper-line bg-paper-soft px-4 py-3">
          <DownloadProgress download={engineDownload} cancelable />
        </div>
      {/if}

      <InstalledModels
        active={app.settings?.activeModel}
        others={app.settings?.otherModels ?? []}
        busy={!!modelDownload}
      >
        {#if app.engine.vision}
          <p class="mt-3 text-sm text-ink-soft">
            {t("images.ready", { count: app.engine.vision.maxImages })}
          </p>
        {:else if visionOffer}
          <div class="mt-3 flex flex-wrap items-center justify-between gap-3 text-sm">
            <p class="min-w-0 text-ink-soft">
              {t("images.enableDescription", { size: formatBytes(visionOffer) })}
            </p>
            <button
              type="button"
              class="btn-outline shrink-0"
              onclick={enableVision}
              disabled={enablingVision || chatBusy || !!modelDownload}
              >{t(enablingVision ? "images.enabling" : "images.enable")}</button
            >
          </div>
        {/if}
      </InstalledModels>
      {#if machine && machine.suggestions.length > 0}
        <div class="mt-5">
          <ModelSuggestionCards
            suggestions={machine.suggestions}
            installing={!!modelDownload}
            heading={app.settings?.activeModel ? t("settings.otherAis") : t("settings.suggested")}
            lede={app.settings?.activeModel
              ? t("settings.otherAisLede")
              : t("settings.suggestedLede")}
            onInstall={installRec}
          />
        </div>
      {/if}

      <div
        class="mt-5 flex flex-wrap items-center gap-4 border-t border-navy-950/10 pt-5 dark:border-white/10"
      >
        <div class="min-w-0 flex-1">
          <h3 class="text-[15px] font-semibold text-ink">{t("settings.exploreHeading")}</h3>
          <p class="mt-0.5 text-[12.5px] leading-snug text-ink-soft">
            {t("settings.exploreLede")}
          </p>
        </div>
        <button
          type="button"
          class="btn-outline shrink-0"
          aria-haspopup="dialog"
          aria-expanded={showExplore}
          aria-controls={showExplore ? "explore-models-dialog" : undefined}
          onclick={() => (showExplore = true)}
        >
          <Search size={13.5} aria-hidden="true" />
          {t("settings.browseAis")}
        </button>
      </div>
    </section>
    {#if !app.settings?.modelsFolder?.missing}
      {@render folderSection()}
    {/if}
  {/snippet}

  {#snippet folderSection()}
    {#if app.settings?.modelsFolder}
      <ModelsFolderCard
        folder={app.settings.modelsFolder}
        freeBytes={machine?.profile.freeDiskBytes}
        installedBytes={aiBytes}
        busy={!!modelDownload || !!engineDownload || chatBusy || enablingVision}
        onChanged={loadMachine}
      />
    {/if}
  {/snippet}

  <section class="card mb-6 px-4 py-5 min-[900px]:px-6">
    <div class="mb-4 flex flex-wrap items-start justify-between gap-3">
      <div class="min-w-0 flex-1">
        <h2 class="text-[15px] font-semibold text-ink">API local compatible con OpenAI</h2>
        <p class="mt-1 text-[12.5px] leading-snug text-ink-soft">
          Permite conectar clientes externos al modelo local. La API permanece detenida hasta iniciarla manualmente.
        </p>
      </div>
      <span class="rounded-full border border-paper-line px-2.5 py-1 text-[11px] text-ink-soft">
        {localApiStatus?.running ? "En ejecución" : "Detenida"}
      </span>
    </div>
    <div class="grid gap-3 sm:grid-cols-[10rem_1fr]">
      <label class="text-[12px] font-medium text-ink-soft" for="local-api-port">
        Puerto
        <input
          id="local-api-port"
          type="number"
          min="1"
          max="65535"
          class="input mt-1 w-full"
          bind:value={apiPort}
          disabled={apiBusy || !!localApiStatus?.running}
        />
      </label>
      <label class="flex cursor-default items-center gap-2 self-end pb-2 text-[12.5px] text-ink-soft">
        <input
          type="checkbox"
          class="size-4 accent-navy-700"
          bind:checked={apiLan}
          disabled={apiBusy || !!localApiStatus?.running}
        />
        Permitir conexiones desde la red local (LAN)
      </label>
    </div>
    <div class="mt-3 flex flex-wrap gap-2">
      <button type="button" class="btn-outline" onclick={saveApiNetwork} disabled={apiBusy || !!localApiStatus?.running}>
        Guardar red
      </button>
      <button type="button" class="btn-outline" onclick={generateApiKey} disabled={apiBusy}>
        Generar / rotar API key
      </button>
      {#if localApiStatus?.keyConfigured}
        <button type="button" class="btn-outline" onclick={revokeApiKey} disabled={apiBusy}>
          Revocar API key
        </button>
      {/if}
      {#if localApiStatus?.running}
        <button type="button" class="btn-primary" onclick={stopLocalApi} disabled={apiBusy}>Detener API</button>
      {:else}
        <button type="button" class="btn-primary" onclick={startLocalApi} disabled={apiBusy || !localApiStatus?.keyConfigured}>Iniciar API</button>
      {/if}
    </div>
    {#if localApiStatus}
      <p class="mt-3 break-all font-mono text-[11.5px] text-ink-soft">
        Base URL: {localApiStatus.baseUrl}
      </p>
      <p class="mt-1 text-[11.5px] text-ink-faint">
        Autenticación Bearer: {localApiStatus.keyConfigured ? "configurada" : "falta generar una clave"}.
        Endpoint: /v1/chat/completions
      </p>
    {/if}
    {#if apiKey}
      <div class="mt-3 rounded-lg border border-paper-line bg-paper-soft p-3">
        <p class="mb-1 text-[12px] font-semibold text-ink">Clave generada — copiala ahora</p>
        <code class="block select-text break-all text-[12px] text-ink">{apiKey}</code>
        <p class="mt-1 text-[11px] text-ink-faint">Por seguridad, Rebost guarda únicamente su hash y no volverá a mostrar esta clave.</p>
      </div>
    {/if}
    {#if apiError}
      <p role="alert" class="mt-3 text-sm text-red-700 dark:text-red-400">{apiError}</p>
    {/if}
  </section>

  <section class="card mb-6 px-4 py-5 min-[900px]:px-6">
    <div class="flex flex-wrap items-center gap-4">
      <img src={icon} alt="" class="h-12 w-12 rounded-[22%]" />
      <div class="min-w-0 flex-1">
        <h2 class="text-[15px] font-semibold text-ink">{t("settings.aboutHeading")}</h2>
        <p class="mt-0.5 text-[12.5px] leading-snug text-ink-soft">
          {t("officialLineShort")}
        </p>
      </div>
      <button
        type="button"
        class="btn-outline shrink-0"
        onclick={() => api.showAboutWindow().catch(notifyInvokeError)}
      >
        {t("settings.open")}
      </button>
    </div>
  </section>

  <section class="card mb-6 px-4 py-5 min-[900px]:px-6">
    <div class="flex flex-wrap items-center gap-4">
      <div class="min-w-0 flex-1">
        <h2 class="text-[15px] font-semibold text-ink">{t("settings.resetHeading")}</h2>
        <p class="mt-0.5 text-[12.5px] leading-snug text-ink-soft">
          {t("settings.resetHelp")}
        </p>
      </div>
      <button
        type="button"
        class="btn-outline shrink-0 text-red-700 hover:border-red-300 hover:bg-red-50 dark:text-red-400 dark:hover:border-red-400/40 dark:hover:bg-red-400/10"
        aria-haspopup="dialog"
        aria-expanded={showReset}
        aria-controls={showReset ? "reset-workspace-dialog" : undefined}
        onclick={() => (showReset = true)}
      >
        {t("settings.reset")}
      </button>
    </div>
  </section>

  <section class="mb-10">
    <button type="button" class="btn-ghost !text-[12px]" onclick={openDiagnostics}>
      <Stethoscope size={13} />
      {showDiag ? t("settings.hideDiagnostics") : t("settings.diagnostics")}
    </button>
    {#if showDiag && hasAi}
      <div class="mt-2 flex flex-wrap items-center gap-3">
        <button
          type="button"
          class="btn-outline"
          onclick={measureAgain}
          disabled={measuring || chatBusy || !!modelDownload || !!engineDownload}
        >
          {measuring ? t("settings.measuring") : t("settings.measureAgain")}
        </button>
        <p role="status" class="text-sm text-ink-soft">{measureStatus}</p>
      </div>
    {/if}
    {#if showDiag && diag}
      <div
        class="card mt-2 px-4 py-4 font-mono text-[11.5px] leading-relaxed break-words text-ink-soft"
      >
        <p>
          Rebost {diag.version} · engine {diag.engineBuild} ({diag.engineState.state}{diag
            .engineState.detail
            ? ` · ${diag.engineState.detail}`
            : ""})
        </p>
        <p>data: {diag.dataDir}</p>
        <p class="mt-1 text-[10.5px] text-ink-faint">
          {t("settings.dataFolderHint")}
        </p>
        <p>
          model: {diag.model
            ? `${diag.model.name} · ${diag.model.file}`
            : t("settings.noneInstalled")}
        </p>
        {#if diag.model?.projector || diag.visionError}
          <p>
            images: {diag.engineState.vision
              ? "ready"
              : diag.visionError
                ? `failed · ${diag.visionError}`
                : "off"}
          </p>
        {/if}
        <p>index records: {diag.indexRecords} · context budget: {diag.contextBudgetChars} chars</p>
        {#if diag.benchmark}
          <p>
            benchmark: {diag.benchmark.promptTokensPerSecond.toFixed(0)} pp tok/s · {diag.benchmark.generationTokensPerSecond.toFixed(
              0,
            )} gen tok/s · {diag.benchmark.measuredAt}
          </p>
        {/if}
        <p>
          machine: {diag.machine.cpu} · {formatBytes(diag.machine.totalRamBytes)} RAM · {diag
            .machine.accelerator}
          {diag.machine.processArch !== diag.machine.osArch
            ? ` · ${diag.machine.processArch} on ${diag.machine.osArch}`
            : ` · ${diag.machine.processArch}`}
        </p>
        <p>formats: {diag.supportedFormats.join(" ")}</p>
        {#if diag.engineLogPresent}
          <p class="mt-2">
            engine log location:
            <button
              type="button"
              class="rounded-sm text-left break-all underline decoration-navy-200 underline-offset-2 hover:text-ink hover:decoration-ink focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-navy-800"
              aria-label={t("settings.openEngineLog")}
              onclick={() => api.openEngineLog().catch(notifyInvokeError)}
            >
              <code>{diag.engineLogPath}</code>
            </button>
          </p>
          <p class="mt-1 text-[10.5px] leading-relaxed text-ink-faint">
            {t("settings.logOnDisk")}
          </p>
        {/if}
      </div>
    {/if}
  </section>
</div>

{#if showExplore}
  <ExploreModelsModal
    installing={!!modelDownload}
    onClose={() => (showExplore = false)}
    onInstall={(result) => {
      installFrom(result);
      showExplore = false;
    }}
    onUse={(file) => {
      showExplore = false;
      void api
        .modelUse(file)
        .catch(notifyInvokeError)
        .finally(() => void refreshSettings().catch(notifyInvokeError));
    }}
  />
{/if}
{#if showReset}
  <ResetWorkspaceModal
    busy={resetting}
    onClose={() => {
      if (!resetting) showReset = false;
    }}
    onConfirm={resetWorkspace}
  />
{/if}
