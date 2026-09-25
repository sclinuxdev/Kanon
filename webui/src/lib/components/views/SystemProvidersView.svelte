<script lang="ts">
import {
  CheckCircle2,
  Copy,
  Cpu,
  ExternalLink,
  Flame,
  Globe,
  HardDrive,
  KeyRound,
  Layers,
  Radio,
  RefreshCw,
  Send,
  Server,
  ShieldAlert,
  ShieldCheck,
  Terminal,
  Zap,
} from 'lucide-svelte';
import { t } from '../../stores/i18n.svelte';
import { providersStore } from '../../stores/providers.svelte';

let customPrompt = $state('ping');
let selectedPreset = $state<string | null>(null);
let copiedSnippet = $state(false);

async function handleTestActive() {
  await providersStore.runTest({ prompt: customPrompt });
}

function copyEnvSnippet(preset: {
  protocol: string;
  base_url: string;
  default_model: string;
}) {
  const snippet = `export KANON_LLM_PROTOCOL="${preset.protocol}"\nexport KANON_LLM_BASE_URL="${preset.base_url}"\nexport KANON_LLM_MODEL="${preset.default_model}"\nexport KANON_LLM_API_KEY="your-api-key"`;
  navigator.clipboard.writeText(snippet);
  copiedSnippet = true;
  setTimeout(() => {
    copiedSnippet = false;
  }, 2000);
}
</script>

