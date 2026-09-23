/**
 * Kanon Official TypeScript SDK.
 *
 * Provides base classes, decorators, and context abstractions for authoring
 * out-of-process Kanon plugins in TypeScript or JavaScript.
 */

export interface PluginContext {
  /** Dedicated filesystem directory for this plugin's local persistent storage. */
  dataDir: string;
  /** Active configuration dictionary passed from the Core microkernel. */
  config: Record<string, any>;
}

export interface CommandMeta {
  name: string;
  description?: string;
  usage?: string;
  priority?: number;
}

export interface ToolMeta {
  name: string;
  description?: string;
  parameters?: Record<string, any>;
}

export interface PluginMeta {
  id: string;
  name: string;
  version: string;
  author?: string;
  description?: string;
  commands?: CommandMeta[];
  tools?: ToolMeta[];
}

export interface MessageSegmentItem {
  text?: { content: string };
  image?: {
    url?: string;
    file_path?: string;
    raw_bytes?: Buffer | Uint8Array;
    mime_type?: string;
    filename?: string;
  };
  audio?: {
    url?: string;
    file_path?: string;
    raw_bytes?: Buffer | Uint8Array;
    duration_seconds?: number;
  };
  mention?: {
    target_user_id: string;
    display_name?: string;
    is_all?: boolean;
  };
  reply?: {
    target_message_id: string;
    snippet?: string;
  };
  custom?: {
    type_name: string;
    payload?: Record<string, any>;
  };
}

export class MessageSegment {
  /** Creates a plain text message segment. */
  static text(content: string): MessageSegmentItem {
    return { text: { content } };
  }

  /** Creates an image message segment referencing a remote URL. */
  static imageUrl(
    url: string,
    mimeType?: string,
    filename?: string,
  ): MessageSegmentItem {
    return {
      image: {
        url,
        mime_type: mimeType,
        filename,
      },
    };
  }

  /** Creates an image message segment referencing a local physical file. */
  static imageFile(
    filePath: string,
    mimeType?: string,
    filename?: string,
  ): MessageSegmentItem {
    return {
      image: {
        file_path: filePath,
        mime_type: mimeType,
        filename,
      },
    };
  }
}

/** Decorator for declaring command handler methods. */
export function Command(
  name: string,
  options?: { description?: string; usage?: string; priority?: number },
): MethodDecorator {
  return (
    target: any,
    propertyKey: string | symbol,
    descriptor: PropertyDescriptor,
  ) => {
    target._kanon_commands = target._kanon_commands || [];
    target._kanon_commands.push({
      methodName: propertyKey,
      name,
      description: options?.description || "",
      usage: options?.usage || `/${name}`,
      priority: options?.priority || 500,
    });
  };
}

/** Decorator for declaring LLM tool calling methods. */
export function Tool(
  nameOrOptions:
    | string
    | { name: string; description?: string; parameters?: Record<string, any> },
): MethodDecorator {
  return (
    target: any,
    propertyKey: string | symbol,
    descriptor: PropertyDescriptor,
  ) => {
    target._kanon_tools = target._kanon_tools || [];
    const info =
      typeof nameOrOptions === "string"
        ? { name: nameOrOptions, description: "", parameters: undefined }
        : nameOrOptions;
    target._kanon_tools.push({
      methodName: propertyKey,
      name: info.name,
      description: info.description || "",
      parameters: info.parameters,
    });
  };
}

/** Base class for Kanon out-of-process TypeScript plugins. */
export abstract class Plugin {
  id: string = "org.kanon.plugin.base";
  name: string = "Base TS Plugin";
  version: string = "0.1.0";
  author: string = "Kanon Dev";
  description: string = "Default TypeScript plugin";
  priority: number = 500;

  context?: PluginContext;

  /** Returns static metadata describing this plugin's identity, commands, and tools. */
  meta(): PluginMeta {
    const proto = Object.getPrototypeOf(this);
    const declaredCommands: any[] = proto._kanon_commands || [];
    const declaredTools: any[] = proto._kanon_tools || [];

    const commands: CommandMeta[] = declaredCommands.map((c) => ({
      name: c.name,
      description: c.description,
      usage: c.usage,
      priority: c.priority,
    }));

    const tools: ToolMeta[] = declaredTools.map((t) => ({
      name: t.name,
      description: t.description,
      parameters: t.parameters ? (toProtoStruct(t.parameters) as any) : undefined,
    }));

    return {
      id: this.id,
      name: this.name,
      version: this.version,
      author: this.author,
      description: this.description,
      commands,
      tools,
    };
  }

  /** Lifecycle hook invoked when the plugin host loads the plugin. */
  async onLoad(ctx: PluginContext): Promise<void> {
    this.context = ctx;
  }

