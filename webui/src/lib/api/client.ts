import type {
  AdaptersResponse,
  ChatCompletionRequest,
  ChatCompletionResponse,
  FetchModelsRequest,
  FetchModelsResponse,
  NodeHealth,
  PersonasResponse,
  PluginConfigResponse,
  PluginsResponse,
  ProvidersCatalog,
  SessionsResponse,
  SystemConfig,
  TestProviderRequest,
  TestProviderResponse,
} from '../types';

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
    ...options,
  });

  if (!res.ok) {
    let code = 'error';
    let message = `HTTP ${res.status}: ${res.statusText}`;
    try {
      const errJson = await res.json();
      if (errJson.error?.code) code = errJson.error.code;
      else if (errJson.code) code = errJson.code;
      if (errJson.error?.message) message = errJson.error.message;
      else if (errJson.message) message = errJson.message;
    } catch {
      // ignore json parse error
    }
    throw new ApiError(res.status, code, message);
  }

  return res.json() as Promise<T>;
}

export const api = {
  getHealth: () => request<NodeHealth>('/api/v1/health'),
  getMetrics: async () => {
    const res = await fetch('/api/v1/metrics');
    return res.text();
  },

  getSystemConfig: () => request<SystemConfig>('/api/v1/system/config'),
  getProviders: () => request<ProvidersCatalog>('/api/v1/providers'),
  testProvider: (req?: TestProviderRequest) =>
    request<TestProviderResponse>('/api/v1/providers/test', {
      method: 'POST',
      body: JSON.stringify(req ?? {}),
    }),
  fetchModels: (req: FetchModelsRequest) =>
    request<FetchModelsResponse>('/api/v1/providers/models', {
      method: 'POST',
      body: JSON.stringify(req),
    }),

  getPlugins: () => request<PluginsResponse>('/api/v1/plugins'),
  getPluginConfig: (pluginId: string) =>
    request<PluginConfigResponse>(
      `/api/v1/plugins/${encodeURIComponent(pluginId)}/config`,
    ),
  updatePluginConfig: (
    pluginId: string,
    config: Record<string, unknown>,
    expectedCasVersion: number,
  ) =>
    request<{ success: boolean; cas_version: number }>(
      `/api/v1/plugins/${encodeURIComponent(pluginId)}/config`,
      {
        method: 'PUT',
        body: JSON.stringify({
          config,
          expected_cas_version: expectedCasVersion,
        }),
      },
    ),
  restartPlugin: (pluginId: string) =>
    request<{ success: boolean; message: string }>(
      `/api/v1/plugins/${encodeURIComponent(pluginId)}/restart`,
      { method: 'POST' },
    ),

  getAdapters: () => request<AdaptersResponse>('/api/v1/adapters'),
  ingestEvent: (platform: string, payload: Record<string, unknown>) =>
    request<{ accepted: boolean; event_id: string }>(
      `/api/v1/adapters/${encodeURIComponent(platform)}/ingest`,
      {
        method: 'POST',
        body: JSON.stringify(payload),
      },
    ),

  getSessions: (limit = 50, offset = 0) =>
    request<SessionsResponse>(
      `/api/v1/sessions?limit=${limit}&offset=${offset}`,
    ),
  resetSession: (sessionId: string) =>
    request<{ success: boolean }>(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}/reset`,
      {
        method: 'POST',
      },
    ),
  setSessionPersona: (sessionId: string, persona: string) =>
    request<{ success: boolean }>(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}/persona`,
      {
        method: 'POST',
        body: JSON.stringify({ persona }),
      },
    ),

  getPersonas: () => request<PersonasResponse>('/api/v1/personas'),

  chatCompletion: (req: ChatCompletionRequest) =>
    request<ChatCompletionResponse>('/api/v1/chat/completions', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
};
