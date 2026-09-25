<script lang="ts">
import {
  Activity,
  Bot,
  Boxes,
  Clock,
  Cpu,
  HardDrive,
  MessageSquare,
  Radio,
  Server,
  Terminal,
  Zap,
} from 'lucide-svelte';
import { api } from '../../api/client';
import { t } from '../../stores/i18n.svelte';
import { nodeStore } from '../../stores/node.svelte';

let { onNavigate = (_tab: string) => {} } = $props<{
  onNavigate?: (tab: string) => void;
}>();

let metricsRaw = $state<string>('');
let loadingMetrics = $state(false);

function formatUptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

function formatBytes(bytes?: number | null): string {
  if (!bytes) return '--';
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) {
    return `${(mb / 1024).toFixed(2)} GB`;
  }
  return `${mb.toFixed(1)} MB`;
}

async function loadMetrics() {
  loadingMetrics = true;
  try {
    metricsRaw = await api.getMetrics();
  } catch {
    metricsRaw = 'Failed to load prometheus metrics.';
  } finally {
    loadingMetrics = false;
  }
}

$effect(() => {
  loadMetrics();
});
</script>

<div class="p-6 space-y-6 max-w-7xl mx-auto">
  <!-- Key Metrics Row (4 top cards) -->
  <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
    <!-- Status & Version Card -->
    <div class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs">
      <div class="flex items-center justify-between text-zinc-500 text-xs font-medium">
        <span>{t('overview.node_status')}</span>
        <Activity class="w-4 h-4 text-emerald-500" />
      </div>
      <div class="mt-2 flex items-baseline gap-2">
        <span class="text-xl font-bold font-mono tracking-tight text-zinc-900 dark:text-zinc-100 uppercase">
          {nodeStore.health?.status ?? 'Connecting'}
        </span>
        <span class="text-xs font-mono text-zinc-400">v{nodeStore.health?.version ?? '0.1.0'}</span>
      </div>
      <p class="mt-1 text-[11px] text-zinc-500">Standalone Microkernel</p>
    </div>

    <!-- Resident Memory (RSS) Card -->
    <div class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs">
      <div class="flex items-center justify-between text-zinc-500 text-xs font-medium">
        <span>{t('overview.resident_memory')}</span>
        <HardDrive class="w-4 h-4 text-zinc-400" />
      </div>
      <div class="mt-2 flex items-baseline gap-2">
        <span class="text-xl font-bold font-mono tracking-tight text-zinc-900 dark:text-zinc-100">
          {formatBytes(nodeStore.health?.memory?.resident_bytes)}
        </span>
        {#if nodeStore.health?.memory?.virtual_bytes}
          <span class="text-xs font-mono text-zinc-400 truncate">
            (Virt: {formatBytes(nodeStore.health.memory.virtual_bytes)})
          </span>
        {/if}
      </div>
      <p class="mt-1 text-[11px] text-zinc-500">Zero external dependencies (&lt; 20MB target)</p>
    </div>

    <!-- Uptime Card -->
    <div class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs">
      <div class="flex items-center justify-between text-zinc-500 text-xs font-medium">
        <span>{t('common.uptime')}</span>
        <Clock class="w-4 h-4 text-zinc-400" />
      </div>
      <div class="mt-2 text-xl font-bold font-mono tracking-tight text-zinc-900 dark:text-zinc-100">
        {nodeStore.health?.uptime_seconds !== undefined ? formatUptime(nodeStore.health.uptime_seconds) : '--'}
      </div>
      <p class="mt-1 text-[11px] text-zinc-500">Continuous scheduler uptime</p>
    </div>

    <!-- AI Model Gateway -->
    <div class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs">
      <div class="flex items-center justify-between text-zinc-500 text-xs font-medium">
        <span>{t('overview.llm_engine')}</span>
        <Bot class="w-4 h-4 {nodeStore.health?.llm_configured ? 'text-emerald-500' : 'text-zinc-400'}" />
      </div>
      <div class="mt-2 flex items-baseline gap-1.5 truncate">
        <span class="text-lg font-bold font-mono tracking-tight text-zinc-900 dark:text-zinc-100 truncate">
          {nodeStore.health?.llm_configured ? t('overview.llm_ready') : t('overview.llm_disabled')}
        </span>
      </div>
      <p class="mt-1 text-[11px] text-zinc-500 truncate">
        {nodeStore.health?.llm_configured ? 'Sandbox chat & Tool calling ready' : 'Chat completions disabled'}
      </p>
    </div>
  </div>

  <!-- Secondary Stats Row (Supervisor hosts, active sessions, realtime) -->
  <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
    <div class="p-3 bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-lg flex items-center justify-between font-mono text-xs shadow-2xs">
      <div class="flex items-center gap-2">
        <Boxes class="w-4 h-4 text-zinc-400" />
        <span class="text-zinc-500">{t('overview.plugin_hosts')}:</span>
      </div>
      <span class="font-bold text-zinc-900 dark:text-zinc-100">
        {nodeStore.health?.plugins?.hosts ?? 0} ({nodeStore.health?.plugins?.loaded ?? 0} {t('overview.plugins_loaded')})
      </span>
    </div>

    <div class="p-3 bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-lg flex items-center justify-between font-mono text-xs shadow-2xs">
      <div class="flex items-center gap-2">
        <MessageSquare class="w-4 h-4 text-zinc-400" />
        <span class="text-zinc-500">{t('overview.sessions_total')}:</span>
      </div>
      <span class="font-bold text-zinc-900 dark:text-zinc-100">
        {nodeStore.health?.sessions?.total ?? 0} ({nodeStore.health?.sessions?.active ?? 0} {t('overview.sessions_active')})
      </span>
    </div>

    <div class="p-3 bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-lg flex items-center justify-between font-mono text-xs shadow-2xs">
      <div class="flex items-center gap-2">
        <Radio class="w-4 h-4 text-zinc-400" />
        <span class="text-zinc-500">{t('overview.ws_connections')}:</span>
      </div>
      <span class="font-bold text-zinc-900 dark:text-zinc-100">
        {nodeStore.health?.realtime?.websocket_connections ?? 0}
      </span>
    </div>
  </div>

  <!-- Quick Action Jump Cards -->
  <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
    <button
      onclick={() => onNavigate('pipeline')}
      class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-zinc-300 dark:hover:border-zinc-700 transition text-left cursor-pointer group shadow-2xs"
    >
      <div class="flex items-center gap-2 text-zinc-900 dark:text-zinc-100 font-semibold text-xs group-hover:text-emerald-600 dark:group-hover:text-emerald-400">
        <Terminal class="w-4 h-4" />
        <span>{t('nav.pipeline')}</span>
      </div>
      <p class="mt-1.5 text-xs text-zinc-500 leading-relaxed">
        {t('subtitle.pipeline')}
      </p>
    </button>

    <button
      onclick={() => onNavigate('plugins')}
      class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-zinc-300 dark:hover:border-zinc-700 transition text-left cursor-pointer group shadow-2xs"
    >
      <div class="flex items-center gap-2 text-zinc-900 dark:text-zinc-100 font-semibold text-xs group-hover:text-emerald-600 dark:group-hover:text-emerald-400">
        <Boxes class="w-4 h-4" />
        <span>{t('nav.plugins')}</span>
      </div>
      <p class="mt-1.5 text-xs text-zinc-500 leading-relaxed">
        {t('subtitle.plugins')}
      </p>
    </button>

    <button
      onclick={() => onNavigate('playground')}
      class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-zinc-300 dark:hover:border-zinc-700 transition text-left cursor-pointer group shadow-2xs"
    >
      <div class="flex items-center gap-2 text-zinc-900 dark:text-zinc-100 font-semibold text-xs group-hover:text-emerald-600 dark:group-hover:text-emerald-400">
        <Bot class="w-4 h-4" />
        <span>{t('nav.playground')}</span>
      </div>
      <p class="mt-1.5 text-xs text-zinc-500 leading-relaxed">
        {t('subtitle.playground')}
      </p>
    </button>

    <button
      onclick={() => onNavigate('providers')}
      class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-zinc-300 dark:hover:border-zinc-700 transition text-left cursor-pointer group shadow-2xs"
    >
      <div class="flex items-center gap-2 text-zinc-900 dark:text-zinc-100 font-semibold text-xs group-hover:text-emerald-600 dark:group-hover:text-emerald-400">
        <Cpu class="w-4 h-4" />
        <span>{t('nav.providers')}</span>
      </div>
      <p class="mt-1.5 text-xs text-zinc-500 leading-relaxed">
        {t('subtitle.providers')}
      </p>
    </button>
  </div>

  <!-- Prometheus Metrics Preview -->
  <div class="rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 overflow-hidden shadow-2xs">
    <div class="px-4 py-3 border-b border-zinc-200 dark:border-zinc-800 flex items-center justify-between">
      <div class="flex items-center gap-2">
        <Server class="w-4 h-4 text-zinc-400" />
        <h3 class="text-xs font-semibold text-zinc-900 dark:text-zinc-100 tracking-tight">Prometheus Exposition (/api/v1/metrics)</h3>
      </div>
      <button
        onclick={loadMetrics}
        class="text-xs text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100 font-mono transition cursor-pointer"
      >
        {t('common.refresh')}
      </button>
    </div>
    <div class="p-4 bg-zinc-950 font-mono text-[11px] text-zinc-300 leading-relaxed max-h-72 overflow-y-auto">
      {#if loadingMetrics}
        <div class="text-zinc-500">{t('common.loading')}</div>
      {:else}
        <pre class="whitespace-pre-wrap">{metricsRaw}</pre>
      {/if}
    </div>
  </div>
</div>