<div class="p-6 space-y-6 max-w-7xl mx-auto">
  <!-- Active Provider & Test Hero Banner -->
  <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
    <!-- Active Provider Card -->
    <div class="lg:col-span-2 bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-5 shadow-xs flex flex-col justify-between">
      <div>
        <div class="flex items-center justify-between pb-3 border-b border-zinc-100 dark:border-zinc-800">
          <div class="flex items-center gap-2.5">
            <div class="p-2 rounded-lg bg-zinc-100 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200">
              <Cpu class="w-4 h-4" />
            </div>
            <div>
              <h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">{t('providers.active_provider')}</h3>
              <p class="text-xs text-zinc-500 font-mono">
                {providersStore.catalog?.active.protocol ?? 'openai'} wire format
              </p>
            </div>
          </div>
          <span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-mono font-medium
            {providersStore.catalog?.active.configured
              ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20'
              : 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20'}">
            <span class="w-1.5 h-1.5 rounded-full {providersStore.catalog?.active.configured ? 'bg-emerald-500' : 'bg-amber-500'}"></span>
            {providersStore.catalog?.active.configured ? t('overview.llm_ready') : t('overview.llm_disabled')}
          </span>
        </div>

        <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 mt-4 text-xs font-mono">
          <div class="p-3 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800/80">
            <span class="text-zinc-400 block mb-1">{t('providers.model_name')}</span>
            <span class="text-zinc-900 dark:text-zinc-100 font-medium">
              {providersStore.catalog?.active.model ?? 'gpt-4o-mini'}
            </span>
          </div>

          <div class="p-3 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800/80">
            <span class="text-zinc-400 block mb-1">{t('providers.base_url')}</span>
            <span class="text-zinc-900 dark:text-zinc-100 truncate block">
              {providersStore.catalog?.active.base_url || 'Unset (Defaults to provider official)'}
            </span>
          </div>

          <div class="p-3 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800/80">
            <span class="text-zinc-400 block mb-1">{t('providers.api_key')}</span>
            <span class="flex items-center gap-1.5 {providersStore.catalog?.active.api_key_configured ? 'text-emerald-600 dark:text-emerald-400' : 'text-zinc-400'}">
              <KeyRound class="w-3.5 h-3.5" />
              {providersStore.catalog?.active.api_key_configured ? t('providers.api_key_set') : t('providers.api_key_unset')}
            </span>
          </div>

          <div class="p-3 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800/80">
            <span class="text-zinc-400 block mb-1">Sampling (Temp / MaxTok)</span>
            <span class="text-zinc-900 dark:text-zinc-100">
              {providersStore.catalog?.active.temperature ?? 'Default'} / {providersStore.catalog?.active.max_tokens ?? 'Unlimited'}
            </span>
          </div>
        </div>
      </div>

      <!-- Test Prompt & Trigger -->
      <div class="mt-5 pt-4 border-t border-zinc-100 dark:border-zinc-800 flex items-center gap-3">
        <div class="flex-1 relative">
          <input
            type="text"
            bind:value={customPrompt}
            placeholder={t('providers.test_prompt')}
            class="w-full px-3 py-2 text-xs font-mono bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 rounded-lg focus:outline-hidden focus:ring-1 focus:ring-zinc-400 dark:focus:ring-zinc-600 text-zinc-900 dark:text-zinc-100"
          />
        </div>
        <button
          onclick={handleTestActive}
          disabled={providersStore.isTesting}
          class="px-4 py-2 bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-950 rounded-lg text-xs font-medium hover:bg-zinc-800 dark:hover:bg-zinc-200 transition flex items-center gap-2 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed shrink-0"
        >
          {#if providersStore.isTesting}
            <RefreshCw class="w-3.5 h-3.5 animate-spin" />
            <span>{t('providers.testing')}</span>
          {:else}
            <Zap class="w-3.5 h-3.5" />
            <span>{t('providers.test_connectivity')}</span>
          {/if}
        </button>
      </div>
    </div>

    <!-- Latency & Connectivity Test Result Card -->
    <div class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-5 shadow-xs flex flex-col justify-between">
      <div>
        <div class="flex items-center justify-between pb-3 border-b border-zinc-100 dark:border-zinc-800">
          <h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">{t('providers.test_result')}</h3>
          {#if providersStore.testResult}
            <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs font-mono font-medium
              {providersStore.testResult.status === 'ok'
                ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20'
                : 'bg-rose-500/10 text-rose-600 dark:text-rose-400 border border-rose-500/20'}">
              {providersStore.testResult.status === 'ok' ? 'SUCCESS' : 'FAILED'}
            </span>
          {/if}
        </div>

        {#if providersStore.testResult}
          <div class="mt-4 space-y-3 font-mono text-xs">
            <div class="flex items-center justify-between p-3 bg-zinc-50 dark:bg-zinc-950/60 rounded-lg border border-zinc-100 dark:border-zinc-800">
              <span class="text-zinc-400">{t('providers.latency_ms')}</span>
              <span class="text-base font-bold {providersStore.testResult.status === 'ok' ? 'text-emerald-500' : 'text-rose-500'}">
                {providersStore.testResult.latency_ms} ms
              </span>
            </div>

            {#if providersStore.testResult.reply}
              <div class="p-3 bg-zinc-50 dark:bg-zinc-950/60 rounded-lg border border-zinc-100 dark:border-zinc-800">
                <span class="text-zinc-400 block mb-1">{t('providers.response_preview')}</span>
                <p class="text-zinc-800 dark:text-zinc-200 line-clamp-3">
                  {providersStore.testResult.reply}
                </p>
              </div>
            {/if}

            {#if providersStore.testResult.error}
              <div class="p-3 bg-rose-500/10 rounded-lg border border-rose-500/20 text-rose-600 dark:text-rose-400">
                <span class="font-semibold block mb-0.5">Error Detail:</span>
                <p class="break-words">{providersStore.testResult.error}</p>
              </div>
            {/if}
          </div>
        {:else}
          <div class="mt-8 text-center text-xs text-zinc-400 font-mono">
            <Zap class="w-8 h-8 mx-auto text-zinc-300 dark:text-zinc-700 mb-2 stroke-[1.5]" />
            <p>Click "Test Connectivity" to measure round-trip latency and verify API token validity.</p>
          </div>
        {/if}
      </div>

      <div class="mt-4 text-[11px] text-zinc-400 font-mono text-right">
        Times out automatically after 15s
      </div>
    </div>
  </div>

  <!-- Supported Presets Section -->
  <div>
    <div class="flex items-center justify-between mb-4">
      <div>
        <h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">{t('providers.presets_title')}</h3>
        <p class="text-xs text-zinc-500 font-mono">Ready-to-use LLM gateway service configurations</p>
      </div>
      {#if copiedSnippet}
        <span class="text-xs text-emerald-500 font-mono flex items-center gap-1">
          <CheckCircle2 class="w-3.5 h-3.5" />
          Environment variables copied!
        </span>
      {/if}
    </div>

    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
      {#each providersStore.catalog?.presets ?? [] as preset}
        <div class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-4 shadow-xs hover:border-zinc-300 dark:hover:border-zinc-700 transition flex flex-col justify-between">
          <div>
            <div class="flex items-center justify-between">
              <h4 class="text-xs font-semibold text-zinc-900 dark:text-zinc-100">{preset.name}</h4>
              <span class="text-[10px] font-mono px-1.5 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400">
                {preset.protocol}
              </span>
            </div>
            <div class="mt-3 space-y-1.5 font-mono text-[11px]">
              <div>
                <span class="text-zinc-400 block text-[10px]">Endpoint:</span>
                <span class="text-zinc-700 dark:text-zinc-300 truncate block" title={preset.base_url}>
                  {preset.base_url}
                </span>
              </div>
              <div>
                <span class="text-zinc-400 block text-[10px]">Model:</span>
                <span class="text-zinc-700 dark:text-zinc-300 font-medium">
                  {preset.default_model}
                </span>
              </div>
            </div>
          </div>

          <div class="mt-4 pt-3 border-t border-zinc-100 dark:border-zinc-800/80 flex items-center gap-2">
            <button
              onclick={() => copyEnvSnippet(preset)}
              class="flex-1 py-1.5 px-2.5 bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-800 dark:text-zinc-200 rounded-md text-[11px] font-medium font-mono flex items-center justify-center gap-1.5 transition cursor-pointer"
              title="Copy environment variables for this provider"
            >
              <Copy class="w-3 h-3" />
              <span>Copy .env</span>
            </button>
            <button
              onclick={() => providersStore.runTest({ protocol: preset.protocol, base_url: preset.base_url, model: preset.default_model, prompt: 'ping' })}
              class="p-1.5 bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-800 dark:text-zinc-200 rounded-md transition cursor-pointer"
              title="Test endpoint directly"
            >
              <Zap class="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      {/each}
    </div>
  </div>

  <!-- Microkernel System & Node Configuration Section -->
  <div class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-5 shadow-xs">
    <div class="flex items-center justify-between pb-3 border-b border-zinc-100 dark:border-zinc-800">
      <div class="flex items-center gap-2">
        <Server class="w-4 h-4 text-zinc-500" />
        <h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">{t('providers.system_config_title')}</h3>
      </div>
      <span class="text-xs font-mono text-zinc-400">
        Rust {providersStore.systemConfig?.environment.rust_edition ?? '2024'} Edition
      </span>
    </div>

    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mt-4 font-mono text-xs">
      <!-- IPC Socket -->
      <div class="p-3 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800">
        <span class="text-zinc-400 block mb-1">{t('providers.ipc_socket')}</span>
        <span class="text-zinc-900 dark:text-zinc-100 truncate block" title={providersStore.systemConfig?.ipc_socket_path}>
          {providersStore.systemConfig?.ipc_socket_path ?? './run/core.sock'}
        </span>
      </div>

      <!-- Run Directory -->
      <div class="p-3 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800">
        <span class="text-zinc-400 block mb-1">{t('providers.run_dir')}</span>
        <span class="text-zinc-900 dark:text-zinc-100 truncate block">
          {providersStore.systemConfig?.run_dir ?? './run'}
        </span>
      </div>

      <!-- Data Directory -->
      <div class="p-3 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800">
        <span class="text-zinc-400 block mb-1">{t('providers.data_dir')}</span>
        <span class="text-zinc-900 dark:text-zinc-100 truncate block">
          {providersStore.systemConfig?.data_dir ?? './data'}
        </span>
      </div>

      <!-- Memory Sliding Window -->
      <div class="p-3 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800">
        <span class="text-zinc-400 block mb-1">{t('providers.memory_window')}</span>
        <span class="text-zinc-900 dark:text-zinc-100">
          {providersStore.systemConfig?.memory_window ?? 40} turns
        </span>
      </div>

      <!-- Webhook Adapter Info -->
      <div class="p-3 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800">
        <span class="text-zinc-400 block mb-1">{t('providers.webhook_adapter')}</span>
        <span class="text-zinc-900 dark:text-zinc-100 flex items-center justify-between">
          <span>{providersStore.systemConfig?.webhook.platform ?? 'webhook'}</span>
          <span class="text-[11px] {providersStore.systemConfig?.webhook.callback_configured ? 'text-emerald-500' : 'text-zinc-400'}">
            {providersStore.systemConfig?.webhook.callback_configured ? 'Outbound Active' : 'Inbound Only'}
          </span>
        </span>
      </div>

      <!-- OS and Arch -->
      <div class="p-3 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800">
        <span class="text-zinc-400 block mb-1">{t('providers.os_arch')}</span>
        <span class="text-zinc-900 dark:text-zinc-100">
          {providersStore.systemConfig?.environment.os ?? 'linux'} ({providersStore.systemConfig?.environment.arch ?? 'x86_64'})
        </span>
      </div>
    </div>
  </div>
</div>
