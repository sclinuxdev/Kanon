<script lang="ts">
import {
  Bot,
  CheckCircle2,
  Send,
  Sliders,
  Sparkles,
  Square,
  Trash2,
  User,
  Wrench,
  XCircle,
} from 'lucide-svelte';
import { api } from '../../api/client';
import { streamChatCompletion } from '../../api/sse';
import { t } from '../../stores/i18n.svelte';
import type { ExecutedTool } from '../../types';

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  executedTools?: ExecutedTool[];
  timestamp: Date;
}

let sessionId = $state('webui:playground');
let inputMessage = $state('');
let isStreaming = $state(false);
let enableTools = $state(true);
let messages = $state<ChatMessage[]>([]);
let abortController: AbortController | null = null;
let chatContainer = $state<HTMLDivElement | null>(null);

// Auto-scroll chat
$effect(() => {
  if (messages.length > 0 && chatContainer) {
    chatContainer.scrollTop = chatContainer.scrollHeight;
  }
});

async function sendMessage() {
  const text = inputMessage.trim();
  if (!text || isStreaming) return;

  inputMessage = '';
  const userMsg: ChatMessage = {
    id: `usr_${Date.now()}`,
    role: 'user',
    content: text,
    timestamp: new Date(),
  };
  messages = [...messages, userMsg];

  const assistantMsg: ChatMessage = {
    id: `ast_${Date.now()}`,
    role: 'assistant',
    content: '',
    executedTools: [],
    timestamp: new Date(),
  };
  messages = [...messages, assistantMsg];

  isStreaming = true;
  abortController = new AbortController();

  try {
    await streamChatCompletion(
      {
        session_id: sessionId,
        message: text,
        tools: enableTools,
      },
      {
        onChunk: (delta) => {
          assistantMsg.content += delta;
          messages = [...messages];
        },
        onFinish: () => {
          isStreaming = false;
        },
        onError: (err) => {
          assistantMsg.content += `\n[Stream Error: ${err.message}]`;
          isStreaming = false;
          messages = [...messages];
        },
      },
      abortController.signal,
    );
  } catch (err) {
    assistantMsg.content += `\n[Failed to initiate completion: ${err}]`;
    isStreaming = false;
    messages = [...messages];
  }
}

function handleStop() {
  if (abortController) {
    abortController.abort();
    abortController = null;
  }
  isStreaming = false;
}

function clearChat() {
  messages = [];
}
</script>

