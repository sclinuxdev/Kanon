<script lang="ts">
import {
  AlertTriangle,
  Boxes,
  Check,
  CheckCircle2,
  ChevronDown,
  Copy,
  ExternalLink,
  Eye,
  EyeOff,
  Folder,
  Play,
  Plus,
  QrCode,
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
let configTab = $state<'visual' | 'raw'>('visual');

// Visual form values for QQ Official Adapter
let qqAppId = $state('');
let qqSecret = $state('');
let qqSecretVisible = $state(false);
let qqIsSandbox = $state(false);
let qqEnableGroupC2C = $state(true);
let qqEnableGuildDm = $state(false);
let qqUseMarkdown = $state(false);
let qqMarkdownTemplateId = $state('');
let qqMarkdownParamsKey = $state('text');

// QR Code Quick Login Modal State
let qrModalOpen = $state(false);
let qrTaskId = $state<string | null>(null);
let qrBindKey = $state<string | null>(null);
let qrCodeUrl = $state<string | null>(null);
let qrStatus = $state<
  'idle' | 'generating' | 'waiting' | 'success' | 'expired' | 'error'
>('idle');
let qrStatusMsg = $state<string | null>(null);
let qrBoundAppId = $state<string | null>(null);
let qrCopied = $state(false);
let qrPollTimer: ReturnType<typeof setInterval> | null = null;

function syncVisualToRaw() {
  let parsed: Record<string, unknown> = {};
  try {
    parsed = JSON.parse(configEditRaw);
  } catch {
    parsed = {};
  }
  parsed.appid = qqAppId;
  parsed.secret = qqSecret;
  parsed.is_sandbox = qqIsSandbox;
  parsed.enable_group_c2c = qqEnableGroupC2C;
  parsed.enable_guild_direct_message = qqEnableGuildDm;
  parsed.use_markdown = qqUseMarkdown;
  if (qqMarkdownTemplateId) parsed.markdown_template_id = qqMarkdownTemplateId;
  else delete parsed.markdown_template_id;
  parsed.markdown_params_key = qqMarkdownParamsKey || 'text';

  configEditRaw = JSON.stringify(parsed, null, 2);
}

function syncRawToVisual() {
  try {
    const parsed = JSON.parse(configEditRaw);
    if (parsed && typeof parsed === 'object') {
      qqAppId = String(parsed.appid || '');
      qqSecret = String(parsed.secret || '');
      qqIsSandbox = Boolean(parsed.is_sandbox);
      qqEnableGroupC2C = parsed.enable_group_c2c !== false;
      qqEnableGuildDm = Boolean(parsed.enable_guild_direct_message);
      qqUseMarkdown = Boolean(parsed.use_markdown);
      qqMarkdownTemplateId = String(parsed.markdown_template_id || '');
      qqMarkdownParamsKey = String(parsed.markdown_params_key || 'text');
    }
  } catch {
    // ignore
  }
}

async function openQrLoginModal() {
  qrModalOpen = true;
  qrStatus = 'generating';
  qrStatusMsg = null;
  qrTaskId = null;
  qrBindKey = null;
  qrCodeUrl = null;
  qrBoundAppId = null;
  qrCopied = false;
  stopQrPolling();

  try {
    const res = await api.requestQQOfficialLoginQr();
    qrTaskId = res.task_id;
    qrBindKey = res.bind_key;
    qrCodeUrl = res.qrcode_url;
    qrStatus = 'waiting';

    const intervalMs = Math.max(res.poll_interval_seconds || 2, 1) * 1000;
    qrPollTimer = setInterval(async () => {
      if (!qrTaskId || !qrBindKey) return;
      try {
        const pollRes = await api.pollQQOfficialLogin(qrTaskId, qrBindKey);
        if (pollRes.status === 'created') {
          stopQrPolling();
          qrStatus = 'success';
          qrBoundAppId = pollRes.appid ?? '';
          if (pollRes.appid) qqAppId = pollRes.appid;
          if (pollRes.secret) qqSecret = pollRes.secret;
          syncVisualToRaw();
          await loadData();
        } else if (pollRes.status === 'expired') {
          stopQrPolling();
          qrStatus = 'expired';
        } else if (pollRes.status === 'error') {
          stopQrPolling();
          qrStatus = 'error';
          qrStatusMsg = pollRes.message || 'Error polling authorization status';
        }
      } catch {
        // Keep polling on transient network glitch
      }
    }, intervalMs);
  } catch (e) {
    qrStatus = 'error';
    qrStatusMsg = e instanceof Error ? e.message : String(e);
  }
}

function stopQrPolling() {
  if (qrPollTimer) {
    clearInterval(qrPollTimer);
    qrPollTimer = null;
  }
}

function closeQrLoginModal() {
  stopQrPolling();
  qrModalOpen = false;
}

async function copyQrUrl() {
  if (!qrCodeUrl) return;
  try {
    await navigator.clipboard.writeText(qrCodeUrl);
    qrCopied = true;
    setTimeout(() => {
      qrCopied = false;
    }, 2000);
  } catch {
    // clipboard
  }
}

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
  configTab = pluginId === 'org.kanon.adapter.qqofficial' ? 'visual' : 'raw';
  try {
    const res = await api.getPluginConfig(pluginId);
    currentConfig = res;
    configEditRaw = JSON.stringify(res.config, null, 2);
    if (pluginId === 'org.kanon.adapter.qqofficial') {
      syncRawToVisual();
    }
  } catch (e) {
    currentConfig = null;
    configStatusMsg = `Failed to fetch config: ${e instanceof Error ? e.message : String(e)}`;
  }
}

