<script lang="ts">
import {
  AlertCircle,
  ArrowDownToLine,
  Bot,
  Check,
  CheckCircle2,
  Cpu,
  Download,
  Eye,
  EyeOff,
  Globe,
  Layers,
  ListPlus,
  Plus,
  Radio,
  RefreshCw,
  Server,
  Sparkles,
  Star,
  Trash2,
  Wrench,
  Zap,
} from 'lucide-svelte';
import { t } from '../../stores/i18n.svelte';
import { providersStore } from '../../stores/providers.svelte';
import type { ProviderPreset } from '../../types';

// State for new provider modal
let isAddingProvider = $state(false);
let isQuickConfigOpen = $state(false);
let newProvName = $state('');
let newProvProtocol = $state<'openai' | 'openai_responses' | 'anthropic'>(
  'openai',
);
let newProvBaseUrl = $state('');
let newProvApiKey = $state('');

// State for new model inline input
let newModelId = $state('');
let newModelDisplayName = $state('');

// Form state for current selected provider
let editName = $state('');
let editProtocol = $state<'openai' | 'openai_responses' | 'anthropic'>(
  'openai',
);
let editBaseUrl = $state('');
let editApiKey = $state('');
let showApiKey = $state(false);
let saveNotification = $state(false);

// State for fetched remote model candidates
let selectedCandidates = $state<Set<string>>(new Set());

// Sync local edit inputs when selected provider changes
$effect(() => {
  const p = providersStore.selectedProvider;
  if (p) {
    editName = p.name;
    editProtocol = p.protocol;
    editBaseUrl = p.base_url;
    editApiKey = p.api_key;
    showApiKey = false;
  }
});

function handleSaveProvider() {
  const p = providersStore.selectedProvider;
  if (!p) return;

  providersStore.updateProvider(p.id, {
    name: editName,
    protocol: editProtocol,
    base_url: editBaseUrl,
    api_key: editApiKey,
  });

  saveNotification = true;
  setTimeout(() => {
    saveNotification = false;
  }, 2000);
}

function handleCreateProvider() {
  if (!newProvName.trim()) return;

  providersStore.addProvider({
    name: newProvName,
    protocol: newProvProtocol,
    base_url:
      newProvBaseUrl ||
      (newProvProtocol === 'anthropic'
        ? 'https://api.anthropic.com/v1'
        : 'https://api.openai.com/v1'),
    api_key: newProvApiKey,
  });

  newProvName = '';
  newProvBaseUrl = '';
  newProvApiKey = '';
  isAddingProvider = false;
}

function handleApplyPreset(preset: ProviderPreset) {
  providersStore.applyPreset(preset);
  isQuickConfigOpen = false;
}

function handleAddModel() {
  const p = providersStore.selectedProvider;
  if (!p || !newModelId.trim()) return;

  providersStore.addModel(
    p.id,
    newModelId.trim(),
    newModelDisplayName.trim() || undefined,
  );
  newModelId = '';
  newModelDisplayName = '';
}

function handleDeleteModel(modelId: string) {
  const p = providersStore.selectedProvider;
  if (!p) return;
  providersStore.deleteModel(p.id, modelId);
}

function handleDeleteProvider(id: string) {
  if (confirm('确定要删除此模型提供商吗？其关联的所有模型也将一并移除。')) {
    providersStore.deleteProvider(id);
  }
}

async function handleFetchRemoteModels() {
  const p = providersStore.selectedProvider;
  if (!p) return;
  // Automatically persist form inputs first so any typed API Key or URL is saved
  handleSaveProvider();
  const list = await providersStore.fetchRemoteModels(
    p.id,
    editApiKey.trim(),
    editBaseUrl.trim(),
    editProtocol,
  );
  selectedCandidates = new Set(list.slice(0, 10)); // pre-select up to 10
}

function handleTestModel(provId: string, modelId: string) {
  handleSaveProvider();
  providersStore.testModel(provId, modelId, 'ping', editApiKey.trim());
}

function handleImportSelectedCandidates() {
  const p = providersStore.selectedProvider;
  if (!p) return;
  providersStore.addMultipleModels(p.id, Array.from(selectedCandidates));
  providersStore.fetchedModelCandidates = [];
  selectedCandidates = new Set();
}

function handleImportAllCandidates() {
  const p = providersStore.selectedProvider;
  if (!p) return;
  providersStore.addMultipleModels(p.id, providersStore.fetchedModelCandidates);
  providersStore.fetchedModelCandidates = [];
  selectedCandidates = new Set();
}
</script>

