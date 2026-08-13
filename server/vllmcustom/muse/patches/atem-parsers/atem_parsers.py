"""ATEM channel + tool-call parsers for Muse Glimmer on vLLM.

vLLM has no native MuseGlimmerForConditionalGeneration and no parser for the
Harmony-style ATEM transcript format the model emits, so `/v1/chat/completions`
hands the whole assistant turn back as `content` -- chain of thought, routing
tokens and all -- and any request with `tool_choice: "auto"` is rejected with

    "auto" tool choice requires --enable-auto-tool-choice and
    --tool-call-parser to be set

This module supplies both missing pieces, registered under the name `atem`:

    ATEMReasoningParser  -> splits the transcript into channels
    ATEMToolParser       -> turns <atem:invoke> blocks into OpenAI tool_calls

Load it with (see ../run, which does this for you):

    --reasoning-parser atem  --reasoning-parser-plugin  <this file>
    --tool-call-parser atem  --tool-parser-plugin       <this file>
    --enable-auto-tool-choice

---------------------------------------------------------------------------
THE WIRE FORMAT
---------------------------------------------------------------------------
The chat template ends the generation prompt at a bare `<|start|>assistant`,
so the model itself emits the routing header. A full turn looks like:

     to=self<|message|>...chain of thought...<|eom|>
    <|start|>assistant to=user<|message|>...the answer...<|eot|>

and a tool call replaces the last channel with (chat_template.jinja render_atem)

    <|start|>assistant to=weather.get_current<|message|>
    <atem:function_calls>
    <atem:invoke name="weather.get_current">
    <atem:parameter name="city">Boston</atem:parameter>
    <atem:parameter name="days">3</atem:parameter>
    </atem:invoke>
    </atem:function_calls><|eot|>

So: every message is `[<|start|>assistant] to=<recipient><|message|>BODY<|eom|>`,
`to=self` is the reasoning channel, `to=user` is the visible answer, and any
other recipient is a tool call whose function name is repeated inside the
`<atem:invoke name=...>` tag (this parser reads the name from the tag, so the
header recipient is only used to route reasoning vs. everything else).

`<|eot|>` (200008) and `<|end_of_text|>` (200001) are EOS so they are consumed
by the engine; `<|eom|>` (200007) is NOT -- it only ends a message -- so it
shows up in the text and has to be stripped here.

---------------------------------------------------------------------------
WHY skip_special_tokens IS FORCED OFF
---------------------------------------------------------------------------
`<|start|>`, `<|message|>` and `<|eom|>` are special tokens, and vLLM strips
special tokens from the response text by default. That would leave

     to=selfWhat is 17*23...assistant to=user391

i.e. no reliable channel boundary at all. `ATEMReasoningParser.adjust_request`
therefore sets `skip_special_tokens = False` on every chat request (the
reasoning parser's `adjust_request` is called for every request, tools or not
-- see vllm/renderers/online_renderer.py, `should_adjust_request`). Jamba,
Hermes and Kimi K3 do exactly the same thing for the same reason. Everything
these parsers hand back to the client is stripped of the markers again, so the
raw tokens never reach the API caller.

There is still a lenient text-only fallback in `extract_reasoning` for the case
where something upstream re-strips the tokens; it splits on the bare `to=`
routing words. It is a safety net, not the normal path.

---------------------------------------------------------------------------
PARAMETER TYPING
---------------------------------------------------------------------------
ATEM parameters are XML-ish text, not JSON, and the template writes scalars
raw (`<atem:parameter name="days">3</atem:parameter>`) while lists/objects go
through `tojson`. Coming back the other way the type has to be recovered, so
values are cast using the tool's own JSON Schema when the parameter is
declared there, and by "try json.loads, else keep the string" when it is not.
One leading and one trailing newline are stripped from string values, because
the tool-use preamble's own example writes multi-line values as
`>value\n</atem:parameter>` while `render_atem` writes them tight.

Delete this plugin once vLLM ships a native Muse Glimmer implementation with
its own ATEM parsers.
"""

from __future__ import annotations

import json
import re
from collections.abc import Sequence
from typing import Any

