/**
 * Demonstration TypeScript plugin for Kanon microkernel.
 */

import {
  Command,
  MessageSegment,
  Plugin,
  PluginContext,
  Tool,
} from "../../src/sdk/index.js";

export default class DemoTsPlugin extends Plugin {
  id = "org.kanon.plugin.demo_ts";
  name = "Demo TypeScript Plugin";
  version = "0.1.0";
  author = "Kanon Dev";
  description = "Demonstration plugin written in TypeScript";
  priority = 100;

  async onLoad(ctx: PluginContext): Promise<void> {
    console.log(`Demo TypeScript Plugin initialized with data directory: ${ctx.dataDir}`);
  }

  async onPreFilter(req: any): Promise<any> {
    if (req.raw_text && req.raw_text.includes("[block]")) {
      return {
        action: "BLOCK",
        modified_text: "",
        reply_messages: [
          MessageSegment.text("Message blocked by Demo TypeScript Plugin pre-filter"),
        ],
      };
    }
    return null;
  }

  @Command("tsgreet", {
    description: "TypeScript greeting command",
    usage: "/tsgreet <name>",
    priority: 100,
  })
  async handleGreet(req: any, args: string[]): Promise<any> {
    const target = args && args.length > 0 ? args.join(" ") : "World";
    return {
      success: true,
      replies: [
        MessageSegment.text(`Hello from Kanon TypeScript plugin, ${target}!`),
      ],
      error_message: "",
    };
  }

  @Tool({
    name: "ts_calc",
    description: "TypeScript mathematical calculation tool",
    parameters: {
      type: "object",
      properties: {
        expr: { type: "string", description: "Expression to evaluate" },
      },
    },
  })
  async handleTsCalc(params: any): Promise<any> {
    return {
      result: 42,
      summary: "Calculated via TypeScript plugin tool",
    };
  }
}
