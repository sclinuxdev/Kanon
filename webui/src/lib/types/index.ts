// Node health & metrics types
export interface MemorySection {
  resident_bytes: number | null;
  virtual_bytes: number | null;
}

export interface PluginSection {
  hosts: number;
  loaded: number;
}

export interface SessionSection {
  total: number;
  active: number;
}

export interface RealtimeSection {
  websocket_connections: number;
  log_subscribers: number;
  event_subscribers: number;
}

export interface NodeHealth {
  status: string;
  version: string;
  uptime_seconds: number;
  llm_configured: boolean;
  memory: MemorySection;
  plugins: PluginSection;
  sessions: SessionSection;
  realtime: RealtimeSection;
}

// System configuration types
export interface WebhookConfig {
  platform: string;
  callback_configured: boolean;
  callback_url: string | null;
  signature_verification: boolean;
}

export interface LlmConfig {
  configured: boolean;
  protocol: string;
  model: string;
  base_url: string | null;
  api_key_configured: boolean;
  max_iterations: number;
  temperature: number | null;
  max_tokens: number | null;
}

export interface EnvironmentConfig {
  os: string;
  arch: string;
  rust_edition: string;
}

export interface SystemConfig {
  version: string;
  uptime_seconds: number;
  ipc_socket_path: string;
  run_dir: string;
  data_dir: string;
  memory_window: number;
  webhook: WebhookConfig;
  llm: LlmConfig;
  environment: EnvironmentConfig;
}

// Provider & Models types
export interface ActiveProviderInfo {
  configured: boolean;
  protocol: string;
  model: string;
  base_url: string | null;
  api_key_configured: boolean;
  temperature: number | null;
  max_tokens: number | null;
}

export interface ProtocolDescriptor {
  id: string;
  name: string;
  default_base_url: string;
}

export interface ProviderPreset {
  id: string;
  name: string;
  protocol: string;
  base_url: string;
}

export interface ProvidersCatalog {
  active: ActiveProviderInfo;
  available_protocols: ProtocolDescriptor[];
  presets: ProviderPreset[];
}

export interface TestProviderRequest {
  protocol?: string;
  base_url?: string;
  api_key?: string;
  model?: string;
  prompt?: string;
}

export interface TestProviderResponse {
  status: 'ok' | 'error';
  latency_ms: number;
  model: string;
  reply: string | null;
  error: string | null;
}

// Plugin & Supervisor types
export interface CommandDescriptor {
  name: string;
  description: string;
  usage?: string;
}

export interface ToolDescriptor {
  name: string;
  description: string;
  parameters?: Record<string, unknown>;
}

export interface PluginMeta {
  id: string;
  name: string;
  version: string;
  description?: string;
  status?: string;
  commands: CommandDescriptor[];
  tools: ToolDescriptor[];
}

export interface PluginHost {
  host_id: string;
  runtime?: string;
  pid?: number | null;
  status: string;
  plugins: PluginMeta[];
}

export interface PluginsResponse {
  total: number;
  hosts: PluginHost[];
  plugins: PluginMeta[];
}

export interface InstallPluginResponse {
  plugin_id: string;
  name: string;
  version: string;
  runtime: string;
  commands: CommandDescriptor[];
  tools: ToolDescriptor[];
  status: string;
  message?: string;
}

export interface PluginConfigResponse {
  plugin_id: string;
  cas_version: number;
  schema: Record<string, unknown>;
  config: Record<string, unknown>;
}

// Adapter types
export interface AdapterItem {
  platform: string;
  kind: 'builtin' | 'plugin';
  display_name: string;
  connected: boolean;
  host_id: string | null;
  plugin_id?: string;
}

export interface AdaptersResponse {
  total: number;
  adapters: AdapterItem[];
}

// Sessions & Personas
export interface SessionSummary {
  session_id: string;
  turn_count: number;
  total_tokens_used: number;
  active_persona?: string;
  last_updated_at?: number;
}

export interface SessionsResponse {
  total: number;
  sessions: SessionSummary[];
}

export interface PersonaItem {
  name: string;
  system_prompt: string;
}

export interface PersonasResponse {
  total: number;
  personas: PersonaItem[];
}

// WebSocket Logs
export type LogLevel = 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';

export interface LogRecord {
  seq?: number;
  timestamp_ms: number;
  level: LogLevel;
  target: string;
  message: string;
  fields?: Record<string, unknown>;
}

// WebSocket Pipeline & Agent Trace Events
export interface PipelineEventPayload {
  stage: string;
  event_id?: string;
  platform?: string;
  channel_id?: string;
  sender_id?: string;
  command?: string;
  plugin_id?: string;
  host_id?: string;
  session_id?: string;
  content_length?: number;
  segment_count?: number;
  message_id?: string;
  reason?: string;
  phase?: string;
  host_count?: number;
  tool_name?: string;
  error?: string;
  [key: string]: unknown;
}

export interface TraceRecord {
  seq: number;
  timestamp_ms: number;
  event: PipelineEventPayload;
}

// Chat Sandbox types
export interface ChatCompletionRequest {
  session_id: string;
  message: string;
  model?: string;
  persona?: string;
  persona_id?: string;
  tools?: boolean;
  protocol?: string;
  base_url?: string;
  api_key?: string;
}

export interface ExecutedTool {
  call_id: string;
  tool_name: string;
  plugin_id: string;
  host_id: string;
  success: boolean;
}

export interface ChatCompletionResponse {
  session_id: string;
  content: string;
  turns: number;
  finish_reason?: string;
  executed_tools: ExecutedTool[];
}

// Multi-provider & Model management (Two-tier provider/model hierarchy)
export interface CustomModel {
  id: string;
  name?: string;
}

export interface CustomProvider {
  id: string;
  name: string;
  protocol: 'openai' | 'openai_responses' | 'anthropic';
  base_url: string;
  api_key: string;
  models: CustomModel[];
}

export interface FetchModelsRequest {
  protocol?: string;
  base_url: string;
  api_key?: string;
}

export interface FetchModelsResponse {
  models: string[];
}