<div class="p-6 space-y-6 max-w-7xl mx-auto font-sans">
  <!-- Node provider state: this is what actually decides whether the bot replies. -->
  <div class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-4 sm:p-5 shadow-xs space-y-4">
    <div class="flex flex-wrap items-start justify-between gap-4">
      <div class="flex items-start gap-3.5">
        <div
          class="p-2.5 rounded-xl {providersStore.nodeProvider?.configured
            ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
            : 'bg-amber-500/10 text-amber-600 dark:text-amber-400'}"
        >
          <Cpu class="w-6 h-6" />
        </div>
        <div>
          <div class="flex items-center gap-2.5 flex-wrap">
            <span class="text-sm text-zinc-500 font-medium">节点当前生效的提供商:</span>
            {#if providersStore.nodeProvider?.configured}
              <code class="px-2.5 py-1 rounded-lg font-mono text-sm font-bold bg-emerald-50 dark:bg-emerald-950/80 text-emerald-600 dark:text-emerald-400 border border-emerald-200 dark:border-emerald-800/60">
                {providersStore.nodeProvider.protocol} · {providersStore.nodeProvider.model}
              </code>
              <span class="px-2 py-0.5 rounded-md text-xs font-mono border border-zinc-200 dark:border-zinc-700 text-zinc-500">
                {providersStore.nodeProvider.source === 'console' ? '控制台已保存' : '环境变量引导'}
              </span>
            {:else}
              <span class="text-sm text-amber-600 dark:text-amber-400 font-medium">
                未配置 —— 机器人不会回复普通消息
              </span>
            {/if}
          </div>
          <p class="text-xs text-zinc-400 mt-1 font-mono break-all">
            {providersStore.nodeProvider?.base_url ?? 'base_url: 未设置'}
            {providersStore.nodeProvider?.api_key_configured ? ' · key: 已配置' : ' · key: 无'}
          </p>
        </div>
      </div>

      <button
        onclick={() => providersStore.activateOnNode()}
        disabled={providersStore.nodeActionPending || !providersStore.selectedProvider}
        class="px-3.5 py-2 bg-emerald-600 hover:bg-emerald-700 disabled:opacity-50 disabled:cursor-not-allowed text-white rounded-lg text-sm font-medium flex items-center gap-2 transition cursor-pointer shadow-2xs"
        title="把下方选中的提供商与模型写入节点配置并立即生效"
      >
        <Zap class="w-4 h-4" />
        <span>{providersStore.nodeActionPending ? '应用中...' : '应用到此节点'}</span>
      </button>
    </div>

    {#if providersStore.nodeMessage}
      <p class="text-xs text-emerald-600 dark:text-emerald-400 flex items-center gap-1.5">
        <CheckCircle2 class="w-3.5 h-3.5" /> {providersStore.nodeMessage}
      </p>
    {/if}
    {#if providersStore.nodeError}
      <p class="text-xs text-rose-600 dark:text-rose-400 flex items-center gap-1.5">
        <AlertCircle class="w-3.5 h-3.5" /> {providersStore.nodeError}
      </p>
    {/if}
    <p class="text-xs text-zinc-400">
      应用后会写入节点的 <code class="font-mono">data/system.json</code> 并立即对流水线、RequestLLM 与聊天接口生效，无需重启。
    </p>
  </div>

  <!-- Browser-local model playlist (Playground only; does not configure the node) -->
  <div class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-4 sm:p-5 shadow-xs flex flex-wrap items-center justify-between gap-4">
    <div class="flex items-center gap-3.5">
      <div class="p-2.5 rounded-xl bg-indigo-500/10 text-indigo-600 dark:text-indigo-400">
        <Bot class="w-6 h-6" />
      </div>
      <div>
        <div class="flex items-center gap-2.5">
          <span class="text-sm text-zinc-500 font-medium">浏览器本地选中的模型:</span>
          {#if providersStore.activeModel}
            <code class="px-2.5 py-1 rounded-lg font-mono text-sm font-bold bg-indigo-50 dark:bg-indigo-950/80 text-indigo-600 dark:text-indigo-400 border border-indigo-200 dark:border-indigo-800/60">
              {providersStore.activeModel}
            </code>
          {:else}
            <span class="text-sm text-zinc-400 font-mono">未设置 (暂无可用模型)</span>
          {/if}
        </div>
        <p class="text-xs text-zinc-400 mt-1 font-mono">
          仅用于本机 Playground 调试；要让机器人使用，请点击上方“应用到此节点”。
        </p>
      </div>
    </div>

    <!-- Quick Actions: One-click Presets & Switcher -->
    <div class="flex items-center gap-3">
      <!-- One-click quick config button -->
      <button
        onclick={() => (isQuickConfigOpen = true)}
        class="px-3.5 py-2 bg-indigo-600 hover:bg-indigo-700 text-white rounded-lg text-sm font-medium flex items-center gap-2 transition cursor-pointer shadow-2xs"
        title="使用主流服务商模板一键配置提供商"
      >
        <Sparkles class="w-4 h-4" />
        <span>一键配置</span>
      </button>

      {#if providersStore.allModelKeys.length > 0}
        <div class="flex items-center gap-2 text-sm font-mono ml-2">
          <label for="active-model-select" class="text-zinc-400 text-xs sm:text-sm">切换:</label>
          <select
            id="active-model-select"
            value={providersStore.activeModel}
            onchange={(e) => providersStore.setActiveModel(e.currentTarget.value)}
            class="px-3 py-1.5 text-xs sm:text-sm font-mono bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden cursor-pointer"
          >
            {#each providersStore.allModelKeys as key}
              <option value={key}>{key}</option>
            {/each}
          </select>
        </div>
      {/if}
    </div>
  </div>

  <!-- If no providers configured at all: Clean Empty State with Quick Actions -->
  {#if providersStore.providers.length === 0}
    <div class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-2xl p-12 text-center shadow-xs space-y-6">
      <div class="w-16 h-16 rounded-2xl bg-zinc-100 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 flex items-center justify-center mx-auto text-zinc-400">
        <Server class="w-8 h-8 stroke-[1.5]" />
      </div>

      <div class="max-w-lg mx-auto space-y-2">
        <h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">暂未配置模型提供商</h3>
        <p class="text-sm text-zinc-500 leading-relaxed">
          先配置提供商（API Base URL 与密钥），再配置该提供商旗下的模型。模型在系统中将统一表示为 <code class="font-mono font-semibold text-zinc-700 dark:text-zinc-300">提供商名称/模型ID</code>。
        </p>
      </div>

      <div class="flex flex-wrap items-center justify-center gap-3.5 pt-2">
        <button
          onclick={() => (isQuickConfigOpen = true)}
          class="px-5 py-2.5 bg-indigo-600 hover:bg-indigo-700 text-white rounded-xl text-sm font-medium flex items-center gap-2 transition cursor-pointer shadow-xs"
        >
          <Sparkles class="w-4.5 h-4.5" />
          <span>一键配置常见提供商</span>
        </button>
        <button
          onclick={() => (isAddingProvider = true)}
          class="px-5 py-2.5 bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-800 dark:text-zinc-200 rounded-xl text-sm font-medium flex items-center gap-2 transition cursor-pointer"
        >
          <Plus class="w-4.5 h-4.5" />
          <span>手动添加提供商</span>
        </button>
      </div>

      <!-- Presets quick list preview -->
      {#if providersStore.catalog?.presets}
        <div class="pt-6 border-t border-zinc-100 dark:border-zinc-800/80 max-w-2xl mx-auto">
          <span class="text-xs font-medium text-zinc-400 block mb-3 font-mono">支持一键配置的主流预设：</span>
          <div class="flex flex-wrap items-center justify-center gap-2.5">
            {#each providersStore.catalog.presets as preset}
              <button
                onclick={() => handleApplyPreset(preset)}
                class="px-3.5 py-2 rounded-lg border border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-950 hover:border-zinc-400 dark:hover:border-zinc-600 text-xs sm:text-sm font-medium text-zinc-700 dark:text-zinc-300 transition cursor-pointer flex items-center gap-2"
              >
                <span>{preset.name}</span>
                <span class="text-xs text-zinc-400 font-mono">({preset.protocol})</span>
              </button>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {:else}
    <!-- Two-Tier Layout: Left Providers List / Right Selected Provider & Models -->
    <div class="grid grid-cols-1 lg:grid-cols-12 gap-6 items-start">
      <!-- Left Column: Providers List (4 cols) -->
      <div class="lg:col-span-4 space-y-3">
        <div class="flex items-center justify-between pb-1">
          <div class="flex items-center gap-2">
            <Server class="w-4.5 h-4.5 text-zinc-500" />
            <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">提供商列表</h3>
          </div>
          <button
            onclick={() => (isAddingProvider = true)}
            class="px-3 py-1.5 bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900 hover:bg-zinc-800 dark:hover:bg-zinc-200 rounded-lg text-xs sm:text-sm font-medium flex items-center gap-1.5 transition cursor-pointer shadow-2xs"
          >
            <Plus class="w-4 h-4" />
            <span>添加提供商</span>
          </button>
        </div>

        <div class="space-y-2.5">
          {#each providersStore.providers as prov (prov.id)}
            <div
              onclick={() => providersStore.selectProvider(prov.id)}
              onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') providersStore.selectProvider(prov.id); }}
              role="button"
              tabindex="0"
              class="w-full text-left p-4 rounded-xl border transition cursor-pointer flex items-center justify-between
                {providersStore.selectedProviderId === prov.id
                  ? 'bg-zinc-100/90 dark:bg-zinc-800/90 border-zinc-300 dark:border-zinc-700 shadow-xs ring-1 ring-zinc-400 dark:ring-zinc-600'
                  : 'bg-white dark:bg-zinc-900 border-zinc-200 dark:border-zinc-800 hover:border-zinc-300 dark:hover:border-zinc-700'}"
            >
              <div>
                <div class="flex items-center gap-2">
                  <span class="font-mono font-bold text-sm text-zinc-900 dark:text-zinc-100">{prov.name}</span>
                  <span class="text-xs font-mono px-2 py-0.5 rounded bg-zinc-200/70 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400">
                    {prov.protocol}
                  </span>
                </div>
                <p class="text-xs text-zinc-400 font-mono truncate max-w-[220px] mt-1.5" title={prov.base_url}>
                  {prov.base_url || 'Default Endpoint'}
                </p>
              </div>
              <div class="text-right">
                <span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-mono font-medium bg-zinc-100 dark:bg-zinc-800 text-zinc-500">
                  {prov.models.length} 个模型
                </span>
              </div>
            </div>
          {/each}
        </div>
      </div>

      <!-- Right Column: Provider Configuration & Model Details (8 cols) -->
      <div class="lg:col-span-8 space-y-6">
        {#if providersStore.selectedProvider}
          {@const prov = providersStore.selectedProvider}

          <!-- 1. Provider Core Config Card -->
          <div class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-5 sm:p-6 shadow-xs space-y-4">
            <div class="flex items-center justify-between pb-3.5 border-b border-zinc-100 dark:border-zinc-800">
              <div>
                <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100 flex items-center gap-2">
                  <span>提供商配置:</span>
                  <code class="font-mono text-indigo-600 dark:text-indigo-400 font-bold text-base">{prov.name}</code>
                </h3>
                <p class="text-xs sm:text-sm text-zinc-500 font-mono mt-1">
                  此前缀将作用于模型标识: <span class="text-zinc-700 dark:text-zinc-300 font-semibold">{prov.name}/&lt;模型ID&gt;</span>
                </p>
              </div>

              <div class="flex items-center gap-2.5">
                {#if saveNotification}
                  <span class="text-xs sm:text-sm text-emerald-500 font-mono flex items-center gap-1">
                    <Check class="w-4 h-4" />
                    已保存
                  </span>
                {/if}
                <button
                  onclick={handleSaveProvider}
                  class="px-3.5 py-2 bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900 hover:bg-zinc-800 dark:hover:bg-zinc-200 rounded-lg text-sm font-medium transition cursor-pointer"
                >
                  保存修改
                </button>
                <button
                  onclick={() => handleDeleteProvider(prov.id)}
                  class="p-2 text-zinc-400 hover:text-rose-500 transition cursor-pointer"
                  title="删除该提供商"
                >
                  <Trash2 class="w-4.5 h-4.5" />
                </button>
              </div>
            </div>

            <!-- Provider Edit Form -->
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 font-mono">
              <div>
                <label for="prov-name-input" class="block text-xs sm:text-sm font-medium text-zinc-500 mb-1.5">提供商名称 (标识符):</label>
                <input
                  id="prov-name-input"
                  type="text"
                  bind:value={editName}
                  onblur={handleSaveProvider}
                  placeholder="例如: deepseek, openai"
                  class="w-full px-3.5 py-2 text-sm bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden"
                />
              </div>

              <div>
                <label for="prov-protocol-select" class="block text-xs sm:text-sm font-medium text-zinc-500 mb-1.5">协议类型:</label>
                <select
                  id="prov-protocol-select"
                  bind:value={editProtocol}
                  onchange={handleSaveProvider}
                  class="w-full px-3.5 py-2 text-sm bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden cursor-pointer"
                >
                  <option value="openai">OpenAI Compatible (Chat Completions)</option>
                  <option value="openai_responses">OpenAI Responses API</option>
                  <option value="anthropic">Anthropic Messages API</option>
                </select>
              </div>

              <div class="sm:col-span-2">
                <label for="prov-base-url-input" class="block text-xs sm:text-sm font-medium text-zinc-500 mb-1.5">接口地址 (Base URL):</label>
                <input
                  id="prov-base-url-input"
                  type="text"
                  bind:value={editBaseUrl}
                  onblur={handleSaveProvider}
                  placeholder="例如: https://api.deepseek.com/v1"
                  class="w-full px-3.5 py-2 text-sm bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden"
                />
              </div>

              <div class="sm:col-span-2">
                <label for="prov-api-key-input" class="block text-xs sm:text-sm font-medium text-zinc-500 mb-1.5">API 密钥 (API Key):</label>
                <div class="relative">
                  <input
                    id="prov-api-key-input"
                    type={showApiKey ? 'text' : 'password'}
                    bind:value={editApiKey}
                    onblur={handleSaveProvider}
                    placeholder="sk-..."
                    class="w-full px-3.5 py-2 pr-10 text-sm bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden"
                  />
                  <button
                    type="button"
                    onclick={() => (showApiKey = !showApiKey)}
                    class="absolute right-3 top-2.5 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200"
                  >
                    {#if showApiKey}
                      <EyeOff class="w-4 h-4" />
                    {:else}
                      <Eye class="w-4 h-4" />
                    {/if}
                  </button>
                </div>
                <span class="text-xs text-zinc-400 dark:text-zinc-500 mt-1.5 block">
                  在线模型服务商（如 DeepSeek、OpenAI）必须填入有效 API Key 方可获取模型列表或对话测试。
                </span>
              </div>
            </div>
          </div>

          <!-- 2. Models under this Provider Card -->
          <div class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-5 shadow-xs space-y-4">
            <div class="flex items-center justify-between pb-3 border-b border-zinc-100 dark:border-zinc-800">
              <div class="flex items-center gap-2">
                <Cpu class="w-4.5 h-4.5 text-indigo-500" />
                <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">
                  该提供商旗下的模型
                </h3>
              </div>

              <!-- Fetch Models from Remote Provider Button -->
              <button
                onclick={handleFetchRemoteModels}
                disabled={providersStore.isFetchingModels || !prov.base_url}
                class="px-3 py-1.5 bg-indigo-50 dark:bg-indigo-950/60 hover:bg-indigo-100 dark:hover:bg-indigo-900/60 text-indigo-600 dark:text-indigo-400 border border-indigo-200 dark:border-indigo-800/80 rounded-lg text-xs sm:text-sm font-medium font-mono flex items-center gap-1.5 transition cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                title="通过接口自动拉取远端提供商支持的所有模型列表"
              >
                {#if providersStore.isFetchingModels}
                  <RefreshCw class="w-3.5 h-3.5 animate-spin" />
                  <span>正在获取模型列表...</span>
                {:else}
                  <ArrowDownToLine class="w-4 h-4" />
                  <span>获取模型列表</span>
                {/if}
              </button>
            </div>

            <!-- Remote Discovered Models Dropdown / Selection Bar (when fetched) -->
            {#if providersStore.fetchModelsError}
              <div class="p-3 bg-rose-500/10 border border-rose-500/20 text-rose-600 dark:text-rose-400 text-xs sm:text-sm rounded-lg flex items-center justify-between font-mono">
                <span>拉取模型列表失败: {providersStore.fetchModelsError}</span>
                <button
                  onclick={() => (providersStore.fetchModelsError = null)}
                  class="text-rose-400 hover:text-rose-600 ml-2"
                >
                  ✕
                </button>
              </div>
            {/if}

            {#if providersStore.fetchedModelCandidates.length > 0}
              <div class="p-4 bg-indigo-50/50 dark:bg-indigo-950/30 border border-indigo-200/80 dark:border-indigo-800/60 rounded-xl space-y-3">
                <div class="flex items-center justify-between">
                  <div class="flex items-center gap-2">
                    <Sparkles class="w-4.5 h-4.5 text-indigo-500" />
                    <span class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                      远端获取到 {providersStore.fetchedModelCandidates.length} 个模型
                    </span>
                  </div>
                  <div class="flex items-center gap-2">
                    <button
                      onclick={handleImportSelectedCandidates}
                      disabled={selectedCandidates.size === 0}
                      class="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-700 text-white rounded-md text-xs sm:text-sm font-medium transition cursor-pointer disabled:opacity-50"
                    >
                      导入选中 ({selectedCandidates.size})
                    </button>
                    <button
                      onclick={handleImportAllCandidates}
                      class="px-3 py-1.5 bg-zinc-200 dark:bg-zinc-800 hover:bg-zinc-300 dark:hover:bg-zinc-700 text-zinc-800 dark:text-zinc-200 rounded-md text-xs sm:text-sm font-medium transition cursor-pointer"
                    >
                      全部导入
                    </button>
                  </div>
                </div>

                <div class="max-h-48 overflow-y-auto grid grid-cols-1 sm:grid-cols-2 gap-1.5 pr-1 font-mono text-xs">
                  {#each providersStore.fetchedModelCandidates as cand}
                    <label class="flex items-center gap-2 p-2 rounded bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 hover:border-zinc-300 cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={selectedCandidates.has(cand)}
                        onchange={(e) => {
                          if (e.currentTarget.checked) selectedCandidates.add(cand);
                          else selectedCandidates.delete(cand);
                          selectedCandidates = new Set(selectedCandidates);
                        }}
                        class="rounded text-indigo-600"
                      />
                      <span class="truncate text-zinc-800 dark:text-zinc-200">{cand}</span>
                    </label>
                  {/each}
                </div>
              </div>
            {/if}

            <!-- Add Model Row -->
            <div class="p-3.5 bg-zinc-50 dark:bg-zinc-950/50 rounded-lg border border-zinc-100 dark:border-zinc-800/80 space-y-2">
              <span class="text-sm font-medium text-zinc-700 dark:text-zinc-300 block">
                手动添加模型到 <code class="font-mono font-bold text-indigo-600 dark:text-indigo-400">{prov.name}</code>:
              </span>
              <div class="flex flex-wrap items-center gap-2">
                <div class="flex-1 min-w-[200px]">
                  <input
                    type="text"
                    bind:value={newModelId}
                    placeholder="输入模型 ID (如: deepseek-chat, gpt-4o)"
                    class="w-full px-3.5 py-2 text-sm font-mono bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-700 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden"
                  />
                </div>
                <div class="w-48">
                  <input
                    type="text"
                    bind:value={newModelDisplayName}
                    placeholder="展示名称 (可选)"
                    class="w-full px-3.5 py-2 text-sm bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-700 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden"
                  />
                </div>
                <button
                  onclick={handleAddModel}
                  disabled={!newModelId.trim()}
                  class="px-3.5 py-2 bg-indigo-600 hover:bg-indigo-700 text-white rounded-lg text-sm font-medium transition cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed shrink-0 flex items-center gap-1"
                >
                  <Plus class="w-4 h-4" />
                  <span>添加模型</span>
                </button>
              </div>
              {#if newModelId.trim()}
                <div class="text-xs font-mono text-zinc-500">
                  将生成模型表示:
                  <code class="font-bold text-indigo-600 dark:text-indigo-400">{prov.name}/{newModelId.trim()}</code>
                </div>
              {/if}
            </div>

            <!-- Models List -->
            <div class="space-y-2.5">
              {#each prov.models as model (model.id)}
                {@const fullKey = `${prov.name}/${model.id}`}
                {@const isActive = providersStore.activeModel === fullKey}
                {@const testResult = providersStore.modelTestResults[fullKey]}
                {@const isTesting = providersStore.testingModelKey === fullKey}

                <div class="p-3.5 rounded-lg border transition flex flex-col sm:flex-row sm:items-center justify-between gap-3
                  {isActive
                    ? 'bg-indigo-50/40 dark:bg-indigo-950/20 border-indigo-200 dark:border-indigo-800/80 shadow-2xs'
                    : 'bg-white dark:bg-zinc-900 border-zinc-200 dark:border-zinc-800'}">
                  <div>
                    <div class="flex items-center gap-2">
                      <!-- 设定的提供商名称/模型ID 突出展示 -->
                      <code class="font-mono font-bold text-sm text-indigo-600 dark:text-indigo-400 bg-indigo-500/10 px-2.5 py-1 rounded border border-indigo-500/20">
                        {fullKey}
                      </code>

                      {#if isActive}
                        <span class="inline-flex items-center gap-1 px-2.5 py-0.5 rounded text-xs font-medium font-mono bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20">
                          <Star class="w-3.5 h-3.5 fill-emerald-500" />
                          默认活跃模型
                        </span>
                      {/if}
                    </div>

                    <div class="flex items-center gap-3 text-xs sm:text-sm text-zinc-400 font-mono mt-1.5">
                      <span>原始ID: {model.id}</span>
                      {#if model.name && model.name !== model.id}
                        <span>• 别名: {model.name}</span>
                      {/if}
                    </div>
                  </div>

                  <!-- Right Action Buttons -->
                  <div class="flex items-center gap-2 shrink-0">
                    {#if testResult}
                      <span class="text-xs font-mono px-2.5 py-1 rounded flex items-center gap-1
                        {testResult.status === 'ok'
                          ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
                          : 'bg-rose-500/10 text-rose-600 dark:text-rose-400'}"
                        title={testResult.error || testResult.reply || ''}
                      >
                        {#if testResult.status === 'ok'}
                          <CheckCircle2 class="w-3.5 h-3.5" />
                          <span>{testResult.latency_ms}ms</span>
                        {:else}
                          <AlertCircle class="w-3.5 h-3.5" />
                          <span>失败</span>
                        {/if}
                      </span>
                    {/if}

                    <button
                      onclick={() => handleTestModel(prov.id, model.id)}
                      disabled={isTesting}
                      class="px-3 py-1.5 text-xs sm:text-sm font-mono bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-700 dark:text-zinc-300 rounded-md transition cursor-pointer flex items-center gap-1 disabled:opacity-50"
                      title="测试连通性与网络延迟"
                    >
                      {#if isTesting}
                        <RefreshCw class="w-3.5 h-3.5 animate-spin" />
                        <span>测速中</span>
                      {:else}
                        <Zap class="w-3.5 h-3.5" />
                        <span>测试</span>
                      {/if}
                    </button>

                    {#if !isActive}
                      <button
                        onclick={() => providersStore.setActiveModel(fullKey)}
                        class="px-3 py-1.5 text-xs sm:text-sm font-mono bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-700 dark:text-zinc-300 rounded-md transition cursor-pointer"
                        title="设为全局默认活跃模型"
                      >
                        设为默认
                      </button>
                    {/if}

                    <button
                      onclick={() => handleDeleteModel(model.id)}
                      class="p-1.5 text-zinc-400 hover:text-rose-500 transition cursor-pointer"
                      title="删除模型"
                    >
                      <Trash2 class="w-4 h-4" />
                    </button>
                  </div>
                </div>
              {/each}

              {#if prov.models.length === 0}
                <div class="p-8 text-center text-sm text-zinc-400 border border-dashed border-zinc-200 dark:border-zinc-800 rounded-lg space-y-2">
                  <p>该提供商下暂无模型。</p>
                  <p class="text-zinc-500 text-xs sm:text-sm">点击右上角“获取模型列表”自动拉取，或在上方手动输入添加。</p>
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<!-- Modal: 一键配置提供商 (Quick Config Modal) -->
{#if isQuickConfigOpen}
  <!-- Backdrop -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="fixed inset-0 bg-black/40 backdrop-blur-xs z-50 flex items-center justify-center p-4"
    onclick={() => (isQuickConfigOpen = false)}
    role="button"
    tabindex="-1"
  >
    <!-- Modal Dialog -->
    <div
      class="w-full max-w-lg bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl shadow-2xl p-6 space-y-4"
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      tabindex="-1"
    >
      <div class="flex items-center justify-between pb-3 border-b border-zinc-100 dark:border-zinc-800">
        <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100 flex items-center gap-2">
          <Sparkles class="w-4.5 h-4.5 text-indigo-500" />
          <span>一键配置提供商模板</span>
        </h3>
        <button
          onclick={() => (isQuickConfigOpen = false)}
          class="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 text-sm"
        >
          ✕
        </button>
      </div>

      <div class="space-y-3 text-sm">
        <p class="text-zinc-500 leading-relaxed">
          选择一个常见大模型服务商模板，将自动填入协议、Base URL 及默认模型，创建后填入 API 密钥即可直接使用：
        </p>

        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1">
          {#each providersStore.catalog?.presets ?? [] as preset}
            <button
              onclick={() => handleApplyPreset(preset)}
              class="p-3.5 text-left rounded-xl border border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-950/60 hover:border-indigo-400 dark:hover:border-indigo-600 hover:bg-white dark:hover:bg-zinc-900 transition cursor-pointer flex flex-col justify-between group shadow-2xs"
            >
              <div>
                <div class="flex items-center justify-between">
                  <span class="font-bold text-sm text-zinc-900 dark:text-zinc-100 group-hover:text-indigo-600 dark:group-hover:text-indigo-400">
                    {preset.name}
                  </span>
                  <span class="text-xs font-mono px-1.5 py-0.5 rounded bg-zinc-200/70 dark:bg-zinc-800 text-zinc-500">
                    {preset.protocol}
                  </span>
                </div>
                <span class="text-xs font-mono text-zinc-400 block truncate mt-1" title={preset.base_url}>
                  {preset.base_url}
                </span>
              </div>
              <div class="mt-3 pt-2 border-t border-zinc-100 dark:border-zinc-800/80 flex items-center justify-between text-xs font-mono text-zinc-500">
                <span>协议类型:</span>
                <span class="font-bold text-zinc-700 dark:text-zinc-300">{preset.protocol}</span>
              </div>
            </button>
          {/each}
        </div>
      </div>
    </div>
  </div>
{/if}

<!-- Modal: 手动添加提供商 (Add Provider Modal) -->
{#if isAddingProvider}
  <!-- Backdrop -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="fixed inset-0 bg-black/40 backdrop-blur-xs z-50 flex items-center justify-center p-4"
    onclick={() => (isAddingProvider = false)}
    role="button"
    tabindex="-1"
  >
    <!-- Modal Dialog -->
    <div
      class="w-full max-w-md bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl shadow-2xl p-6 space-y-4"
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      tabindex="-1"
    >
      <div class="flex items-center justify-between pb-3 border-b border-zinc-100 dark:border-zinc-800">
        <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100 flex items-center gap-2">
          <Plus class="w-4.5 h-4.5 text-indigo-500" />
          <span>手动添加模型提供商</span>
        </h3>
        <button
          onclick={() => (isAddingProvider = false)}
          class="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 text-sm"
        >
          ✕
        </button>
      </div>

      <div class="space-y-3.5 text-sm font-mono">
        <div>
          <label for="new-prov-name" class="block text-xs font-sans text-zinc-500 mb-1">提供商名称 (前缀标识符):</label>
          <input
            id="new-prov-name"
            type="text"
            bind:value={newProvName}
            placeholder="例如: deepseek, openai, siliconflow"
            class="w-full px-3.5 py-2 text-sm bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden"
          />
          <p class="text-xs font-sans text-zinc-400 mt-1">作为该提供商旗下模型的前缀 (如 deepseek/deepseek-chat)</p>
        </div>

        <div>
          <label for="new-prov-protocol" class="block text-xs font-sans text-zinc-500 mb-1">协议类型:</label>
          <select
            id="new-prov-protocol"
            bind:value={newProvProtocol}
            class="w-full px-3.5 py-2 text-sm bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden cursor-pointer"
          >
            <option value="openai">OpenAI Compatible (Chat Completions)</option>
            <option value="openai_responses">OpenAI Responses API</option>
            <option value="anthropic">Anthropic Messages API</option>
          </select>
        </div>

        <div>
          <label for="new-prov-base-url" class="block text-xs font-sans text-zinc-500 mb-1">接口地址 (Base URL):</label>
          <input
            id="new-prov-base-url"
            type="text"
            bind:value={newProvBaseUrl}
            placeholder="例如: https://api.deepseek.com/v1"
            class="w-full px-3.5 py-2 text-sm bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden"
          />
        </div>

        <div>
          <label for="new-prov-api-key" class="block text-xs font-sans text-zinc-500 mb-1">API 密钥 (API Key):</label>
          <input
            id="new-prov-api-key"
            type="password"
            bind:value={newProvApiKey}
            placeholder="sk-..."
            class="w-full px-3.5 py-2 text-sm bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 rounded-lg text-zinc-900 dark:text-zinc-100 focus:outline-hidden"
          />
        </div>
      </div>

      <div class="pt-3 border-t border-zinc-100 dark:border-zinc-800 flex items-center justify-end gap-2">
        <button
          onclick={() => (isAddingProvider = false)}
          class="px-3.5 py-2 bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-700 dark:text-zinc-300 rounded-lg text-sm font-medium cursor-pointer"
        >
          取消
        </button>
        <button
          onclick={handleCreateProvider}
          disabled={!newProvName.trim()}
          class="px-4 py-2 bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900 hover:bg-zinc-800 dark:hover:bg-zinc-200 rounded-lg text-sm font-medium cursor-pointer disabled:opacity-50"
        >
          创建提供商
        </button>
      </div>
    </div>
  </div>
{/if}