<div class="h-full flex flex-col bg-zinc-50/50 dark:bg-zinc-950/50 overflow-hidden">
  <!-- Top Control Bar -->
  <div class="p-3 border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 flex items-center justify-between shrink-0">
    <div class="flex items-center gap-3">
      <div class="flex items-center gap-2">
        <Bot class="w-4 h-4 text-indigo-500" />
        <span class="text-xs font-semibold text-zinc-900 dark:text-zinc-100">{t('nav.playground')}</span>
      </div>
      <div class="flex items-center gap-1.5 text-xs">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="text-zinc-400 font-mono text-[11px]">{t('sessions.session_id')}:</label>
        <input
          type="text"
          bind:value={sessionId}
          class="px-2 py-0.5 rounded font-mono text-xs bg-zinc-100 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 text-zinc-800 dark:text-zinc-200 w-36"
        />
      </div>
    </div>

    <div class="flex items-center gap-3">
      <!-- Toggle tools -->
      <label class="flex items-center gap-1.5 text-xs text-zinc-600 dark:text-zinc-400 cursor-pointer select-none">
        <input
          type="checkbox"
          bind:checked={enableTools}
          class="rounded text-indigo-600 focus:ring-0"
        />
        <span>{t('playground.enable_tools')}</span>
      </label>

      <!-- Clear messages -->
      <button
        onclick={clearChat}
        class="p-1 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 transition cursor-pointer"
        title={t('common.clear')}
      >
        <Trash2 class="w-3.5 h-3.5" />
      </button>
    </div>
  </div>

  <!-- Messages Scroll Area -->
  <div
    bind:this={chatContainer}
    class="flex-1 p-6 overflow-y-auto space-y-4 max-w-4xl w-full mx-auto"
  >
    {#if messages.length === 0}
      <div class="p-16 text-center space-y-3">
        <div class="w-12 h-12 rounded-2xl bg-indigo-50 dark:bg-indigo-950/60 border border-indigo-200 dark:border-indigo-800/60 flex items-center justify-center mx-auto text-indigo-600 dark:text-indigo-400">
          <Bot class="w-6 h-6" />
        </div>
        <h4 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">{t('title.playground')}</h4>
        <p class="text-xs text-zinc-500 max-w-sm mx-auto leading-relaxed">
          {t('playground.empty_chat')}
        </p>
      </div>
    {:else}
      {#each messages as msg}
        <div class="flex items-start gap-3 text-xs leading-relaxed {msg.role === 'user' ? 'justify-end' : 'justify-start'}">
          {#if msg.role === 'assistant'}
            <div class="w-6 h-6 rounded-lg bg-indigo-600 text-white flex items-center justify-center shrink-0 mt-0.5">
              <Bot class="w-3.5 h-3.5" />
            </div>
          {/if}

          <div class="space-y-2 max-w-[85%] sm:max-w-2xl">
            <div class="p-3 rounded-2xl shadow-2xs text-xs
              {msg.role === 'user'
                ? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900 font-medium'
                : 'bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 text-zinc-800 dark:text-zinc-200'}">
              <div class="whitespace-pre-wrap">{msg.content || (isStreaming ? 'Thinking...' : '')}</div>
            </div>

            <!-- Executed Tools inspection cards -->
            {#if msg.executedTools && msg.executedTools.length > 0}
              <div class="space-y-1">
                {#each msg.executedTools as tool}
                  <div class="p-2 rounded-lg bg-zinc-100 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 flex items-center justify-between text-[11px] font-mono">
                    <div class="flex items-center gap-1.5">
                      <Wrench class="w-3 h-3 text-indigo-500" />
                      <span class="font-bold">{tool.tool_name}</span>
                      <span class="text-zinc-400">({tool.plugin_id})</span>
                    </div>
                    <span class="flex items-center gap-1 {tool.success ? 'text-emerald-500' : 'text-rose-500'} font-semibold">
                      {#if tool.success}
                        <CheckCircle2 class="w-3 h-3" />
                        <span>Success</span>
                      {:else}
                        <XCircle class="w-3 h-3" />
                        <span>Failed</span>
                      {/if}
                    </span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>

          {#if msg.role === 'user'}
            <div class="w-6 h-6 rounded-lg bg-zinc-200 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300 flex items-center justify-center shrink-0 mt-0.5">
              <User class="w-3.5 h-3.5" />
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>

  <!-- Input Bar -->
  <div class="p-4 border-t border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shrink-0">
    <div class="max-w-4xl mx-auto flex items-center gap-2">
      <input
        type="text"
        bind:value={inputMessage}
        onkeydown={(e) => e.key === 'Enter' && sendMessage()}
        placeholder={t('playground.placeholder')}
        class="flex-1 px-3.5 py-2.5 rounded-xl bg-zinc-100 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 text-xs text-zinc-900 dark:text-zinc-100 placeholder-zinc-400 focus:outline-hidden focus:border-zinc-400 dark:focus:border-zinc-600"
      />
      {#if isStreaming}
        <button
          onclick={handleStop}
          class="p-2.5 bg-rose-600 hover:bg-rose-500 text-white rounded-xl font-medium transition cursor-pointer flex items-center justify-center"
          title="Stop generation"
        >
          <Square class="w-4 h-4 fill-current" />
        </button>
      {:else}
        <button
          onclick={sendMessage}
          disabled={!inputMessage.trim()}
          class="p-2.5 bg-zinc-900 hover:bg-zinc-800 dark:bg-zinc-100 dark:hover:bg-zinc-200 text-white dark:text-zinc-900 rounded-xl font-medium transition cursor-pointer flex items-center justify-center disabled:opacity-40"
          title={t('playground.send')}
        >
          <Send class="w-4 h-4" />
        </button>
      {/if}
    </div>
  </div>
</div>
