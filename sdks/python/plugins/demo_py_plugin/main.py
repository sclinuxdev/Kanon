"""Demonstration Python plugin for Kanon microkernel."""

from typing import Any, Dict, List, Optional

from kanon_sdk import MessageSegment, Plugin, PluginContext, command, tool
from kanon_sdk.proto import pb


class DemoPythonPlugin(Plugin):
    """Demonstration plugin implemented in Python."""

    id = "org.kanon.plugin.demo_py"
    name = "Demo Python Plugin"
    version = "0.1.0"
    author = "Kanon Dev"
    description = "Demonstration plugin written in Python"
    priority = 100

    async def on_load(self, ctx: PluginContext) -> None:
        print(f"Demo Python Plugin initialized with data directory: {ctx.data_dir}", flush=True)

    async def on_pre_filter(
        self,
        req: pb.PipelineEventRequest,
    ) -> Optional[pb.PreFilterResult]:
        if "[block]" in req.raw_text:
            return pb.PreFilterResult(
                action=pb.PreFilterResult.Action.BLOCK,
                modified_text="",
                reply_messages=[
                    MessageSegment.text("Message blocked by Demo Python Plugin pre-filter")
                ],
            )
        return None

    @command(
        name="pycalc",
        description="Python-based calculation command",
        usage="/pycalc <expr>",
        priority=100,
    )
    async def handle_pycalc(
        self,
        req: pb.CommandExecuteRequest,
        args: List[str],
    ) -> pb.CommandExecuteResponse:
        expr = " ".join(args)
        reply = MessageSegment.text(
            f"Python calculation result for [{expr}]: 42 (fast-path)"
        )
        return pb.CommandExecuteResponse(
            success=True,
            replies=[reply],
            error_message="",
        )

    @tool(
        name="py_calc",
        description="Python mathematical calculation tool",
        parameters={
            "type": "object",
            "properties": {
                "expr": {"type": "string", "description": "Expression to evaluate"},
            },
        },
    )
    async def handle_py_calc(self, params: Dict[str, Any]) -> Dict[str, Any]:
        return {
            "result": 42.0,
            "summary": "Calculated via Python plugin tool",
        }