  /** Lifecycle hook invoked prior to plugin unload and host process shutdown. */
  async onUnload(): Promise<void> {}

  /**
   * Pre-filter interceptor hook invoked before command parsing and LLM dispatching.
   * Return null/undefined or { action: 'PASS' } to allow downstream flow.
   * Return { action: 'BLOCK', reply_messages: [...] } to block message processing.
   */
  async onPreFilter(req: any): Promise<any> {
    return null;
  }

  /** Executes a matched slash command. */
  async onExecuteCommand(req: any): Promise<any> {
    const proto = Object.getPrototypeOf(this);
    const declaredCommands: any[] = proto._kanon_commands || [];
    const entry = declaredCommands.find((c) => c.name === req.command);

    if (entry && typeof (this as any)[entry.methodName] === "function") {
      const res = await (this as any)[entry.methodName](req, req.args);
      if (typeof res === "string") {
        return {
          success: true,
          replies: [MessageSegment.text(res)],
          error_message: "",
        };
      } else if (Array.isArray(res)) {
        return {
          success: true,
          replies: res,
          error_message: "",
        };
      } else if (res && typeof res === "object") {
        return {
          success: res.success ?? true,
          replies: res.replies || [],
          error_message: res.error_message || "",
        };
      }
      return { success: true, replies: [], error_message: "" };
    }

    return {
      success: false,
      replies: [],
      error_message: `Unknown command: ${req.command}`,
    };
  }

  /** Executes an LLM tool call dispatched by the Core microkernel. */
  async onCallTool(req: any): Promise<any> {
    const proto = Object.getPrototypeOf(this);
    const declaredTools: any[] = proto._kanon_tools || [];
    const entry = declaredTools.find((t) => t.name === req.tool_name);

    if (entry && typeof (this as any)[entry.methodName] === "function") {
      const args = req.structured_args ? fromProtoStruct(req.structured_args) : {};
      const res = await (this as any)[entry.methodName](args);

      if (Buffer.isBuffer(res) || res instanceof Uint8Array) {
        return {
          call_id: req.call_id,
          success: true,
          error_message: "",
          raw_bytes: res,
        };
      } else if (res && typeof res === "object") {
        return {
          call_id: req.call_id,
          success: true,
          error_message: "",
          structured_result: toProtoStruct(res),
        };
      }
      return {
        call_id: req.call_id,
        success: true,
        error_message: "",
        structured_result: toProtoStruct({ result: String(res) }),
      };
    }

    return {
      call_id: req.call_id,
      success: false,
      error_message: `Unknown tool: ${req.tool_name}`,
    };
  }

  /** Processes an event notification broadcast from Core. */
  async onEvent(req: any): Promise<void> {}

  /** Delivers an outbound message to a target platform. */
  async onDeliverMessage(req: any): Promise<any> {
    return {
      success: true,
      message_id: "delivered_ts_1",
      error_message: "",
    };
  }
}

/** Converts a JS primitive/object to a Protobuf Value descriptor. */
export function toProtoValue(val: any): any {
  if (val === null || val === undefined) {
    return { nullValue: 0 };
  } else if (typeof val === "number") {
    return { numberValue: val };
  } else if (typeof val === "string") {
    return { stringValue: val };
  } else if (typeof val === "boolean") {
    return { boolValue: val };
  } else if (Array.isArray(val)) {
    return { listValue: { values: val.map(toProtoValue) } };
  } else if (typeof val === "object") {
    return { structValue: toProtoStruct(val) };
  }
  return { stringValue: String(val) };
}

/** Converts a standard JS object dictionary into a google.protobuf.Struct payload. */
export function toProtoStruct(obj: Record<string, any>): {
  fields: Record<string, any>;
} {
  const fields: Record<string, any> = {};
  if (obj && typeof obj === "object") {
    for (const [k, v] of Object.entries(obj)) {
      fields[k] = toProtoValue(v);
    }
  }
  return { fields };
}

/** Converts a Protobuf Value descriptor back to a standard JS value. */
export function fromProtoValue(val: any): any {
  if (!val) return null;
  if ("numberValue" in val) return val.numberValue;
  if ("stringValue" in val) return val.stringValue;
  if ("boolValue" in val) return val.boolValue;
  if ("nullValue" in val) return null;
  if ("listValue" in val) return (val.listValue?.values || []).map(fromProtoValue);
  if ("structValue" in val) return fromProtoStruct(val.structValue);
  return null;
}

/** Converts a google.protobuf.Struct payload back into a standard JS object dictionary. */
export function fromProtoStruct(structObj: any): Record<string, any> {
  const res: Record<string, any> = {};
  if (structObj && structObj.fields) {
    for (const [k, v] of Object.entries(structObj.fields)) {
      res[k] = fromProtoValue(v);
    }
  }
  return res;
}

