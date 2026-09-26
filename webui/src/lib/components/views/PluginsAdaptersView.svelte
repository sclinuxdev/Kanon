<script lang="ts">
import {
  AlertTriangle,
  Boxes,
  CheckCircle2,
  ChevronDown,
  Folder,
  Play,
  Plus,
  Radio,
  RefreshCw,
  Save,
  Send,
  Settings,
  Upload,
  X,
  XCircle,
} from 'lucide-svelte';
import { api } from '../../api/client';
import { t } from '../../stores/i18n.svelte';
import type {
  AdapterItem,
  PluginConfigResponse,
  PluginHost,
} from '../../types';

let hosts = $state<PluginHost[]>([]);
let adapters = $state<AdapterItem[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);

// Install Plugin Modal
let installModalOpen = $state(false);
let installTab = $state<'path' | 'archive'>('path');
let localPathInput = $state('');
let archiveFile = $state<File | null>(null);
let installLoading = $state(false);
let installError = $state<string | null>(null);
let installSuccess = $state<string | null>(null);

function openInstallModal() {
  installModalOpen = true;
  installError = null;
  installSuccess = null;
  localPathInput = '';
  archiveFile = null;
}

function handleArchiveFileChange(event: Event) {
  const target = event.target as HTMLInputElement;
  if (target.files && target.files.length > 0) {
    archiveFile = target.files[0];
  } else {
    archiveFile = null;
  }
}

async function handleInstall() {
  installLoading = true;
  installError = null;
  installSuccess = null;
  try {
    if (installTab === 'path') {
      if (!localPathInput.trim()) {
        throw new Error('Please enter a local directory path');
      }
      const res = await api.installPluginPath(localPathInput.trim());
      installSuccess = `${t('plugins.install_success')} (${res.plugin_id}, status: ${res.status})`;
    } else {
      if (!archiveFile) {
        throw new Error('Please select a .kpk or .zip archive file');
      }
      const res = await api.installPluginArchive(archiveFile);
      installSuccess = `${t('plugins.install_success')} (${res.plugin_id}, status: ${res.status})`;
    }
    await loadData();
  } catch (e) {
    installError = e instanceof Error ? e.message : String(e);
  } finally {
    installLoading = false;
  }
}

// Selected plugin for config modal / panel
let selectedPluginId = $state<string | null>(null);
let currentConfig = $state<PluginConfigResponse | null>(null);
let configEditRaw = $state<string>('');
let configSaving = $state(false);
let configStatusMsg = $state<string | null>(null);

// Quick Ingest Test Form
let testPlatform = $state('webhook');
let testChannel = $state('general');
let testSender = $state('alice');
let testMessage = $state('Hello Kanon!');
let ingesting = $state(false);
let ingestResult = $state<string | null>(null);

async function loadData() {
  loading = true;
  error = null;
  try {
    const [pluginsRes, adaptersRes] = await Promise.all([
      api.getPlugins(),
      api.getAdapters(),
    ]);
    hosts = pluginsRes.hosts;
    adapters = adaptersRes.adapters;
    if (
      adapters.length > 0 &&
      !adapters.some((a) => a.platform === testPlatform)
    ) {
      testPlatform = adapters[0].platform;
    }
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  } finally {
    loading = false;
  }
}

async function restartHost(hostId: string) {
  if (!confirm(`Are you sure you want to restart plugin host '${hostId}'?`))
    return;
  try {
    await api.restartPlugin(hostId);
    await loadData();
  } catch (e) {
    alert(`Restart failed: ${e instanceof Error ? e.message : String(e)}`);
  }
}

async function openConfig(pluginId: string) {
  selectedPluginId = pluginId;
  configStatusMsg = null;
  try {
    const res = await api.getPluginConfig(pluginId);
    currentConfig = res;
    configEditRaw = JSON.stringify(res.config, null, 2);
  } catch (e) {
    currentConfig = null;
    configStatusMsg = `Failed to fetch config: ${e instanceof Error ? e.message : String(e)}`;
  }
}

