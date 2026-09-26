<script lang="ts">
import { Radio, RefreshCw } from 'lucide-svelte';
import { t } from '../../stores/i18n.svelte';
import { logStore } from '../../stores/logs.svelte';
import { nodeStore } from '../../stores/node.svelte';
import { pipelineStore } from '../../stores/pipeline.svelte';

let { title = 'Overview', subtitle = '' } = $props<{
  title?: string;
  subtitle?: string;
}>();

let isRefreshing = $state(false);

async function handleRefresh() {
  isRefreshing = true;
  await nodeStore.refresh();
  setTimeout(() => {
    isRefreshing = false;
  }, 400);
}
</script>

<header class="h-16 border-b border-zinc-200 dark:border-zinc-800 bg-white/60 dark:bg-zinc-950/60 backdrop-blur-md px-6 flex items-center justify-between shrink-0">
  <div>
    <h2 class="text-base sm:text-lg font-semibold text-zinc-900 dark:text-zinc-100 tracking-tight">{title}</h2>
    {#if subtitle}
      <p class="text-xs text-zinc-500 dark:text-zinc-400 mt-0.5">{subtitle}</p>
    {/if}
  </div>

  <div class="flex items-center gap-3">
    <!-- WebSocket connection status indicators -->
    <div class="hidden sm:flex items-center gap-2.5 text-xs font-mono text-zinc-500 dark:text-zinc-400 bg-zinc-100 dark:bg-zinc-900 px-3 py-1.5 rounded-lg border border-zinc-200 dark:border-zinc-800">
      <span class="flex items-center gap-1.5">
        <Radio class="w-3.5 h-3.5 {pipelineStore.status === 'connected' ? 'text-emerald-500' : 'text-amber-500'}" />
        <span>{t('common.events')}</span>
      </span>
      <span class="text-zinc-300 dark:text-zinc-700">|</span>
      <span class="flex items-center gap-1.5">
        <Radio class="w-3.5 h-3.5 {logStore.status === 'connected' ? 'text-emerald-500' : 'text-amber-500'}" />
        <span>{t('common.logs')}</span>
      </span>
    </div>

    <!-- Manual refresh button -->
    <button
      onclick={handleRefresh}
      class="p-2 text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-900 transition border border-transparent hover:border-zinc-200 dark:hover:border-zinc-800 cursor-pointer"
      title={t('common.refresh')}
    >
      <RefreshCw class="w-4 h-4 {isRefreshing ? 'animate-spin text-zinc-900 dark:text-zinc-100' : ''}" />
    </button>
  </div>
</header>
