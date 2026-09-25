<script lang="ts">
import {
  Activity,
  AlertCircle,
  ArrowDown,
  Bot,
  CheckCircle2,
  Clock,
  Filter,
  Layers,
  Search,
  Terminal,
  Trash2,
  Zap,
} from 'lucide-svelte';
import { t } from '../../stores/i18n.svelte';
import { logStore } from '../../stores/logs.svelte';
import { pipelineStore } from '../../stores/pipeline.svelte';
import type { LogLevel } from '../../types';

let logContainer = $state<HTMLDivElement | null>(null);

// Auto scroll to bottom when new logs arrive if autoScroll is enabled
$effect(() => {
  if (logStore.autoScroll && logContainer) {
    logContainer.scrollTop = logContainer.scrollHeight;
  }
});

const levels: (LogLevel | 'ALL')[] = ['ALL', 'INFO', 'WARN', 'ERROR', 'DEBUG'];

function formatTime(timestampMs: number): string {
  const d = new Date(timestampMs);
  return (
    d.toTimeString().split(' ')[0] +
    '.' +
    String(d.getMilliseconds()).padStart(3, '0')
  );
}

function getLevelBadgeClass(level: LogLevel): string {
  switch (level) {
    case 'ERROR':
      return 'bg-rose-500/10 text-rose-600 dark:text-rose-400 border border-rose-500/20';
    case 'WARN':
      return 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20';
    case 'INFO':
      return 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20';
    case 'DEBUG':
      return 'bg-zinc-500/10 text-zinc-600 dark:text-zinc-400 border border-zinc-500/20';
  }
}

function getStageColor(stage: string): string {
  if (stage === 'ingested')
    return 'text-blue-500 bg-blue-500/10 border-blue-500/20';
  if (stage.includes('blocked') || stage.includes('failed'))
    return 'text-rose-500 bg-rose-500/10 border-rose-500/20';
  if (stage.includes('command'))
    return 'text-violet-500 bg-violet-500/10 border-violet-500/20';
  if (stage.includes('llm') || stage.includes('tool'))
    return 'text-indigo-500 bg-indigo-500/10 border-indigo-500/20';
  if (stage.includes('outbound') || stage.includes('passed'))
    return 'text-emerald-500 bg-emerald-500/10 border-emerald-500/20';
  if (stage.includes('breaker'))
    return 'text-amber-500 bg-amber-500/10 border-amber-500/20';
  return 'text-zinc-400 bg-zinc-500/10 border-zinc-500/20';
}
</script>

