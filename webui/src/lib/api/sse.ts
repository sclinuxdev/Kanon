import type { ChatCompletionRequest } from '../types';

export interface StreamCallbacks {
  onChunk: (delta: string, reasoning?: string) => void;
  onFinish?: (reason?: string) => void;
  onError?: (err: Error) => void;
}

export async function streamChatCompletion(
  req: ChatCompletionRequest,
  callbacks: StreamCallbacks,
  signal?: AbortSignal,
): Promise<void> {
  try {
    const res = await fetch('/api/v1/chat/completions', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'text/event-stream',
      },
      body: JSON.stringify({ ...req, stream: true }),
      signal,
    });

    if (!res.ok) {
      const errText = await res.text();
      throw new Error(`HTTP ${res.status}: ${errText}`);
    }

    const reader = res.body?.getReader();
    if (!reader) {
      throw new Error('ReadableStream not supported on response body');
    }

    const decoder = new TextDecoder();
    let buffer = '';

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer = lines.pop() ?? '';

      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed || trimmed.startsWith(':')) continue;

        if (trimmed.startsWith('data:')) {
          const payload = trimmed.slice(5).trim();
          if (payload === '[DONE]') {
            callbacks.onFinish?.('stop');
            return;
          }

          try {
            const data = JSON.parse(payload);
            if (data.delta !== undefined || data.reasoning !== undefined) {
              callbacks.onChunk(data.delta ?? '', data.reasoning ?? undefined);
            }
            if (data.finish_reason) {
              callbacks.onFinish?.(data.finish_reason);
            }
          } catch {
            // raw text fallback
            callbacks.onChunk(payload);
          }
        }
      }
    }

    callbacks.onFinish?.('stop');
  } catch (err) {
    if (signal?.aborted) return;
    callbacks.onError?.(err instanceof Error ? err : new Error(String(err)));
  }
}