async function saveConfig() {
  if (!selectedPluginId || !currentConfig) return;
  if (
    selectedPluginId === 'org.kanon.adapter.qqofficial' &&
    configTab === 'visual'
  ) {
    syncVisualToRaw();
  }
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
            <div class="p-3.5 rounded-lg border border-zinc-200 dark:border-zinc-800 flex flex-col sm:flex-row sm:items-center justify-between gap-2 text-xs sm:text-sm">
              <div class="flex items-center gap-2">
                <span class="font-semibold text-zinc-800 dark:text-zinc-200">{adapter.display_name}</span>
                <span class="text-xs font-mono text-zinc-500">({adapter.platform})</span>
                <span class="px-2 py-0.5 rounded text-xs font-mono {adapter.connected ? 'bg-emerald-500/10 text-emerald-600 border border-emerald-500/20' : 'bg-zinc-500/10 text-zinc-500 border border-zinc-500/20'}">
                  {adapter.connected ? 'Active' : 'Inbound-only'}
                </span>
              </div>
              <div class="flex items-center gap-2 self-end sm:self-auto">
                {#if adapter.platform === 'qqofficial'}
                  <button
                    onclick={openQrLoginModal}
                    class="px-2.5 py-1 text-xs font-medium text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 hover:bg-emerald-500/20 border border-emerald-500/20 rounded-md transition cursor-pointer flex items-center gap-1 shadow-2xs"
                    title={t('adapters.qq_qr_title')}
                  >
                    <QrCode class="w-3.5 h-3.5" />
                    <span>{t('adapters.qq_qr_btn')}</span>
                  </button>
                  <button
                    onclick={() => openConfig('org.kanon.adapter.qqofficial')}
                    class="px-2.5 py-1 text-xs font-medium text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100 hover:bg-zinc-100 dark:hover:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-md transition cursor-pointer flex items-center gap-1"
                  >
                    <Settings class="w-3.5 h-3.5" />
                    <span>{t('plugins.config')}</span>
                  </button>
                {/if}
              </div>
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

      {#if selectedPluginId === 'org.kanon.adapter.qqofficial'}
        <!-- Tabs for QQ Official: Visual Form vs Raw JSON -->
        <div class="flex items-center gap-2 border-b border-zinc-200 dark:border-zinc-800 pb-2">
          <button
            onclick={() => { configTab = 'visual'; syncRawToVisual(); }}
            class="px-3 py-1.5 text-xs font-medium rounded-lg transition cursor-pointer {configTab === 'visual' ? 'bg-indigo-600 text-white' : 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800'}"
          >
            {t('adapters.qq_visual_config')}
          </button>
          <button
            onclick={() => { configTab = 'raw'; syncVisualToRaw(); }}
            class="px-3 py-1.5 text-xs font-medium rounded-lg transition cursor-pointer {configTab === 'raw' ? 'bg-indigo-600 text-white' : 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800'}"
          >
            原始 JSON
          </button>
        </div>
      {/if}

      {#if selectedPluginId === 'org.kanon.adapter.qqofficial' && configTab === 'visual'}
        <!-- QR Quick Bind Banner -->
        <div class="p-3.5 rounded-xl bg-gradient-to-r from-emerald-500/10 via-teal-500/10 to-transparent border border-emerald-500/20 flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div class="space-y-0.5">
            <div class="font-semibold text-xs sm:text-sm text-emerald-800 dark:text-emerald-300 flex items-center gap-1.5">
              <QrCode class="w-4 h-4 text-emerald-600" />
              <span>{t('adapters.qq_qr_btn')}</span>
            </div>
            <p class="text-xs text-zinc-500 dark:text-zinc-400">使用手机 QQ 扫码一键写入并激活机器人凭据，免去手动查找</p>
          </div>
          <button
            onclick={openQrLoginModal}
            class="px-3 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg text-xs font-medium transition cursor-pointer shadow-2xs flex items-center gap-1.5 shrink-0 self-start sm:self-auto"
          >
            <QrCode class="w-3.5 h-3.5" />
            <span>立即扫码绑定</span>
          </button>
        </div>

        <!-- Visual Form Fields -->
        <div class="space-y-3.5 text-xs sm:text-sm">
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div>
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="block text-xs font-medium text-zinc-600 dark:text-zinc-400 mb-1">{t('adapters.qq_appid')}</label>
              <input
                type="text"
                bind:value={qqAppId}
                oninput={syncVisualToRaw}
                placeholder="例如: 102345678"
                class="w-full px-3 py-2 rounded-lg bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 font-mono text-xs sm:text-sm focus:outline-hidden"
              />
            </div>
            <div>
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="block text-xs font-medium text-zinc-600 dark:text-zinc-400 mb-1">{t('adapters.qq_secret')}</label>
              <div class="relative">
                <input
                  type={qqSecretVisible ? 'text' : 'password'}
                  bind:value={qqSecret}
                  oninput={syncVisualToRaw}
                  placeholder="AppSecret 密钥"
                  class="w-full px-3 py-2 pr-9 rounded-lg bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 font-mono text-xs sm:text-sm focus:outline-hidden"
                />
                <button
                  type="button"
                  onclick={() => (qqSecretVisible = !qqSecretVisible)}
                  class="absolute right-2.5 top-1/2 -translate-y-1/2 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 cursor-pointer"
                >
                  {#if qqSecretVisible}
                    <EyeOff class="w-4 h-4" />
                  {:else}
                    <Eye class="w-4 h-4" />
                  {/if}
                </button>
              </div>
            </div>
          </div>

          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1">
            <div>
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="block text-xs font-medium text-zinc-600 dark:text-zinc-400 mb-1">{t('adapters.qq_md_template')}</label>
              <input
                type="text"
                bind:value={qqMarkdownTemplateId}
                oninput={syncVisualToRaw}
                placeholder="可选自定义模板 ID"
                class="w-full px-3 py-2 rounded-lg bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 font-mono text-xs sm:text-sm focus:outline-hidden"
              />
            </div>
            <div>
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="block text-xs font-medium text-zinc-600 dark:text-zinc-400 mb-1">{t('adapters.qq_md_param')}</label>
              <input
                type="text"
                bind:value={qqMarkdownParamsKey}
                oninput={syncVisualToRaw}
                placeholder="text"
                class="w-full px-3 py-2 rounded-lg bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 font-mono text-xs sm:text-sm focus:outline-hidden"
              />
            </div>
          </div>

          <!-- Feature Toggles -->
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-2.5 pt-1">
            <label class="flex items-center gap-2.5 p-2.5 rounded-lg border border-zinc-200 dark:border-zinc-800 bg-zinc-50/50 dark:bg-zinc-950/40 cursor-pointer">
              <input
                type="checkbox"
                bind:checked={qqEnableGroupC2C}
                onchange={syncVisualToRaw}
                class="rounded border-zinc-300 text-indigo-600 focus:ring-indigo-500 w-4 h-4"
              />
              <span class="text-xs text-zinc-700 dark:text-zinc-300">{t('adapters.qq_group_c2c')}</span>
            </label>
            <label class="flex items-center gap-2.5 p-2.5 rounded-lg border border-zinc-200 dark:border-zinc-800 bg-zinc-50/50 dark:bg-zinc-950/40 cursor-pointer">
              <input
                type="checkbox"
                bind:checked={qqEnableGuildDm}
                onchange={syncVisualToRaw}
                class="rounded border-zinc-300 text-indigo-600 focus:ring-indigo-500 w-4 h-4"
              />
              <span class="text-xs text-zinc-700 dark:text-zinc-300">{t('adapters.qq_guild_dm')}</span>
            </label>
            <label class="flex items-center gap-2.5 p-2.5 rounded-lg border border-zinc-200 dark:border-zinc-800 bg-zinc-50/50 dark:bg-zinc-950/40 cursor-pointer">
              <input
                type="checkbox"
                bind:checked={qqUseMarkdown}
                onchange={syncVisualToRaw}
                class="rounded border-zinc-300 text-indigo-600 focus:ring-indigo-500 w-4 h-4"
              />
              <span class="text-xs text-zinc-700 dark:text-zinc-300">{t('adapters.qq_use_markdown')}</span>
            </label>
            <label class="flex items-center gap-2.5 p-2.5 rounded-lg border border-zinc-200 dark:border-zinc-800 bg-zinc-50/50 dark:bg-zinc-950/40 cursor-pointer">
              <input
                type="checkbox"
                bind:checked={qqIsSandbox}
                onchange={syncVisualToRaw}
                class="rounded border-zinc-300 text-indigo-600 focus:ring-indigo-500 w-4 h-4"
              />
              <span class="text-xs text-zinc-700 dark:text-zinc-300">{t('adapters.qq_sandbox')}</span>
            </label>
          </div>
        </div>
      {:else}
        <div>
          <!-- svelte-ignore a11y_label_has_associated_control -->
          <label class="block text-xs sm:text-sm font-medium text-zinc-600 dark:text-zinc-400 mb-1.5">Configuration (JSON)</label>
          <textarea
            bind:value={configEditRaw}
            oninput={() => { if (selectedPluginId === 'org.kanon.adapter.qqofficial') syncRawToVisual(); }}
            rows={10}
            class="w-full p-3 bg-zinc-950 font-mono text-xs sm:text-sm text-zinc-200 border border-zinc-800 rounded-lg focus:outline-hidden"
          ></textarea>
        </div>
      {/if}

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

<!-- QQ Official QR Code Login Modal -->
{#if qrModalOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="fixed inset-0 bg-black/50 backdrop-blur-xs z-50 flex items-center justify-center p-4"
    onclick={closeQrLoginModal}
    role="button"
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="w-full max-w-md bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-2xl shadow-2xl p-6 space-y-5 text-center"
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      tabindex="-1"
    >
      <div class="flex items-center justify-between border-b border-zinc-200 dark:border-zinc-800 pb-3 text-left">
        <div>
          <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100 flex items-center gap-2">
            <QrCode class="w-5 h-5 text-emerald-600 dark:text-emerald-400" />
            <span>{t('adapters.qq_qr_title')}</span>
          </h3>
          <p class="text-xs text-zinc-500 mt-0.5">{t('adapters.qq_qr_desc')}</p>
        </div>
        <button
          onclick={closeQrLoginModal}
          class="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 cursor-pointer p-1"
        >
          <X class="w-5 h-5" />
        </button>
      </div>

      <!-- QR Display & Live Polling Status -->
      <div class="py-2 flex flex-col items-center justify-center min-h-[260px]">
        {#if qrStatus === 'generating'}
          <div class="w-56 h-56 rounded-xl bg-zinc-100 dark:bg-zinc-800/60 flex flex-col items-center justify-center gap-3">
            <RefreshCw class="w-8 h-8 text-zinc-400 animate-spin" />
            <span class="text-xs text-zinc-500">{t('adapters.qq_qr_generating')}</span>
          </div>
        {:else if qrStatus === 'waiting' && qrCodeUrl}
          <div class="p-3 bg-white rounded-xl shadow-xs border border-zinc-200 dark:border-zinc-700">
            <img
              src={`https://api.qrserver.com/v1/create-qr-code/?size=220x220&data=${encodeURIComponent(qrCodeUrl)}`}
              alt="QQ Official Login QR Code"
              class="w-52 h-52 object-contain"
            />
          </div>
          <div class="mt-3.5 flex items-center gap-2 text-xs font-medium text-emerald-600 dark:text-emerald-400">
            <span class="relative flex h-2.5 w-2.5">
              <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
              <span class="relative inline-flex rounded-full h-2.5 w-2.5 bg-emerald-500"></span>
            </span>
            <span>{t('adapters.qq_qr_waiting')}</span>
          </div>
        {:else if qrStatus === 'success'}
          <div class="w-56 h-56 rounded-xl bg-emerald-500/10 border border-emerald-500/20 flex flex-col items-center justify-center gap-3 p-4">
            <CheckCircle2 class="w-12 h-12 text-emerald-500" />
            <div class="space-y-1">
              <span class="text-sm font-semibold text-emerald-700 dark:text-emerald-300">授权绑定成功！</span>
              <p class="text-xs font-mono text-emerald-600 dark:text-emerald-400">AppID: {qrBoundAppId}</p>
            </div>
            <p class="text-[11px] text-zinc-500">{t('adapters.qq_qr_success')}</p>
          </div>
        {:else if qrStatus === 'expired'}
          <div class="w-56 h-56 rounded-xl bg-amber-500/10 border border-amber-500/20 flex flex-col items-center justify-center gap-3 p-4">
            <AlertTriangle class="w-10 h-10 text-amber-500" />
            <span class="text-xs text-amber-700 dark:text-amber-300">{t('adapters.qq_qr_expired')}</span>
            <button
              onclick={openQrLoginModal}
              class="px-3.5 py-1.5 bg-amber-600 hover:bg-amber-500 text-white rounded-lg text-xs font-medium transition cursor-pointer"
            >
              {t('adapters.qq_qr_retry')}
            </button>
          </div>
        {:else if qrStatus === 'error'}
          <div class="w-56 h-56 rounded-xl bg-rose-500/10 border border-rose-500/20 flex flex-col items-center justify-center gap-3 p-4">
            <XCircle class="w-10 h-10 text-rose-500" />
            <span class="text-xs text-rose-700 dark:text-rose-300">{qrStatusMsg || 'Error'}</span>
            <button
              onclick={openQrLoginModal}
              class="px-3.5 py-1.5 bg-rose-600 hover:bg-rose-500 text-white rounded-lg text-xs font-medium transition cursor-pointer"
            >
              {t('common.retry')}
            </button>
          </div>
        {/if}
      </div>

      <!-- Action buttons -->
      {#if qrCodeUrl && qrStatus === 'waiting'}
        <div class="flex items-center justify-center gap-2 pt-1">
          <a
            href={qrCodeUrl}
            target="_blank"
            rel="noopener noreferrer"
            class="px-3 py-1.5 text-xs font-medium rounded-lg bg-indigo-50 dark:bg-indigo-950/40 text-indigo-600 dark:text-indigo-400 hover:bg-indigo-100 dark:hover:bg-indigo-900/60 transition flex items-center gap-1.5"
          >
            <ExternalLink class="w-3.5 h-3.5" />
            <span>{t('adapters.qq_qr_open_link')}</span>
          </a>
          <button
            onclick={copyQrUrl}
            class="px-3 py-1.5 text-xs font-medium rounded-lg border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition flex items-center gap-1.5 cursor-pointer"
          >
            {#if qrCopied}
              <Check class="w-3.5 h-3.5 text-emerald-500" />
              <span>{t('adapters.qq_qr_copied')}</span>
            {:else}
              <Copy class="w-3.5 h-3.5" />
              <span>{t('adapters.qq_qr_copy_link')}</span>
            {/if}
          </button>
        </div>
      {/if}

      <div class="border-t border-zinc-200 dark:border-zinc-800 pt-3 flex justify-end">
        <button
          onclick={closeQrLoginModal}
          class="px-4 py-2 text-xs sm:text-sm bg-zinc-900 hover:bg-zinc-800 dark:bg-zinc-100 dark:hover:bg-zinc-200 text-white dark:text-zinc-900 rounded-lg font-medium transition cursor-pointer"
        >
          {t('common.close')}
        </button>
      </div>
    </div>
  </div>
{/if}