<div class="h-full flex flex-col md:flex-row overflow-hidden divide-y md:divide-y-0 md:divide-x divide-zinc-200 dark:divide-zinc-800">
  <!-- Left Column: Pipeline Stages & Events Stream -->
  <div class="w-full md:w-5/12 flex flex-col h-1/2 md:h-full bg-zinc-50/40 dark:bg-zinc-950/40">
    <!-- Header -->
    <div class="p-3 border-b border-zinc-200 dark:border-zinc-800 flex items-center justify-between shrink-0 bg-white dark:bg-zinc-900">
      <div class="flex items-center gap-2">
        <Layers class="w-4 h-4 text-zinc-500" />
        <span class="text-xs font-semibold text-zinc-900 dark:text-zinc-100 tracking-tight">{t('pipeline.live_events')}</span>
      </div>
      <div class="flex items-center gap-2">
        <span class="text-[10px] font-mono text-zinc-400">({pipelineStore.records.length} events)</span>
        <button
          onclick={() => pipelineStore.clear()}
          class="p-1 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 transition cursor-pointer"
          title={t('common.clear')}
        >
          <Trash2 class="w-3.5 h-3.5" />
        </button>
      </div>
    </div>

    <!-- Quick stage stats chips -->
    <div class="p-2 border-b border-zinc-200/80 dark:border-zinc-800/80 flex items-center gap-1.5 overflow-x-auto text-[10px] font-mono shrink-0">
      <button
        onclick={() => (pipelineStore.selectedStage = 'ALL')}
        class="px-2 py-0.5 rounded cursor-pointer transition {pipelineStore.selectedStage === 'ALL' ? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        All ({pipelineStore.records.length})
      </button>
      <button
        onclick={() => (pipelineStore.selectedStage = 'ingested')}
        class="px-2 py-0.5 rounded cursor-pointer transition {pipelineStore.selectedStage === 'ingested' ? 'bg-blue-600 text-white' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        Ingest ({pipelineStore.stats.ingested})
      </button>
      <button
        onclick={() => (pipelineStore.selectedStage = 'pre_filter')}
        class="px-2 py-0.5 rounded cursor-pointer transition {pipelineStore.selectedStage === 'pre_filter' ? 'bg-zinc-600 text-white' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        PreFilter ({pipelineStore.stats.pre_filter})
      </button>
      <button
        onclick={() => (pipelineStore.selectedStage = 'command')}
        class="px-2 py-0.5 rounded cursor-pointer transition {pipelineStore.selectedStage === 'command' ? 'bg-violet-600 text-white' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        Cmd ({pipelineStore.stats.command})
      </button>
      <button
        onclick={() => (pipelineStore.selectedStage = 'llm')}
        class="px-2 py-0.5 rounded cursor-pointer transition {pipelineStore.selectedStage === 'llm' ? 'bg-indigo-600 text-white' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        LLM ({pipelineStore.stats.llm})
      </button>
      <button
        onclick={() => (pipelineStore.selectedStage = 'outbound')}
        class="px-2 py-0.5 rounded cursor-pointer transition {pipelineStore.selectedStage === 'outbound' ? 'bg-emerald-600 text-white' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        Outbound ({pipelineStore.stats.outbound})
      </button>
    </div>

    <!-- Event Timeline List -->
    <div class="flex-1 overflow-y-auto p-3 space-y-2 font-mono text-xs">
      {#if pipelineStore.filteredRecords.length === 0}
        <div class="p-8 text-center text-zinc-400 text-xs font-sans">
          {t('pipeline.empty_events')}
        </div>
      {:else}
        {#each pipelineStore.filteredRecords.slice().reverse() as record (record.seq)}
          <div class="p-2.5 rounded-lg border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs space-y-1.5">
            <div class="flex items-center justify-between text-[11px]">
              <span class="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-semibold border {getStageColor(record.event.stage)}">
                {record.event.stage}
              </span>
              <span class="text-zinc-400 text-[10px]">{formatTime(record.timestamp_ms)}</span>
            </div>

            <!-- Stage Context Details -->
            <div class="text-[11px] text-zinc-700 dark:text-zinc-300 space-y-0.5">
              {#if record.event.event_id}
                <div class="flex items-center gap-1.5 text-zinc-500 text-[10px]">
                  <span>ID:</span>
                  <span class="text-zinc-700 dark:text-zinc-300 truncate">{record.event.event_id}</span>
                </div>
              {/if}
              {#if record.event.platform}
                <div class="flex items-center gap-1.5 text-zinc-500 text-[10px]">
                  <span>Platform:</span>
                  <span class="font-semibold text-zinc-800 dark:text-zinc-200">{record.event.platform}</span>
                  {#if record.event.channel_id}
                    <span>• {record.event.channel_id}</span>
                  {/if}
                </div>
              {/if}
              {#if record.event.command}
                <div class="text-violet-600 dark:text-violet-400 font-semibold">
                  /{record.event.command} (plugin: {record.event.plugin_id ?? 'unknown'})
                </div>
              {/if}
              {#if record.event.session_id}
                <div class="text-indigo-600 dark:text-indigo-400">
                  Session: {record.event.session_id}
                </div>
              {/if}
              {#if record.event.reason}
                <div class="text-rose-500 text-[10px]">
                  Reason: {record.event.reason}
                </div>
              {/if}
            </div>
          </div>
        {/each}
      {/if}
    </div>
  </div>

  <!-- Right Column: Rolling Terminal Console -->
  <div class="w-full md:w-7/12 flex flex-col h-1/2 md:h-full bg-zinc-950 text-zinc-200 font-mono text-xs">
    <!-- Toolbar -->
    <div class="p-2.5 bg-zinc-900 border-b border-zinc-800 flex flex-wrap items-center justify-between gap-2 shrink-0">
      <div class="flex items-center gap-1 text-[11px]">
        {#each levels as lvl}
          <button
            onclick={() => logStore.setFilter(lvl)}
            class="px-2 py-0.5 rounded cursor-pointer transition text-[10px] font-semibold {logStore.filterLevel === lvl ? 'bg-zinc-800 text-white border border-zinc-700' : 'text-zinc-400 hover:text-zinc-200'}"
          >
            {lvl}
          </button>
        {/each}
      </div>

      <div class="flex items-center gap-2">
        <!-- Search filter input -->
        <div class="relative">
          <input
            type="text"
            bind:value={logStore.searchQuery}
            placeholder={t('common.search')}
            class="w-36 sm:w-48 px-2 py-1 pl-6 text-[11px] bg-zinc-950 border border-zinc-800 rounded text-zinc-200 placeholder-zinc-500 focus:outline-hidden focus:border-zinc-700"
          />
          <Search class="w-3 h-3 text-zinc-500 absolute left-2 top-2" />
        </div>

        <!-- Auto-scroll lock toggle -->
        <button
          onclick={() => (logStore.autoScroll = !logStore.autoScroll)}
          class="p-1 rounded cursor-pointer transition {logStore.autoScroll ? 'text-emerald-400' : 'text-zinc-500'}"
          title={logStore.autoScroll ? 'Auto-scroll enabled' : 'Auto-scroll paused'}
        >
          <ArrowDown class="w-3.5 h-3.5" />
        </button>

        <!-- Clear logs -->
        <button
          onclick={() => logStore.clear()}
          class="p-1 text-zinc-400 hover:text-rose-400 transition cursor-pointer"
          title={t('common.clear')}
        >
          <Trash2 class="w-3.5 h-3.5" />
        </button>
      </div>
    </div>

    <!-- Terminal logs viewport -->
    <div
      bind:this={logContainer}
      class="flex-1 p-3 overflow-y-auto space-y-1 font-mono text-[11px] leading-relaxed select-text"
    >
      {#if logStore.filteredRecords.length === 0}
        <div class="text-zinc-600 p-8 text-center font-sans">
          {t('pipeline.empty_logs')}
        </div>
      {:else}
        {#each logStore.filteredRecords as record (record.seq)}
          <div class="flex items-start gap-2 hover:bg-zinc-900/60 p-0.5 rounded">
            <span class="text-zinc-500 shrink-0 text-[10px]">{formatTime(record.timestamp_ms)}</span>
            <span class="px-1 rounded text-[9px] font-bold shrink-0 {getLevelBadgeClass(record.level)}">
              {record.level}
            </span>
            <span class="text-zinc-400 shrink-0 max-w-[140px] truncate text-[10px]" title={record.target}>
              {record.target.split('::').slice(-2).join('::')}
            </span>
            <span class="text-zinc-200 break-all">{record.message}</span>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</div>
