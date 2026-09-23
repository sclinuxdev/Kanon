/**
 * Kanon Official TypeScript / Node.js Host Process.
 *
 * Spawns a gRPC server hosting TypeScript plugins, providing PluginHostService
 * and MessagePipelineService over an IPC socket (Unix Domain Socket or Windows Loopback TCP).
 */

import * as fs from "fs";
import * as path from "path";
import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";
import { Plugin, PluginContext, PluginMeta } from "../sdk/index.js";

/** Locates the canonical proto IDL file across workspaces. */
function findProtoPath(): string {
  if (process.env.KANON_PROTO_PATH && fs.existsSync(process.env.KANON_PROTO_PATH)) {
    return process.env.KANON_PROTO_PATH;
  }

  let current = __dirname;
  for (let i = 0; i < 6; i++) {
    const candidate = path.join(current, "proto/kanon/v1/plugin.proto");
    if (fs.existsSync(candidate)) {
      return candidate;
    }
    current = path.dirname(current);
  }

  current = process.cwd();
  for (let i = 0; i < 6; i++) {
    const candidate = path.join(current, "proto/kanon/v1/plugin.proto");
    if (fs.existsSync(candidate)) {
      return candidate;
    }
    current = path.dirname(current);
  }

  throw new Error("Cannot locate proto/kanon/v1/plugin.proto");
}

/** Parses entrypoint from plugin.toml or direct script path. */
function resolvePluginEntrypoint(targetPath: string): string {
  const resolved = path.resolve(targetPath);
  if (fs.statSync(resolved).isDirectory()) {
    const tomlPath = path.join(resolved, "plugin.toml");
    if (fs.existsSync(tomlPath)) {
      return resolvePluginEntrypoint(tomlPath);
    }
    return path.join(resolved, "index.js");
  }

  if (resolved.endsWith(".toml")) {
    const content = fs.readFileSync(resolved, "utf-8");
    const entryMatch = content.match(/entrypoint\s*=\s*"([^"]+)"/);
    const entrypoint = entryMatch ? entryMatch[1] : "index.js";
    const dir = path.dirname(resolved);
    const candidate = path.join(dir, entrypoint);
    // If typescript source file was referenced (.ts), try dist equivalent or require ts directly
    if (candidate.endsWith(".ts")) {
      const jsCandidate = candidate.replace(/\.ts$/, ".js");
      if (fs.existsSync(jsCandidate)) {
        return jsCandidate;
      }
      // If built under dist/
      const distCandidate = path.join(dir, "../../dist/plugins", path.basename(dir), "index.js");
      if (fs.existsSync(distCandidate)) {
        return distCandidate;
      }
    }
    return candidate;
  }

  return resolved;
}

/** Loads and instantiates the Plugin instance. */
async function loadPlugin(targetPath: string): Promise<Plugin> {
  const entrypoint = resolvePluginEntrypoint(targetPath);
  if (!fs.existsSync(entrypoint)) {
    throw new Error(`Plugin entrypoint not found: ${entrypoint}`);
  }

  const imported = await import(`file://${entrypoint}`);
  let target: any = imported.default || imported;
  while (target && target.default && typeof target !== "function") {
    target = target.default;
  }

  if (typeof target === "function") {
    return new target();
  }
  if (target && typeof target.meta === "function") {
    return target;
  }

  for (const val of Object.values(imported)) {
    let candidate: any = val;
    while (candidate && candidate.default && typeof candidate !== "function") {
      candidate = candidate.default;
    }
    if (typeof candidate === "function") {
      try {
        const inst = new candidate();
        if (inst instanceof Plugin || typeof inst.meta === "function") {
          return inst;
        }
      } catch (_) {}
    }
  }

  throw new Error(`Invalid plugin export in ${entrypoint}`);
}