async function saveConfig() {
  if (!selectedPluginId || !currentConfig) return;
  configSaving = true;
  configStatusMsg = null;
  try {
    const parsed = JSON.parse(configEditRaw);
    const res = await api.updatePluginConfig(
      selectedPluginId,
      parsed,
      currentConfig.cas_version,
    );
    currentConfig.cas_version = res.cas_version;
    currentConfig.config = parsed;
    configStatusMsg = 'Configuration saved successfully (CAS enforced).';
  } catch (e) {
    configStatusMsg = `Save failed: ${e instanceof Error ? e.message : String(e)}`;
  } finally {
    configSaving = false;
  }
}

async function sendTestEvent() {
  ingesting = true;
  ingestResult = null;
  try {
    const res = await api.ingestEvent(testPlatform, {
      channel_id: testChannel,
      sender_id: testSender,
      text: testMessage,
      event_id: `test_${Date.now()}`,
    });
    ingestResult = `Fast-ACK Accepted! Event ID: ${res.event_id}`;
  } catch (e) {
    ingestResult = `Ingest rejected: ${e instanceof Error ? e.message : String(e)}`;
  } finally {
    ingesting = false;
  }
}

$effect(() => {
  loadData();
});
</script>

<div class="p-6 space-y-6 max-w-7xl mx-auto">
  <!-- Top bar -->
  <div class="flex items-center justify-between">
    <div>
      <h3 class="text-base sm:text-lg font-semibold text-zinc-900 dark:text-zinc-100 tracking-tight">{t('plugins.hosts_title')}</h3>
      <p class="text-xs sm:text-sm text-zinc-500">{t('subtitle.plugins')}</p>
    </div>
    <div class="flex items-center gap-2">
      <button
        onclick={openInstallModal}
        class="px-3 py-1.5 text-xs sm:text-sm font-medium rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white transition cursor-pointer flex items-center gap-1.5 shadow-2xs"
      >
        <Plus class="w-4 h-4" />
        <span>{t('plugins.install')}</span>
      </button>
      <button
        onclick={loadData}
        class="px-3 py-1.5 text-xs sm:text-sm font-medium rounded-lg bg-white dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700 transition cursor-pointer flex items-center gap-1.5 shadow-2xs"
      >
        <RefreshCw class="w-4 h-4" />
        <span>{t('common.refresh')}</span>
      </button>
    </div>
  </div>

  {#if loading}
    <div class="p-12 text-center text-sm text-zinc-400">{t('common.loading')}</div>
  {:else if error}
    <div class="p-4 rounded-lg bg-rose-500/10 border border-rose-500/20 text-rose-600 dark:text-rose-400 text-sm">
      {error}
    </div>
  {:else}
    <!-- Grid of Plugin Hosts -->
    <div class="grid grid-cols-1 gap-4">
      {#if hosts.length === 0}
        <div class="p-8 rounded-xl border border-dashed border-zinc-300 dark:border-zinc-800 text-center text-zinc-400 text-sm">
          {t('plugins.no_hosts')}
        </div>
      {:else}
        {#each hosts as host}
          <div class="p-5 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs space-y-3.5">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2.5">
                <Boxes class="w-4.5 h-4.5 text-zinc-500" />
                <span class="font-mono font-bold text-base text-zinc-900 dark:text-zinc-100">{host.host_id}</span>
                {#if host.status === 'RuntimeUnavailable'}
                  <span class="px-2 py-0.5 text-xs font-mono bg-amber-500/10 border border-amber-500/20 rounded text-amber-600 dark:text-amber-400">
                    {t('plugins.runtime_unavailable')}
                  </span>
                {:else}
                  <span class="px-2 py-0.5 text-xs font-mono bg-zinc-100 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded text-zinc-600 dark:text-zinc-400">
                    runtime: {host.runtime} (PID: {host.pid ?? 'N/A'})
                  </span>
                {/if}
              </div>
              <div class="flex items-center gap-2">
                <button
                  onclick={() => restartHost(host.host_id)}
                  class="px-3 py-1.5 text-xs sm:text-sm text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100 border border-zinc-200 dark:border-zinc-700 rounded-md hover:bg-zinc-100 dark:hover:bg-zinc-800 transition cursor-pointer"
                >
                  {t('plugins.restart')}
                </button>
              </div>
            </div>

            <!-- Nested Plugins list -->
            <div class="space-y-2.5 pt-2 border-t border-zinc-100 dark:border-zinc-800">
              {#each host.plugins as plugin}
                <div class="p-3.5 rounded-lg bg-zinc-50 dark:bg-zinc-950/60 border border-zinc-200/80 dark:border-zinc-800/80 space-y-2">
                  <div class="flex items-center justify-between">
                    <div>
                      <span class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">{plugin.name}</span>
                      <span class="text-xs font-mono text-zinc-500 ml-2">v{plugin.version} ({plugin.id})</span>
                    </div>
                    <button
                      onclick={() => openConfig(plugin.id)}
                      class="px-2.5 py-1 text-xs font-medium text-indigo-600 dark:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-950/40 rounded transition cursor-pointer flex items-center gap-1"
                    >
                      <Settings class="w-3.5 h-3.5" />
                      <span>{t('plugins.config')}</span>
                    </button>
                  </div>

                  <!-- Commands & Tools descriptors badges -->
                  <div class="flex flex-wrap gap-2 text-xs font-mono">
                    {#each plugin.commands as cmd}
                      <span class="px-2 py-0.5 rounded bg-violet-500/10 text-violet-600 dark:text-violet-400 border border-violet-500/20" title={cmd.description}>
                        cmd: /{cmd.name}
                      </span>
                    {/each}
                    {#each plugin.tools as tool}
                      <span class="px-2 py-0.5 rounded bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 border border-indigo-500/20" title={tool.description}>
                        tool: {tool.name}
                      </span>
                    {/each}
                  </div>
                </div>
              {/each}
            </div>
          </div>
        {/each}
      {/if}
    </div>

    <!-- Platform Adapters Section & Test Ingest -->
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-6 pt-4 border-t border-zinc-200 dark:border-zinc-800">
      <!-- Left: Adapters Status -->
      <div class="p-5 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs space-y-3.5">
        <div class="flex items-center gap-2">
          <Radio class="w-4.5 h-4.5 text-zinc-500" />
          <h4 class="text-sm sm:text-base font-semibold text-zinc-900 dark:text-zinc-100">{t('plugins.adapters_title')} ({adapters.length})</h4>
        </div>
        <div class="space-y-2">
          {#each adapters as adapter}
            <div class="p-3 rounded-lg border border-zinc-200 dark:border-zinc-800 flex items-center justify-between text-xs sm:text-sm">
              <div>
                <span class="font-semibold text-zinc-800 dark:text-zinc-200">{adapter.display_name}</span>
                <span class="text-xs font-mono text-zinc-500 ml-2">({adapter.platform})</span>
              </div>
              <span class="px-2 py-0.5 rounded text-xs font-mono {adapter.connected ? 'bg-emerald-500/10 text-emerald-600 border border-emerald-500/20' : 'bg-zinc-500/10 text-zinc-500 border border-zinc-500/20'}">
                {adapter.connected ? 'Active' : 'Inbound-only'}
              </span>
            </div>
          {/each}
        </div>
      </div>

      <!-- Right: Fast-ACK Quick Ingest Box -->
      <div class="p-5 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs space-y-3.5">
        <div class="flex items-center gap-2">
          <Send class="w-4.5 h-4.5 text-zinc-500" />
          <h4 class="text-sm sm:text-base font-semibold text-zinc-900 dark:text-zinc-100">Simulate Inbound Event (Fast-ACK)</h4>
        </div>
        <div class="space-y-3 text-xs sm:text-sm">
          <div class="grid grid-cols-3 gap-2.5">
            <div>
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="block text-xs font-sans text-zinc-500 mb-1">Platform</label>
              <input
                type="text"
                bind:value={testPlatform}
                class="w-full px-3 py-2 rounded-lg bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 font-mono text-xs sm:text-sm focus:outline-hidden"
              />
            </div>
            <div>
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="block text-xs font-sans text-zinc-500 mb-1">Channel ID</label>
              <input
                type="text"
                bind:value={testChannel}
                class="w-full px-3 py-2 rounded-lg bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 font-mono text-xs sm:text-sm focus:outline-hidden"
              />
            </div>
            <div>
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="block text-xs font-sans text-zinc-500 mb-1">Sender ID</label>
              <input
                type="text"
                bind:value={testSender}
                class="w-full px-3 py-2 rounded-lg bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 font-mono text-xs sm:text-sm focus:outline-hidden"
              />
            </div>
          </div>
          <div>
            <!-- svelte-ignore a11y_label_has_associated_control -->
            <label class="block text-xs font-sans text-zinc-500 mb-1">Message Content</label>
            <input
              type="text"
              bind:value={testMessage}
              class="w-full px-3 py-2 rounded-lg bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 text-xs sm:text-sm focus:outline-hidden"
            />
          </div>
          <button
            onclick={sendTestEvent}
            disabled={ingesting}
            class="w-full py-2 bg-zinc-900 hover:bg-zinc-800 dark:bg-zinc-100 dark:hover:bg-zinc-200 text-white dark:text-zinc-900 rounded-lg font-medium transition cursor-pointer text-xs sm:text-sm disabled:opacity-50"
          >
            {ingesting ? 'Pushing into Tokio queue...' : 'Send Event to Pipeline'}
          </button>
          {#if ingestResult}
            <div class="p-2.5 rounded-lg bg-zinc-100 dark:bg-zinc-800 font-mono text-xs text-zinc-700 dark:text-zinc-300">
              {ingestResult}
            </div>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>

<!-- Config Modal Drawer -->
{#if selectedPluginId}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="fixed inset-0 bg-black/40 backdrop-blur-xs z-50 flex items-center justify-center p-4"
    onclick={() => (selectedPluginId = null)}
    role="button"
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="w-full max-w-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl shadow-2xl p-6 space-y-4"
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      tabindex="-1"
    >
      <div class="flex items-center justify-between border-b border-zinc-200 dark:border-zinc-800 pb-3">
        <div>
          <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">Plugin Config: {selectedPluginId}</h3>
          <span class="text-xs font-mono text-zinc-500">CAS Lock Version: {currentConfig?.cas_version ?? 0}</span>
        </div>
        <button
          onclick={() => (selectedPluginId = null)}
          class="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 text-xs sm:text-sm font-mono cursor-pointer"
        >
          {t('common.close')}
        </button>
      </div>

      {#if configStatusMsg}
        <div class="p-3 text-xs sm:text-sm rounded-lg bg-zinc-100 dark:bg-zinc-800 font-mono text-zinc-700 dark:text-zinc-300">
          {configStatusMsg}
        </div>
      {/if}

      <div>
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="block text-xs sm:text-sm font-medium text-zinc-600 dark:text-zinc-400 mb-1.5">Configuration (JSON)</label>
        <textarea
          bind:value={configEditRaw}
          rows={10}
          class="w-full p-3 bg-zinc-950 font-mono text-xs sm:text-sm text-zinc-200 border border-zinc-800 rounded-lg focus:outline-hidden"
        ></textarea>
      </div>

      <div class="flex items-center justify-end gap-2 pt-2">
        <button
          onclick={() => (selectedPluginId = null)}
          class="px-3.5 py-2 text-xs sm:text-sm text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800 rounded-lg transition cursor-pointer"
        >
          {t('common.cancel')}
        </button>
        <button
          onclick={saveConfig}
          disabled={configSaving}
          class="px-4 py-2 text-xs sm:text-sm bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg font-medium transition cursor-pointer flex items-center gap-1.5 disabled:opacity-50"
        >
          <Save class="w-4 h-4" />
          <span>{configSaving ? 'Enforcing CAS...' : t('common.save')}</span>
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Install Plugin Modal -->
{#if installModalOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="fixed inset-0 bg-black/40 backdrop-blur-xs z-50 flex items-center justify-center p-4"
    onclick={() => (installModalOpen = false)}
    role="button"
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="w-full max-w-lg bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl shadow-2xl p-6 space-y-4"
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      tabindex="-1"
    >
      <div class="flex items-center justify-between border-b border-zinc-200 dark:border-zinc-800 pb-3">
        <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100 flex items-center gap-2">
          <Plus class="w-4 h-4 text-indigo-500" />
          <span>{t('plugins.install_modal_title')}</span>
        </h3>
        <button
          onclick={() => (installModalOpen = false)}
          class="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 p-1 cursor-pointer"
        >
          <X class="w-4 h-4" />
        </button>
      </div>

      <!-- Tab switcher -->
      <div class="flex rounded-lg bg-zinc-100 dark:bg-zinc-800/80 p-1 text-xs sm:text-sm">
        <button
          type="button"
          onclick={() => { installTab = 'path'; installError = null; installSuccess = null; }}
          class="flex-1 py-1.5 px-3 rounded-md font-medium transition cursor-pointer flex items-center justify-center gap-1.5 {installTab === 'path' ? 'bg-white dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100 shadow-2xs' : 'text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200'}"
        >
          <Folder class="w-3.5 h-3.5" />
          <span>{t('plugins.tab_local_path')}</span>
        </button>
        <button
          type="button"
          onclick={() => { installTab = 'archive'; installError = null; installSuccess = null; }}
          class="flex-1 py-1.5 px-3 rounded-md font-medium transition cursor-pointer flex items-center justify-center gap-1.5 {installTab === 'archive' ? 'bg-white dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100 shadow-2xs' : 'text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200'}"
        >
          <Upload class="w-3.5 h-3.5" />
          <span>{t('plugins.tab_upload_archive')}</span>
        </button>
      </div>

      <!-- Tab content -->
      {#if installTab === 'path'}
        <div class="space-y-1.5">
          <!-- svelte-ignore a11y_label_has_associated_control -->
          <label class="block text-xs sm:text-sm font-medium text-zinc-700 dark:text-zinc-300">
            {t('plugins.local_path_label')}
          </label>
          <input
            type="text"
            bind:value={localPathInput}
            placeholder={t('plugins.local_path_placeholder')}
            class="w-full px-3 py-2 rounded-lg bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 font-mono text-xs sm:text-sm focus:outline-hidden"
          />
          <p class="text-xs text-zinc-500">{t('plugins.local_path_help')}</p>
        </div>
      {:else}
        <div class="space-y-1.5">
          <!-- svelte-ignore a11y_label_has_associated_control -->
          <label class="block text-xs sm:text-sm font-medium text-zinc-700 dark:text-zinc-300">
            {t('plugins.archive_label')}
          </label>
          <input
            type="file"
            accept=".kpk,.zip,application/zip"
            onchange={handleArchiveFileChange}
            class="w-full text-xs text-zinc-500 file:mr-3 file:py-2 file:px-3 file:rounded-lg file:border-0 file:text-xs file:font-medium file:bg-zinc-100 dark:file:bg-zinc-800 file:text-zinc-700 dark:file:text-zinc-200 hover:file:bg-zinc-200 dark:hover:file:bg-zinc-700 cursor-pointer"
          />
          <p class="text-xs text-zinc-500">{t('plugins.archive_help')}</p>
        </div>
      {/if}

      <!-- Status alerts -->
      {#if installError}
        <div class="p-3 rounded-lg bg-rose-500/10 border border-rose-500/20 text-rose-600 dark:text-rose-400 text-xs sm:text-sm flex items-start gap-2">
          <AlertTriangle class="w-4 h-4 shrink-0 mt-0.5" />
          <span>{installError}</span>
        </div>
      {/if}

      {#if installSuccess}
        <div class="p-3 rounded-lg bg-emerald-500/10 border border-emerald-500/20 text-emerald-600 dark:text-emerald-400 text-xs sm:text-sm flex items-start gap-2">
          <CheckCircle2 class="w-4 h-4 shrink-0 mt-0.5" />
          <span>{installSuccess}</span>
        </div>
      {/if}

      <!-- Action buttons -->
      <div class="flex items-center justify-end gap-2 pt-2 border-t border-zinc-100 dark:border-zinc-800">
        <button
          onclick={() => (installModalOpen = false)}
          disabled={installLoading}
          class="px-3.5 py-2 text-xs sm:text-sm text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800 rounded-lg transition cursor-pointer"
        >
          {t('common.cancel')}
        </button>
        <button
          onclick={handleInstall}
          disabled={installLoading || (installTab === 'path' ? !localPathInput.trim() : !archiveFile)}
          class="px-4 py-2 text-xs sm:text-sm bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg font-medium transition cursor-pointer flex items-center gap-1.5 disabled:opacity-50"
        >
          {#if installLoading}
            <RefreshCw class="w-4 h-4 animate-spin" />
            <span>{t('plugins.installing')}</span>
          {:else}
            <Plus class="w-4 h-4" />
            <span>{t('plugins.install_btn')}</span>
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}

