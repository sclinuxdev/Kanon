<script lang="ts">
import {
  CheckCircle2,
  Copy,
  Database,
  HardDrive,
  Network,
  RefreshCw,
  Server,
} from 'lucide-svelte';
import { i18n, t } from '../../stores/i18n.svelte';
import { nodeStore } from '../../stores/node.svelte';
import { providersStore } from '../../stores/providers.svelte';

let copiedSnippet = $state(false);

function copySocketPath(path: string) {
  navigator.clipboard.writeText(path);
  copiedSnippet = true;
  setTimeout(() => {
    copiedSnippet = false;
  }, 2000);
}
</script>

<div class="p-6 space-y-6 max-w-7xl mx-auto">
  <!-- System Architecture Banner -->
  <div class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-6 shadow-xs">
    <div class="flex items-center justify-between pb-4 border-b border-zinc-100 dark:border-zinc-800">
      <div class="flex items-center gap-3">
        <div class="p-2.5 rounded-xl bg-zinc-100 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200">
          <Server class="w-5 h-5" />
        </div>
        <div>
          <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">{t('providers.system_config_title')}</h3>
          <p class="text-xs text-zinc-500 font-mono mt-0.5">
            {i18n.locale === 'zh' ? '微内核节点底层运行参数与持久化路径配置' : 'Microkernel runtime parameters & persistent paths configuration'}
          </p>
        </div>
      </div>
      <div class="flex items-center gap-2">
        {#if copiedSnippet}
          <span class="text-xs font-mono text-emerald-500 flex items-center gap-1.5">
            <CheckCircle2 class="w-4 h-4" />
            Copied!
          </span>
        {/if}
        <button
          onclick={() => providersStore.refresh()}
          class="px-3 py-1.5 bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-800 dark:text-zinc-200 rounded-md text-xs font-mono flex items-center gap-1.5 transition cursor-pointer"
        >
          <RefreshCw class="w-3.5 h-3.5" />
          <span>{t('common.refresh')}</span>
        </button>
      </div>
    </div>

    <!-- System parameters grid -->
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mt-5 font-mono text-xs">
      <!-- IPC Socket -->
      <div class="p-4 bg-zinc-50 dark:bg-zinc-950/50 rounded-xl border border-zinc-100 dark:border-zinc-800/80">
        <div class="flex items-center justify-between mb-1.5">
          <span class="text-zinc-400 flex items-center gap-1.5">
            <Network class="w-4 h-4" />
            {t('providers.ipc_socket')}
          </span>
          <button
            onclick={() => copySocketPath(providersStore.systemConfig?.ipc_socket_path ?? './run/core.sock')}
            class="text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200 cursor-pointer"
            title="Copy Socket Path"
          >
            <Copy class="w-3.5 h-3.5" />
          </button>
        </div>
        <span class="text-zinc-900 dark:text-zinc-100 font-medium text-sm truncate block" title={providersStore.systemConfig?.ipc_socket_path}>
          {providersStore.systemConfig?.ipc_socket_path ?? './run/core.sock'}
        </span>
      </div>

      <!-- Run Directory -->
      <div class="p-4 bg-zinc-50 dark:bg-zinc-950/50 rounded-xl border border-zinc-100 dark:border-zinc-800/80">
        <span class="text-zinc-400 flex items-center gap-1.5 mb-1.5">
          <HardDrive class="w-4 h-4" />
          {t('providers.run_dir')}
        </span>
        <span class="text-zinc-900 dark:text-zinc-100 font-medium text-sm truncate block">
          {providersStore.systemConfig?.run_dir ?? './run'}
        </span>
      </div>

      <!-- Data Directory -->
      <div class="p-4 bg-zinc-50 dark:bg-zinc-950/50 rounded-xl border border-zinc-100 dark:border-zinc-800/80">
        <span class="text-zinc-400 flex items-center gap-1.5 mb-1.5">
          <Database class="w-4 h-4" />
          {t('providers.data_dir')}
        </span>
        <span class="text-zinc-900 dark:text-zinc-100 font-medium text-sm truncate block">
          {providersStore.systemConfig?.data_dir ?? './data'}
        </span>
      </div>

      <!-- Memory Sliding Window -->
      <div class="p-4 bg-zinc-50 dark:bg-zinc-950/50 rounded-xl border border-zinc-100 dark:border-zinc-800/80">
        <span class="text-zinc-400 block mb-1.5">{t('providers.memory_window')}</span>
        <span class="text-zinc-900 dark:text-zinc-100 font-medium text-sm">
          {providersStore.systemConfig?.memory_window ?? 40} turns (Sliding Window FIFO)
        </span>
      </div>

      <!-- Platform Webhook -->
      <div class="p-4 bg-zinc-50 dark:bg-zinc-950/50 rounded-xl border border-zinc-100 dark:border-zinc-800/80">
        <span class="text-zinc-400 block mb-1.5">{t('providers.webhook_adapter')}</span>
        <span class="text-zinc-900 dark:text-zinc-100 flex items-center justify-between font-medium text-sm">
          <span>{providersStore.systemConfig?.webhook.platform ?? 'webhook'}</span>
          <span class="text-xs {providersStore.systemConfig?.webhook.callback_configured ? 'text-emerald-500' : 'text-zinc-400'}">
            {providersStore.systemConfig?.webhook.callback_configured ? 'Outbound Active' : 'Inbound Only'}
          </span>
        </span>
      </div>

      <!-- OS and Architecture -->
      <div class="p-4 bg-zinc-50 dark:bg-zinc-950/50 rounded-xl border border-zinc-100 dark:border-zinc-800/80">
        <span class="text-zinc-400 block mb-1.5">{t('providers.os_arch')}</span>
        <span class="text-zinc-900 dark:text-zinc-100 font-medium text-sm">
          {providersStore.systemConfig?.environment.os ?? 'linux'} ({providersStore.systemConfig?.environment.arch ?? 'x86_64'})
        </span>
      </div>
    </div>
  </div>
</div>
