// Node health & metrics types
export interface NodeHealth {
  status: string;
  version: string;
  uptime_secs: number;
  chat_enabled: boolean;
  default_model?: string;
  supervisor: {
    managed_hosts: number;
    total_plugins: number;
    adapters: number;
  };
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
  commands: CommandDescriptor[];
  tools: ToolDescriptor[];
}

export interface PluginHost {
  host_id: string;
  runtime: string;
  pid: number;
  status: string;
  plugins: PluginMeta[];
}

export interface PluginsResponse {
  total: number;
  hosts: PluginHost[];
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
  seq: number;
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
  tools?: boolean;
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