from vllm.entrypoints.chat_utils import make_tool_call_id
from vllm.entrypoints.openai.chat_completion.protocol import ChatCompletionRequest
from vllm.entrypoints.openai.engine.protocol import (
    DeltaFunctionCall,
    DeltaMessage,
    DeltaToolCall,
    ExtractedToolCallInformation,
    FunctionCall,
    ToolCall,
)
from vllm.entrypoints.openai.responses.protocol import ResponsesRequest
from vllm.logger import init_logger
from vllm.reasoning.abs_reasoning_parsers import ReasoningParser, ReasoningParserManager
from vllm.tokenizers import TokenizerLike
from vllm.tool_parsers.abstract_tool_parser import Tool, ToolParser, ToolParserManager

logger = init_logger(__name__)

AnyRequest = ChatCompletionRequest | ResponsesRequest

# --------------------------------------------------------------------------
# transcript markers
# --------------------------------------------------------------------------
START = "<|start|>"
MESSAGE = "<|message|>"
EOM = "<|eom|>"
EOT = "<|eot|>"
EOD = "<|end_of_text|>"

# `[<|start|>][assistant] [to=<recipient>]<|message|>`. Every part except
# <|message|> is optional: the first channel of a turn has its <|start|>assistant
# in the prompt, and a recipient-less header means the user channel.
HEADER_RE = re.compile(
    r"(?:<\|start\|>)?[ \t]*(?:assistant)?[ \t]*(?:to=(?P<to>[^\s<]+))?[ \t]*<\|message\|>"
)
# A message body runs until one of these; none of them can occur inside a body.
BODY_END_RE = re.compile(r"<\|eom\|>|<\|eot\|>|<\|start\|>|<\|end_of_text\|>")
# Safety net for output that reached us with special tokens already stripped.
# With <|message|> gone the recipient runs straight into the body ("to=selfI
# should..."), so only the two recipients whose names are known up front are
# accepted -- plus any recipient that is followed immediately by ATEM markup,
# which is ordinary text and survives stripping.
LENIENT_HEADER_RE = re.compile(
    r"(?:^|(?<=[\s>]))(?:assistant[ \t]*)?to="
    r"(?P<to>self|user|[A-Za-z0-9_.\-]+(?=<atem:))"
)
# ... and the role word of the *next* header is left stranded at the end of the
# body it follows.
LENIENT_TRAILER_RE = re.compile(r"\s*assistant[ \t]*$")

SELF_RECIPIENT = "self"

# Suffixes worth holding back mid-stream because they may be the first
# characters of a marker we must not leak.
PARTIAL_MARKERS = (
    START,
    MESSAGE,
    EOM,
    EOT,
    EOD,
    "<atem:function_calls>",
    "<atem:invoke",
)

# --------------------------------------------------------------------------
# ATEM tool-call markup
# --------------------------------------------------------------------------
CALLS_OPEN = "<atem:function_calls>"
INVOKE_OPEN_RE = re.compile(r"<atem:invoke\s+name\s*=\s*\"(?P<name>[^\"]*)\"\s*>")
INVOKE_CLOSE = "</atem:invoke>"
PARAM_OPEN_RE = re.compile(r"<atem:parameter\s+name\s*=\s*\"(?P<name>[^\"]*)\"\s*>")
PARAM_CLOSE = "</atem:parameter>"


def _partial_overlap(text: str, marker: str) -> int:
    """Length of the longest prefix of *marker* that is a suffix of *text*."""
    for k in range(min(len(marker) - 1, len(text)), 0, -1):
        if text.endswith(marker[:k]):
            return k
    return 0


def _hold_back(text: str) -> str:
    """Drop a trailing fragment of a marker that has not fully arrived yet.

    Only exact marker prefixes are held back, so ordinary prose containing a
    stray ``<`` streams through untouched.
    """
    keep = max((_partial_overlap(text, m) for m in PARTIAL_MARKERS), default=0)
    return text[: len(text) - keep] if keep else text