async function main(): Promise<void> {
  // 1. Parse arguments and environment
  const args = process.argv.slice(2);
  let pluginArg: string | undefined;
  let socketArg: string | undefined;

  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--plugin" && i + 1 < args.length) {
      pluginArg = args[++i];
    } else if (args[i] === "--socket" && i + 1 < args.length) {
      socketArg = args[++i];
    }
  }

  const pluginPath =
    pluginArg ||
    process.env.KANON_PLUGIN_MANIFEST ||
    process.env.KANON_PLUGIN_ENTRYPOINT;

  if (!pluginPath) {
    console.error("Error: No plugin specified via --plugin or KANON_PLUGIN_MANIFEST");
    process.exit(1);
  }

  const socketPath = path.resolve(
    socketArg || process.env.KANON_HOST_SOCK || "./run/host_ts.sock",
  );
  const coreSockPath = process.env.KANON_CORE_SOCK;
  const hostId = process.env.KANON_HOST_ID || "host_ts";

  // 2. Load and initialize plugin
  const plugin = await loadPlugin(pluginPath);
  const meta = plugin.meta();

  const dataDir = path.resolve(`./data/plugins/${meta.id}`);
  fs.mkdirSync(dataDir, { recursive: true });
  const ctx: PluginContext = {
    dataDir,
    config: {},
  };
  await plugin.onLoad(ctx);

  // 3. Prepare IPC socket directory and clean up stale socket
  fs.mkdirSync(path.dirname(socketPath), { recursive: true });
  if (fs.existsSync(socketPath)) {
    try {
      fs.unlinkSync(socketPath);
    } catch (_) {}
  }

  // 4. Load gRPC IDL definitions
  const protoFile = findProtoPath();
  const packageDefinition = protoLoader.loadSync(protoFile, {
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
    oneofs: true,
    includeDirs: [path.dirname(path.dirname(path.dirname(protoFile)))],
  });

  const protoDescriptor = grpc.loadPackageDefinition(packageDefinition);
  const kanonV1 = (protoDescriptor as any).kanon.plugin.v1;

  // 5. Initialize gRPC server and register services
  const server = new grpc.Server();

  server.addService(kanonV1.PluginHostService.service, {
    Ping: (call: any, callback: any) => {
      callback(null, { timestamp: call.request.timestamp });
    },
    ReloadPluginConfig: (call: any, callback: any) => {
      callback(null, { success: true, error_message: "" });
    },
    GetPluginMeta: (call: any, callback: any) => {
      callback(null, { plugins: [plugin.meta()] });
    },
  });

  server.addService(kanonV1.MessagePipelineService.service, {
    OnPreFilter: async (call: any, callback: any) => {
      try {
        const res = await plugin.onPreFilter(call.request);
        if (!res) {
          callback(null, {
            action: "PASS",
            modified_text: "",
            reply_messages: [],
          });
        } else {
          callback(null, res);
        }
      } catch (err: any) {
        callback({
          code: grpc.status.INTERNAL,
          message: err?.message || "PreFilter error",
        });
      }
    },
    OnExecuteCommand: async (call: any, callback: any) => {
      try {
        const res = await plugin.onExecuteCommand(call.request);
        callback(null, res);
      } catch (err: any) {
        callback(null, {
          success: false,
          replies: [],
          error_message: err?.message || "Command error",
        });
      }
    },
    OnCallTool: async (call: any, callback: any) => {
      try {
        const res = await plugin.onCallTool(call.request);
        callback(null, res);
      } catch (err: any) {
        callback(null, {
          call_id: call.request.call_id,
          success: false,
          error_message: err?.message || "Tool error",
        });
      }
    },
    OnEvent: async (call: any, callback: any) => {
      try {
        await plugin.onEvent(call.request);
        callback(null, { received: true });
      } catch (err: any) {
        callback(null, { received: false });
      }
    },
    OnDeliverMessage: async (call: any, callback: any) => {
      try {
        const res = await plugin.onDeliverMessage(call.request);
        callback(null, res);
      } catch (err: any) {
        callback(null, {
          success: false,
          message_id: "",
          error_message: err?.message || "Deliver error",
        });
      }
    },
  });

  // 6. Bind to IPC endpoint
  const bindAddress = `unix:${socketPath}`;
  server.bindAsync(
    bindAddress,
    grpc.ServerCredentials.createInsecure(),
    (err: Error | null, port: number) => {
      if (err) {
        console.error(`Failed to bind socket ${bindAddress}:`, err);
        process.exit(1);
      }
      console.log(`Kanon TypeScript Host running on ${socketPath}`);
    },
  );

  // 7. Handle graceful shutdown
  const shutdown = async () => {
    try {
      await plugin.onUnload();
    } catch (_) {}

    server.tryShutdown(() => {
      if (fs.existsSync(socketPath)) {
        try {
          fs.unlinkSync(socketPath);
        } catch (_) {}
      }
      process.exit(0);
    });
  };

  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

main().catch((err) => {
  console.error("Fatal error in Kanon TypeScript Host:", err);
  process.exit(1);
});
