"""Offline check of the ATEM parsers against vLLM's real parser plumbing.

No GPU and no server: it builds the same composed parser the OpenAI chat
endpoint builds (``ParserManager.get_parser``), feeds it recorded transcripts,
and asserts both the one-shot and the token-by-token streaming paths agree.

    ./run-muse --selftest          # runs this in the vLLM image

Run it after a vLLM bump: the streaming contract these parsers plug into
(vllm/parser/abstract_parser.py) is internal API and does move.
"""

import json
import sys

from transformers import AutoTokenizer

sys.path.insert(0, "/vllm-patches/atem-parsers")
import atem_parsers  # noqa: F401  (registers the parsers)

from vllm.entrypoints.openai.chat_completion.protocol import ChatCompletionRequest
from vllm.parser.parser_manager import ParserManager

MODEL = sys.argv[1] if len(sys.argv) > 1 else "/models/Muse-Glimmer-30B"

TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "weather.get_current",
            "description": "Current weather",
            "parameters": {
                "type": "object",
                "properties": {
                    "city": {"type": "string"},
                    "days": {"type": "integer"},
                    "units": {"type": "string"},
                    "tags": {"type": "array", "items": {"type": "string"}},
                },
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "shell",
            "description": "Run a command",
            "parameters": {
                "type": "object",
                "properties": {"cmd": {"type": "string"}},
            },
        },
    },
]

PLAIN = " to=user<|message|>391"

THOUGHT = " to=self<|message|>17*23 = 391. Keep it short.<|eom|>" \
          "<|start|>assistant to=user<|message|>391"

TOOL_CALL = (
    " to=self<|message|>I should look up the weather.<|eom|>"
    '<|start|>assistant to=weather.get_current<|message|><atem:function_calls>\n'
    '<atem:invoke name="weather.get_current">\n'
    '<atem:parameter name="city">Boston</atem:parameter>\n'
    '<atem:parameter name="days">3</atem:parameter>\n'
    '<atem:parameter name="tags">["a", "b"]</atem:parameter>\n'
    "</atem:invoke>\n</atem:function_calls>"
)

TWO_CALLS = (
    " to=self<|message|>Two calls.<|eom|>"
    '<|start|>assistant to=shell<|message|><atem:function_calls>\n'
    '<atem:invoke name="shell">\n'
    '<atem:parameter name="cmd">ls -la\n</atem:parameter>\n'
    "</atem:invoke>\n</atem:function_calls><|eom|>"
    '<|start|>assistant to=shell<|message|><atem:function_calls>\n'
    '<atem:invoke name="shell">\n'
    '<atem:parameter name="cmd">pwd</atem:parameter>\n'
    "</atem:invoke>\n</atem:function_calls>"
)

# What the response looks like if something upstream re-enables special-token
# stripping: the lenient fallback has to keep the answer out of `reasoning`.
STRIPPED = " to=selfShort answer coming.assistant to=user391"

CASES = [
    # name, raw model output, expected reasoning, content, [(fn, args)]
    ("plain", PLAIN, None, "391", []),
    ("thought", THOUGHT, "17*23 = 391. Keep it short.", "391", []),
    (
        "tool_call",
        TOOL_CALL,
        "I should look up the weather.",
        None,
        [("weather.get_current", {"city": "Boston", "days": 3, "tags": ["a", "b"]})],
    ),
    (
        "two_calls",
        TWO_CALLS,
        "Two calls.",
        None,
        [("shell", {"cmd": "ls -la"}), ("shell", {"cmd": "pwd"})],
    ),
    ("stripped_fallback", STRIPPED, "Short answer coming.", "391", []),
]

# Streaming can't run the lenient fallback incrementally -- with no markers it
# never knows a channel opened -- so the driver's end-of-stream hook promotes
# the answer as content and the chain of thought is dropped rather than leaked.
STREAM_OVERRIDES = {"stripped_fallback": ("", "391")}

failures: list[str] = []


def check(case: str, what: str, got, want) -> None:
    if got != want:
        failures.append(f"{case}: {what}\n     got  {got!r}\n     want {want!r}")


def main() -> int:
    tokenizer = AutoTokenizer.from_pretrained(MODEL, trust_remote_code=True)
    parser_cls = ParserManager.get_parser(
        tool_parser_name="atem",
        reasoning_parser_name="atem",
        enable_auto_tools=True,
        model_name=MODEL,
    )
    assert parser_cls is not None, "ParserManager returned no parser"

    request = ChatCompletionRequest(
        model=MODEL,
        messages=[{"role": "user", "content": "hi"}],
        tools=TOOLS,
        tool_choice="auto",
        include_reasoning=True,
    )
    # The generation prompt stops at a bare `<|start|>assistant`, so reasoning
    # must still be open when the first token arrives.
    prompt_ids = tokenizer.encode("<|start|>user<|message|>hi<|eot|><|start|>assistant")
    probe = parser_cls(tokenizer, request.tools)
    check(
        "prompt", "is_reasoning_end(prompt)", probe.is_reasoning_end(prompt_ids), False
    )

    for name, raw, want_reasoning, want_content, want_calls in CASES:
        # ---- one-shot ----------------------------------------------------
        parser = parser_cls(tokenizer, request.tools)
        reasoning, content, calls = parser.parse(raw, request, enable_auto_tools=True)
        check(name, "reasoning", reasoning, want_reasoning)
        check(name, "content", content, want_content)
        check(
            name,
            "tool_calls",
            [(c.name, json.loads(c.arguments)) for c in (calls or [])],
            want_calls,
        )

        # ---- streaming, one token at a time ------------------------------
        parser = parser_cls(tokenizer, request.tools)
        ids = tokenizer.encode(raw, add_special_tokens=False)
        streamed_reasoning, streamed_content = "", ""
        streamed_calls: list[dict] = []
        for i, token_id in enumerate(ids):
            delta = parser.parse_delta(
                tokenizer.decode([token_id]),
                [token_id],
                request,
                prompt_token_ids=prompt_ids if i == 0 else None,
                finished=i == len(ids) - 1,
            )
            if delta is None:
                continue
            streamed_reasoning += delta.reasoning or ""
            streamed_content += delta.content or ""
            for tc in delta.tool_calls or []:
                while tc.index >= len(streamed_calls):
                    streamed_calls.append({"name": "", "arguments": ""})
                fn = tc.function or {}
                fn = fn if isinstance(fn, dict) else fn.model_dump(exclude_none=True)
                streamed_calls[tc.index]["name"] += fn.get("name") or ""
                streamed_calls[tc.index]["arguments"] += fn.get("arguments") or ""

        want_stream = STREAM_OVERRIDES.get(
            name, (want_reasoning or "", want_content or "")
        )
        check(name, "stream reasoning", streamed_reasoning, want_stream[0])
        check(name, "stream content", streamed_content, want_stream[1])
        check(
            name,
            "stream tool_calls",
            [(c["name"], json.loads(c["arguments"])) for c in streamed_calls],
            want_calls,
        )

    if failures:
        print(f"\n{len(failures)} FAILURE(S):\n")
        for failure in failures:
            print("  " + failure)
        return 1
    print(f"ok: {len(CASES)} transcripts, one-shot and streaming")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