def split_channels(text: str, *, streaming: bool = False) -> tuple[str, str, bool]:
    """Split an ATEM transcript into ``(reasoning, content, reasoning_ended)``.

    ``reasoning`` is every ``to=self`` body, ``content`` is every other body
    (the visible answer, or the ``<atem:function_calls>`` markup for a tool
    call). ``reasoning_ended`` is True once a non-self channel has been opened.

    Text outside a body -- the routing headers, ``<|eom|>``, the ``assistant``
    role word -- is scaffolding and is dropped. With ``streaming=True`` the
    tail of an unterminated body is additionally held back while it could
    still turn out to be a partial marker.

    A transcript with no ``<|message|>`` at all is a turn that has only emitted
    its header so far, so everything is scaffolding and nothing is returned.
    """
    headers = list(HEADER_RE.finditer(text))
    if not headers:
        return "", "", False

    reasoning: list[str] = []
    content: list[str] = []
    ended = False
    # Text before the first header is either nothing (the usual case -- the
    # header matches at offset 0) or a body whose header has already been
    # consumed. The latter is what the tool parser sees when it re-cleans its
    # accumulated text after the driver handed it a stripped first chunk, so
    # keep it as content. It deliberately does not flip `ended`: that decision
    # belongs to the token-level check, which only counts real headers.
    prefix = text[: headers[0].start()]
    prefix_end = BODY_END_RE.search(prefix)
    if prefix_end is not None:
        prefix = prefix[: prefix_end.start()]
    if prefix.strip():
        content.append(prefix)
    for i, header in enumerate(headers):
        limit = headers[i + 1].start() if i + 1 < len(headers) else len(text)
        body = text[header.end() : limit]
        terminator = BODY_END_RE.search(body)
        if terminator is not None:
            body = body[: terminator.start()]
        elif streaming and i == len(headers) - 1:
            body = _hold_back(body)

        if (header.group("to") or "user") == SELF_RECIPIENT:
            reasoning.append(body)
        else:
            content.append(body)
            ended = True
    return "".join(reasoning), "".join(content), ended


def split_channels_lenient(text: str) -> tuple[str, str, bool]:
    """``split_channels`` for output whose special tokens were already stripped.

    Splits on the bare ``to=<recipient>`` routing words that survive stripping.
    Only used when no ``<|message|>`` marker is present anywhere; see the module
    docstring on why that should not happen.
    """
    headers = list(LENIENT_HEADER_RE.finditer(text))
    if not headers:
        return "", text, True

    reasoning: list[str] = []
    content: list[str] = []
    ended = False
    prefix = text[: headers[0].start()]
    if prefix.strip():
        content.append(prefix)
        ended = True
    for i, header in enumerate(headers):
        limit = headers[i + 1].start() if i + 1 < len(headers) else len(text)
        body = LENIENT_TRAILER_RE.sub("", text[header.end() : limit])
        if header.group("to") == SELF_RECIPIENT:
            reasoning.append(body)
        else:
            content.append(body)
            ended = True
    return "".join(reasoning), "".join(content), ended


def content_of(text: str) -> str:
    """Strip channel scaffolding from *text*, keeping only non-reasoning bodies.

    Idempotent on text that has already been split out by the reasoning parser,
    which is what makes it safe for the tool parser to apply unconditionally:
    once the scaffolding is gone there are no headers left to match and the
    text is returned as-is.
    """
    if MESSAGE not in text:
        return text
    _, content, _ = split_channels(text, streaming=True)
    return content


class ATEMReasoningParser(ReasoningParser):
    """Routes Muse Glimmer's `to=self` channel to `reasoning`, the rest to `content`."""

    def __init__(self, tokenizer: TokenizerLike, *args, **kwargs):
        super().__init__(tokenizer, *args, **kwargs)
        if tokenizer is None:
            raise ValueError("ATEM reasoning parser requires a tokenizer.")

        def one(text: str) -> int:
            ids = tokenizer.encode(text, add_special_tokens=False)
            if len(ids) != 1:
                raise ValueError(
                    f"ATEM reasoning parser expected {text!r} to be a single token "
                    f"for this model, got {ids}. Wrong model for --reasoning-parser atem?"
                )
            return ids[0]

        self.start_id = one(START)
        self.message_id = one(MESSAGE)
        # " to=self" tokenizes as [' to', '=self']; the '=self' token sitting
        # directly in front of <|message|> is what marks the reasoning channel.
        self.self_id = tokenizer.encode(" to=self", add_special_tokens=False)[-1]

    # -- request shaping ---------------------------------------------------

    def adjust_request(self, request: AnyRequest) -> AnyRequest:
        # Unconditional: the channel markers are special tokens, and without
        # them there is no reliable boundary between reasoning and answer.
        request.skip_special_tokens = False
        if hasattr(request, "spaces_between_special_tokens"):
            # Keep detokenization byte-exact around the markers.
            request.spaces_between_special_tokens = False
        return request

    # -- token-level channel tracking --------------------------------------
    #
    # These run on token ids, not text: the streaming driver and the structured
    # output engine use them to decide when the reasoning phase is over.

    def _region(self, input_ids: Sequence[int]) -> Sequence[int]:
        """The tokens after the last `<|start|>`, i.e. the newest message.

        Scoping to the newest message is what keeps multi-turn prompts honest:
        a prompt is full of `<|message|>` tokens from previous turns, but it
        ends at the bare `<|start|>assistant` generation prefix, so the region
        is empty and reasoning is correctly reported as still open.
        """
        for i in range(len(input_ids) - 1, -1, -1):
            if input_ids[i] == self.start_id:
                return input_ids[i + 1 :]
        return input_ids

    def is_reasoning_end(self, input_ids: Sequence[int]) -> bool:
        region = self._region(input_ids)
        for i, token_id in enumerate(region):
            if token_id == self.message_id:
                if i == 0 or region[i - 1] != self.self_id:
                    return True
        return False

    def extract_content_ids(self, input_ids: list[int]) -> list[int]:
        """The tokens after the last `<|message|>` -- i.e. the current body."""
        for i in range(len(input_ids) - 1, -1, -1):
            if input_ids[i] == self.message_id:
                return input_ids[i + 1 :]
        return input_ids

    def count_reasoning_tokens(self, token_ids: Sequence[int]) -> int:
        """Count tokens inside `to=self` bodies, for the usage report."""
        count = 0
        in_reasoning = False
        for i, token_id in enumerate(token_ids):
            if token_id == self.message_id:
                in_reasoning = i > 0 and token_ids[i - 1] == self.self_id
                continue
            if token_id in (self.start_id,):
                in_reasoning = False
                continue
            if in_reasoning:
                count += 1
        return count

    # -- text extraction ---------------------------------------------------

    def extract_reasoning(
        self, model_output: str, request: AnyRequest
    ) -> tuple[str | None, str | None]:
        if MESSAGE in model_output:
            reasoning, content, _ = split_channels(model_output)
        else:
            # No channel markers at all: either the turn never opened a message
            # (nothing to say) or special tokens were stripped upstream.
            reasoning, content, _ = split_channels_lenient(model_output)
        return reasoning or None, content or None

    def extract_reasoning_streaming(
        self,
        previous_text: str,
        current_text: str,
        delta_text: str,
        previous_token_ids: Sequence[int],
        current_token_ids: Sequence[int],
        delta_token_ids: Sequence[int],
    ) -> DeltaMessage | None:
        previous_reasoning, _, _ = split_channels(previous_text, streaming=True)
        reasoning, content, ended = split_channels(current_text, streaming=True)

        if reasoning.startswith(previous_reasoning):
            delta_reasoning = reasoning[len(previous_reasoning) :]
        else:  # pragma: no cover - only if the model rewrites history
            delta_reasoning = reasoning

        if ended:
            # The driver takes `content` as the whole text handed to the tool
            # parser and restarts its bookkeeping from there, so this must be
            # everything accumulated on the non-reasoning channels so far.
            return DeltaMessage(reasoning=delta_reasoning or None, content=content or None)
        return DeltaMessage(reasoning=delta_reasoning) if delta_reasoning else None

    def get_streaming_fallback_content(self, previous_text: str, request: AnyRequest) -> str:
        """Last-ditch content recovery for a stream that never left reasoning.

        Called by the driver when generation finishes with the reasoning phase
        still open: either the turn only ever spoke on `to=self`, or the
        markers were stripped and `split_channels` had nothing to match. Better
        to hand the caller the answer late than to return an empty message.
        """
        if MESSAGE in previous_text:
            _, content, _ = split_channels(previous_text)
        else:
            _, content, _ = split_channels_lenient(previous_text)
        return content

    # Aliases kept for parity with the in-tree parsers.
    extract_reasoning_content = extract_reasoning
    extract_reasoning_content_streaming = extract_reasoning_streaming


class ATEMToolParser(ToolParser):
    """Turns `<atem:invoke>` blocks into OpenAI tool calls."""

    # ATEM is XML-ish, so vLLM's JSON-schema-guided path for
    # tool_choice="required" / named functions cannot be used: it would force
    # the model to emit bare JSON, which the chat format has no channel for.
    # False routes those choices through extract_tool_calls, same as "auto".
    supports_required_and_named = False

    def __init__(self, tokenizer: TokenizerLike, tools: list[Tool] | None = None):
        super().__init__(tokenizer, tools)
        self._sent_content_idx = 0
        self._streamed_names: list[str] = []

    def adjust_request(self, request: AnyRequest) -> AnyRequest:
        # Deliberately does NOT call super(): the base implementation installs a
        # JSON structured-output schema for required/named tool choice, which is
        # wrong for this format (see supports_required_and_named above).
        # The reasoning parser already turns off special-token stripping.
        request.skip_special_tokens = False
        return request

    # -- schema-aware value typing ----------------------------------------

    def _param_types(self, name: str, request: AnyRequest | None) -> dict[str, str]:
        """`{parameter: json-schema type}` for the tool called *name*."""
        tools = getattr(request, "tools", None) or self.tools or []
        for tool in tools:
            function = getattr(tool, "function", tool)
            if getattr(function, "name", None) != name:
                continue
            parameters = getattr(function, "parameters", None) or {}
            properties = parameters.get("properties") or {}
            types = {}
            for key, schema in properties.items():
                declared = schema.get("type") if isinstance(schema, dict) else None
                if isinstance(declared, list):  # e.g. ["string", "null"]
                    declared = next((t for t in declared if t != "null"), None)
                if isinstance(declared, str):
                    types[key] = declared
            return types
        return {}

    @staticmethod
    def _coerce(raw: str, declared: str | None) -> Any:
        # render_atem writes values tight, but the tool-use preamble's example
        # shows a trailing newline before the closing tag; accept both.
        value = raw
        if value.startswith("\n"):
            value = value[1:]
        if value.endswith("\n"):
            value = value[:-1]

        if declared == "string":
            return value
        try:
            return json.loads(value.strip())
        except (ValueError, TypeError):
            # Unknown or unparseable: a bare word is just a string.
            return value

    def _arguments(
        self,
        name: str,
        params: list[tuple[str, str, bool]],
        request: AnyRequest | None,
    ) -> dict[str, Any]:
        types = self._param_types(name, request)
        return {key: self._coerce(raw, types.get(key)) for key, raw, _ in params}

    # -- markup parsing ----------------------------------------------------

    @staticmethod
    def _parse_params(body: str) -> list[tuple[str, str, bool]]:
        """`[(name, raw_value, closed)]` for one `<atem:invoke>` body."""
        params: list[tuple[str, str, bool]] = []
        pos = 0
        while (opening := PARAM_OPEN_RE.search(body, pos)) is not None:
            value_start = opening.end()
            closing = body.find(PARAM_CLOSE, value_start)
            if closing == -1:
                params.append((opening.group("name"), body[value_start:], False))
                break
            params.append((opening.group("name"), body[value_start:closing], True))
            pos = closing + len(PARAM_CLOSE)
        return params

    @classmethod
    def _parse_invokes(cls, text: str) -> list[tuple[str, list[tuple[str, str, bool]], bool]]:
        """`[(name, params, closed)]` for every `<atem:invoke>` in *text*.

        Invokes are collected wherever they appear rather than only inside a
        `<atem:function_calls>` wrapper: the wrapper carries no information the
        invoke does not, and the model occasionally emits one call per message
        with its own wrapper.
        """
        invokes = []
        pos = 0
        while (opening := INVOKE_OPEN_RE.search(text, pos)) is not None:
            body_start = opening.end()
            closing = text.find(INVOKE_CLOSE, body_start)
            if closing == -1:
                invokes.append(
                    (opening.group("name"), cls._parse_params(text[body_start:]), False)
                )
                break
            invokes.append(
                (
                    opening.group("name"),
                    cls._parse_params(text[body_start:closing]),
                    True,
                )
            )
            pos = closing + len(INVOKE_CLOSE)
        return invokes

    @staticmethod
    def _content_end(text: str) -> int:
        """Index at which the tool-call markup starts, or len(text)."""
        candidates = [text.find(CALLS_OPEN)]
        invoke = INVOKE_OPEN_RE.search(text)
        candidates.append(invoke.start() if invoke is not None else -1)
        found = [c for c in candidates if c != -1]
        return min(found) if found else len(text)

    # -- non-streaming -----------------------------------------------------

    def extract_tool_calls(
        self, model_output: str, request: ChatCompletionRequest
    ) -> ExtractedToolCallInformation:
        text = content_of(model_output)
        try:
            invokes = self._parse_invokes(text)
            if not invokes:
                return ExtractedToolCallInformation(
                    tools_called=False, tool_calls=[], content=text or None
                )
            tool_calls = [
                ToolCall(
                    type="function",
                    function=FunctionCall(
                        name=name,
                        arguments=json.dumps(
                            self._arguments(name, params, request), ensure_ascii=False
                        ),
                    ),
                )
                for name, params, _ in invokes
            ]
            content = text[: self._content_end(text)].strip()
            return ExtractedToolCallInformation(
                tools_called=True, tool_calls=tool_calls, content=content or None
            )
        except Exception:
            logger.exception("Error extracting ATEM tool calls from response.")
            return ExtractedToolCallInformation(
                tools_called=False, tool_calls=[], content=text or None
            )

    # -- streaming ---------------------------------------------------------

    def _partial_arguments(
        self,
        name: str,
        params: list[tuple[str, str, bool]],
        closed: bool,
        request: AnyRequest | None,
    ) -> str:
        """A growing prefix of the final `arguments` JSON.

        Only parameters whose closing tag has arrived are included, so the
        string only ever grows by whole `, "key": value` chunks and the final
        `}` -- which is exactly what the OpenAI streaming contract wants, since
        the client concatenates the fragments.
        """
        types = self._param_types(name, request)
        done = [(k, v) for k, v, complete in params if complete]
        body = ", ".join(
            f"{json.dumps(k, ensure_ascii=False)}: "
            f"{json.dumps(self._coerce(v, types.get(k)), ensure_ascii=False)}"
            for k, v in done
        )
        return "{" + body + ("}" if closed else "")

    def _content_delta(self, text: str) -> str | None:
        """Unsent plain text, holding back anything that may become markup."""
        end = self._content_end(text)
        if end == len(text):
            text = _hold_back(text)
            end = len(text)
        if end <= self._sent_content_idx:
            return None
        delta = text[self._sent_content_idx : end]
        self._sent_content_idx = end
        return delta or None

    def extract_tool_calls_streaming(
        self,
        previous_text: str,
        current_text: str,
        delta_text: str,
        previous_token_ids: Sequence[int],
        current_token_ids: Sequence[int],
        delta_token_ids: Sequence[int],
        request: ChatCompletionRequest,
    ) -> DeltaMessage | None:
        try:
            text = content_of(current_text)
            content = self._content_delta(text)
            deltas: list[DeltaToolCall] = []

            for index, (name, params, closed) in enumerate(self._parse_invokes(text)):
                while index >= len(self.prev_tool_call_arr):
                    self.prev_tool_call_arr.append({})
                    self.streamed_args_for_tool.append("")
                    self._streamed_names.append("")

                if not self._streamed_names[index]:
                    if not name:
                        break  # name tag still arriving
                    self._streamed_names[index] = name
                    self.prev_tool_call_arr[index]["name"] = name
                    deltas.append(
                        DeltaToolCall(
                            index=index,
                            type="function",
                            id=make_tool_call_id(),
                            function=DeltaFunctionCall(name=name).model_dump(
                                exclude_none=True
                            ),
                        )
                    )

                arguments = self._partial_arguments(name, params, closed, request)
                # Record the closed form: if generation is cut short, the
                # driver flushes the difference (the missing `}`) at the end of
                # the stream via get_remaining_unstreamed_args, so the client
                # never sees truncated JSON.
                self.prev_tool_call_arr[index]["arguments"] = (
                    arguments if closed else arguments + "}"
                )
                sent = self.streamed_args_for_tool[index]
                if not arguments.startswith(sent) or len(arguments) == len(sent):
                    continue
                self.streamed_args_for_tool[index] = arguments
                deltas.append(
                    DeltaToolCall(
                        index=index,
                        function=DeltaFunctionCall(
                            arguments=arguments[len(sent) :]
                        ).model_dump(exclude_none=True),
                    )
                )

            if content or deltas:
                return DeltaMessage(content=content, tool_calls=deltas)
            return None
        except Exception:
            logger.exception("Error handling streaming ATEM tool call.")
            return None


# Eager registration: `--tool-parser-plugin` / `--reasoning-parser-plugin` load
# this file by path, so there is no importable module name for the managers'
# lazy path to come back to.
ReasoningParserManager.register_module(name="atem", module=ATEMReasoningParser)
ToolParserManager.register_module(name="atem", module=ATEMToolParser)
logger.info("Registered ATEM reasoning and tool parsers (Muse Glimmer).")
