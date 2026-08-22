# On-device AI (`shell.ai`)

`shell.ai` gives contents HTML a local language model: plain text generation, schema-constrained
structured output, tool calling back into your own JavaScript, and token streaming.

This is the deep reference. For the one-screen version, see
[`js-api.md`](js-api.md#on-device-ai-ai) and
[`app-agent-guide.md`](app-agent-guide.md#ai--generate--generateobject--stream).
The same generate path is also on the binary as a headless command (`app-ly ai "say hi"`) —
no window. JS tool handlers are not available; `[[allowedCommands]]` from the loaded
`app.toml` are offered to the model instead (same allowlist as `shell.run` / `app-ly run`).
See [`app-agent-guide.md`](app-agent-guide.md#cli).

## What it is, and the privacy story

Everything runs **on the device**. The shell makes no network request on your behalf for
generation: prompts, tool arguments, tool results and completions are handed to the operating
system's local model and come straight back. There is no API key, no account, no per-token cost,
and no endpoint to configure — because there is no endpoint.

The consequences are worth stating in both directions:

- Prompts and generated text never leave the machine through this API, so it is usable for
  content you would not send to a hosted model.
- The model is small compared to a hosted one. Expect short, focused work — summarising, tagging,
  extracting fields, rewriting, classifying — and not long-context reasoning or world knowledge.
- Availability is **not** guaranteed. A perfectly healthy machine can report the model as
  unavailable, so every app has to handle that state. See
  [Check availability first](#check-availability-first).

The API surface is vendor-neutral on purpose: method names, config keys, model ids, event names
and reason codes never mention an OS vendor, so a future backend on another platform is a drop-in.
Only this document's platform sections name the vendor, because there honesty requires it.

## Platform support

| Requirement | Detail |
|---|---|
| Operating system | **macOS 26 or newer.** An older macOS reports `unsupported-os`. |
| Hardware | **Apple silicon.** Intel Macs report `device-not-eligible`. |
| OS setting | **Apple Intelligence must be turned on** in System Settings. If it is off, the shell reports `not-enabled`. |
| Model download | The on-device model is downloaded by the OS. While that is pending the shell reports `model-not-ready`. |
| Shell build | Built with the default `ai-apple` Cargo feature, which needs Xcode 26 / the macOS 26 SDK and a Swift toolchain **at build time** (the backing crate compiles a Swift bridge). |
| Everything else | Windows, Linux, and any macOS build made with `--no-default-features` fall back to a stub backend that reports `unsupported-platform` and rejects every generate call. |

The stub is not a failure mode — it is the supported way to build the shell without an AI
toolchain. `shell.ai.info()` still answers on those builds, it just answers "no".

### Linking the Swift runtime

The AI backend pulls in Apple's Swift runtime. Most of it is linked by absolute path and
resolves by itself, but `libswift_Concurrency.dylib` is *back-deployable* and is referenced
as `@rpath/libswift_Concurrency.dylib`. A Rust link emits no `LC_RPATH` entries at all, so
without help `@rpath` expands to nothing and the app dies at launch — when run directly and
when bundled in the `.app`:

```
dyld: Library not loaded: @rpath/libswift_Concurrency.dylib
  Reason: tried: '/opt/homebrew/lib/libswift_Concurrency.dylib' (no such file),
                 '/libswift_Concurrency.dylib' (no such file)
```

[`src-tauri/build.rs`](../src-tauri/build.rs) fixes this by adding the search path when the
`ai-apple` feature is on:

```rust
println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
```

**This has to live in this app's own `build.rs`, not in the AI crate.** The crate does emit
the same flag, but `cargo:rustc-link-arg` never reaches a dependent's binary: Cargo applies
it only to the emitting package's own benchmarks, binaries, `cdylib`s, examples and tests.
That is deliberate — see [rust-lang/cargo#9554](https://github.com/rust-lang/cargo/issues/9554),
where the Cargo team concluded that "in general, rpath is something that a dependency can't
safely set for a crate that depends on it", and that a dependent must opt in from its own
build script. The AI crate is additionally built as an `rlib` here, which is not a linked
target at all, so its flag matches nothing. Tauri itself takes the same approach — the
`@executable_path/../Frameworks` rpath comes from `tauri_build::build()`, which is a library
function this app calls from its own `build.rs`.

Two things that are normal and not worth chasing:

- `ls /usr/lib/swift` looks empty. On macOS 26 the Swift runtime lives in the dyld shared
  cache rather than on disk; dyld still resolves the path. Do not bundle a copy of the
  library — embedding is for back-deploying to older macOS, and there is no file to embed.
- The rpath is baked in at link time, so `tauri build` and code signing preserve it.
  Patching a built binary with `install_name_tool` instead would invalidate its signature
  and force a re-sign.

## Check availability first

`info()`, `available()` and `models()` **never reject**. `generate()`, `generateObject()` and
`stream()` **do** reject when the model is unavailable, with:

```
ai unavailable: <reason> — <detail>
```

(the `— <detail>` half is omitted when there is no detail). So the pattern is: ask once, branch,
and only then generate.

```javascript
const info = await shell.ai.info();

if (!info.available) {
  status.textContent = `AI unavailable: ${info.reason}${info.detail ? ` — ${info.detail}` : ""}`;
} else {
  const { text } = await shell.ai.generate("Say hello in five words.");
  show(text);
}
```

`shell.ai.available()` is the boolean shorthand when you do not care why:

```javascript
if (await shell.ai.available()) {
  aiButton.disabled = false;
}
```

### `info()` shape

| Field | Type | Description |
|-------|------|-------------|
| `available` | `boolean` | `true` when generation may proceed right now |
| `reason` | `string \| null` | `null` when available; otherwise one of the codes below |
| `detail` | `string \| null` | Human-readable explanation; may be `null` |
| `models` | `AiModel[]` | `[]` when unavailable; otherwise exactly one entry |
| `features` | `object` | `{ text, structured, tools, streaming }` — all `false` when unavailable |

`AiModel` is `{ id, name, default }`. This shell exposes a single non-selectable model:
`{ id: "default", name: "On-device model", default: true }`.

### Reason codes

A closed set. Anything you branch on is in this table.

| Code | Meaning | `detail` |
|---|---|---|
| `unsupported-platform` | Not macOS, or a build without the AI backend | `this build has no on-device model backend` |
| `unsupported-os` | The OS is too old to have an on-device model API | `this operating system has no on-device model API` |
| `disabled-by-config` | `[ai] enabled = false` in `app.toml` | `set [ai] enabled = true in app.toml to turn it on` |
| `device-not-eligible` | The hardware/region does not support on-device AI | `this device does not support on-device AI` |
| `not-enabled` | The user has not turned the OS AI feature on | `on-device AI is turned off in system settings` |
| `model-not-ready` | The model is still downloading or preparing | `the on-device model is still downloading or preparing` |
| `unavailable` | Any other state the OS reports | `the system reports the on-device model as unavailable` |

`unavailable` is also what `info()` reports if the underlying call itself fails (for example an ACL
misconfiguration in a modified shell) — in that case `detail` carries the underlying error message
instead. That is why `info()` can promise never to reject.

Only `disabled-by-config` is under the app author's control; the rest describe the machine.

## Configuration — `[ai]`

An optional table in `app.toml`. Absent means "all defaults, feature on".

```toml
[ai]
enabled = true                                   # optional; default true. false => reason "disabled-by-config"
instructions = "Answer briefly and in plain text."  # optional; default system prompt for every request
temperature = 0.7                                # optional; default sampling temperature
maxTokens = 512                                  # optional; default cap on response length
toolTimeoutMs = 30000                            # optional; default 30000. How long the shell waits for a
                                                 # JS tool handler before answering the model with an error.
```

| Key | Type | Default | Notes |
|---|---|---|---|
| `enabled` | boolean | `true` | `false` makes every call report/reject with `disabled-by-config` |
| `instructions` | string | none | System prompt applied to every request that does not set its own |
| `temperature` | float | model default | Passed through to the model's sampling options |
| `maxTokens` | integer | model default | Upper bound on response length |
| `toolTimeoutMs` | integer | `30000` | Per tool call. Overridden by per-request `options.toolTimeoutMs` |

Unknown keys are ignored, so an `app.toml` written for a newer shell still loads on an older one.
Per-request `options` always win over these defaults, field by field: setting `temperature` on one
call does not disturb the configured `instructions`.

## API reference

Six calls, all on `window.shell.ai`.

### `shell.ai.info()`

- Returns: `Promise<AiInfo>` — the shape above. Never rejects.

### `shell.ai.available()`

- Returns: `Promise<boolean>` — `info().available`. Never rejects.

### `shell.ai.models()`

- Returns: `Promise<AiModel[]>` — `[]` when unavailable. Never rejects.

```javascript
const models = await shell.ai.models();
// [{ id: "default", name: "On-device model", default: true }]
```

### `shell.ai.generate(prompt, options?)`

One-shot text generation. Runs off the UI thread, so the webview stays responsive while it works.

- `prompt` — the user-side text
- `options` — optional, see [Request options](#request-options)
- Returns: `Promise<{ text, model, toolCalls }>`

| Field | Type | Description |
|-------|------|-------------|
| `text` | `string` | The generated text |
| `model` | `string` | The model id actually used — always `"default"` today |
| `toolCalls` | `array` | Every tool call the model made, `[]` if none. See [Tool calling](#tool-calling) |

```javascript
const result = await shell.ai.generate("Write a haiku about desktop apps.", {
  instructions: "You are concise.",
  maxTokens: 200,
});

show(`${result.text}\n\n-- model: ${result.model}`);
```

### `shell.ai.generateObject(prompt, schema, options?)`

Structured output. The model is **grammatically constrained** to the schema while it decodes, so
the result parses — this is real constrained decoding, not "please reply in JSON" prompting.

- `prompt` — what to produce
- `schema` — a JSON Schema object, see [Structured output](#structured-output)
- `options` — optional, see [Request options](#request-options)
- Returns: `Promise<{ object, model, toolCalls }>`, where `object` is already parsed JSON

```javascript
const schema = {
  type: "object",
  properties: {
    title: { type: "string", description: "Short title" },
    tags: { type: "array", items: { type: "string" }, maxItems: 5 },
    rating: { type: "integer", minimum: 1, maximum: 5 },
  },
  required: ["title", "tags", "rating"],
};

const { object } = await shell.ai.generateObject(
  "Describe the app-ly desktop shell.",
  schema,
);

console.log(object.title, object.tags, object.rating);
```

### `shell.ai.stream(prompt, options?)`

Streaming text generation. See [Streaming](#streaming) for the returned handle.

- Returns: `Promise<AiStream>` — resolves once the event listeners are live **and** the backend has
  accepted the request, so no delta can be missed by subscribing "too late"

### Request options

Every field is optional, and every one overrides the matching `[ai]` config default.

| Option | Type | Description |
|---|---|---|
| `model` | `string` | Must be `"default"` (or omitted). Anything else rejects with `unknown model "x" — this shell exposes one model id, "default"` |
| `instructions` | `string` | System prompt for this request only |
| `temperature` | `number` | Sampling temperature |
| `maxTokens` | `number` | Response length cap |
| `toolTimeoutMs` | `number` | How long to wait for a JS tool handler, in milliseconds |
| `tools` | `array` | Tool declarations plus their JS handlers — see [Tool calling](#tool-calling) |

All three generating calls accept the same options, tools included.

## Structured output

`generateObject` takes a JSON Schema, but the backend's schema engine is **not** a JSON Schema
implementation — the shell translates your schema into the backend's own dialect. That translation
supports a deliberate subset. Anything outside it is either silently ignored or rejected with a
clear error, and the two lists below are exhaustive.

### Supported keywords

| Keyword | Applies to | Behaviour |
|---|---|---|
| `type` | any node | `object`, `array`, `string`, `integer`, `number`, `boolean`, `null`. A **missing** `type` is treated as `object`. An array such as `["string", "null"]` uses the first non-`null` entry. Nullable is not optional: the property stays required unless `required`/`optional` says otherwise |
| `description` | any node | Passed to the model verbatim. This is the single most effective way to steer a field — use it |
| `properties` | object | Child schemas, translated recursively |
| `required` | object | Marks which properties are mandatory. **An absent `required` means every property is required** (see the caveat below) |
| `optional` | property | Non-standard extension: `optional: true` on a property forces it optional even if `required` lists it |
| `items` | array | A single element schema. Absent defaults to `{ "type": "string" }` |
| `minItems` / `maxItems` | array | Element-count bounds. `min` / `max` are accepted as aliases |
| `enum` | string | String values only; becomes a fixed set of choices |
| `anyOf` / `oneOf` | any node | A choice between translated branches. `oneOf` is treated exactly like `anyOf` — there is no exclusivity check |
| `pattern` | string | Regex constraint on the value |
| `const` | string | Fixes the value |
| `minimum` / `maximum` | number, integer | Numeric range. Either bound may be given alone |

Precedence within one node: `anyOf`/`oneOf` first, then `enum`, then `type`. A node with both
`enum` and `anyOf` uses `anyOf` and ignores the `enum`.

> **The `required` caveat.** Standard JSON Schema treats an omitted `required` as "nothing is
> required". This translator inverts that: with no `required` array, *all* properties are
> mandatory. Without it the model is free to return `{}` and technically satisfy the schema. If you
> want optional fields, list the mandatory ones in `required`, or mark the optional ones with
> `optional: true`.

### Ignored keywords

Present-but-ignored, no error, no effect: `$schema`, `$id`, `$ref`, `$defs`, `definitions`,
`title`, `default`, `examples`, `additionalProperties`, `patternProperties`, `propertyNames`,
`minLength`, `maxLength`, `format`, `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`,
`uniqueItems`, `allOf`, `not`, `if`/`then`/`else`, and `const` on anything other than a string.

There is no `$ref` resolution of any kind, so every schema must be written out inline. Deep nesting
of objects and arrays is fine; references between them are not.

### Rejected shapes

These reject the whole call before any generation starts, with the error text shown.

| Schema | Error |
|---|---|
| A node that is not a JSON object | `schema at "Root" must be a JSON object` |
| An unknown `type` | `unsupported schema type "widget" at "Root"` |
| `enum` with non-string values | `"enum" at "Root" supports string values only` |
| `anyOf` / `oneOf` that is not an array | `"anyOf" at "Root" must be an array` |
| `type` that is neither string nor array | `"type" at "Root" must be a string or array` |
| `type: ["null"]` and nothing else | `schema at "Root" has no usable "type"` |
| Tuple-form `items` (an array of schemas) | `schema at "RootItem" must be a JSON object` |

The quoted name in an error is a path, so you can find the offending node: the root is `Root`, a
property uses its own key, an array's element schema is `<name>Item`, and an `anyOf` branch is
`<name>Choice<index>`. Errors from a **tool's** `parameters` schema use the tool name as the root
and are prefixed, e.g. `tool "get_time": unsupported schema type "widget" at "zone"`.

If the model somehow emits output the shell cannot parse, `generateObject` rejects with
`model returned invalid JSON: <detail>`.

## Tool calling

Declare tools in `options.tools` and the model can call them mid-generation. Each entry is:

| Field | Type | Description |
|---|---|---|
| `name` | `string` | What the model calls. Must be unique within the request |
| `description` | `string` | What it does — the model picks tools by this text. Defaults to `""` |
| `parameters` | object | JSON Schema for the arguments, same subset as above. Defaults to `{ type: "object", properties: {} }` |
| `handler` | function | `handler(args)` — sync or async, returning anything JSON-serialisable |

The `handler` **never leaves the page**. The shell strips it and sends only
`{ name, description, parameters }`; handlers stay in JS, keyed to this one request, and are torn
down when it finishes. Two concurrent requests exposing the same tool name never cross-wire.

### Worked example

```javascript
const result = await shell.ai.generate("What time is it in Tokyo, and is that a work hour?", {
  instructions: "Use the tools you are given instead of guessing.",
  tools: [
    {
      name: "get_time",
      description: "Return the current wall-clock time in an IANA time zone.",
      parameters: {
        type: "object",
        properties: {
          zone: { type: "string", description: "IANA zone name, e.g. Asia/Tokyo" },
        },
        required: ["zone"],
      },
      handler: async ({ zone }) => ({
        zone,
        time: new Date().toLocaleTimeString("en-GB", { timeZone: zone }),
      }),
    },
  ],
});

show(result.text);

for (const call of result.toolCalls) {
  console.log(call.name, call.arguments, call.error ?? call.result);
}
```

### `toolCalls`

Every *attempted* call is recorded, in the order the model made them, on `generate`,
`generateObject`, and a stream's `completed`.

| Field | Type | Description |
|-------|------|-------------|
| `name` | `string` | The tool the model asked for |
| `arguments` | `any` | The arguments it supplied, as JSON |
| `result` | `any \| null` | What your handler returned, on success |
| `error` | `string \| null` | Why it failed, otherwise |

Exactly one of `result` / `error` is ever set. A handler returning `undefined` records `null`.

### The three failure rules

These matter because none of them abort the generation — the model is told what went wrong and
keeps going, which is almost always what you want:

1. **A handler that throws** reports the failure to the model. The thrown message
   (`error.message`, or the stringified value) lands in that call's `error`, and the model receives
   `{ "error": "<message>" }` as the tool's output. It does **not** wait for the timeout.
2. **An unknown tool name** — the model asks for a tool this request did not register — is answered
   with `unknown tool "x"` in the same way. Generation continues.
3. **A handler that never returns** hits `toolTimeoutMs` (default `30000`, from `[ai]` or this request's `options.toolTimeoutMs`). The call
   is recorded as `tool "x" did not answer within 30000ms` and the model is handed that as the
   tool's output. A hung handler can never hang the app; if it eventually resolves, its late answer
   is discarded silently.

Two practical consequences: keep handlers fast (they are competing with a running inference), and
return an explicit error value rather than throwing when "no result" is a normal outcome — the
model reads either one, but a deliberate value reads better.

## Streaming

`shell.ai.stream(prompt, options?)` resolves to a handle once the listeners are registered and the
backend has accepted the request.

| Member | Type | Description |
|---|---|---|
| `id` | `string` | The request id, echoed on every event for this request |
| `onText(cb)` | `(text: string) => void` → `unsubscribe` | Called with each text delta; the first handler also drains everything buffered before it |
| `completed` | `Promise<{ text, model, toolCalls }>` | Resolves when generation finishes; **rejects** on error or cancellation |
| `cancel()` | `() => Promise<void>` | Best-effort stop, see [Cancellation](#cancellation). Resolves even if already finished |
| `[Symbol.asyncIterator]` | yields `string` | `for await` over the deltas; ends when generation ends |

**No delta is lost.** Deltas that arrive before a consumer exists are buffered and replayed to the
first `onText` handler or the first iterator, so attaching late is safe. Every `shell://ai-chunk`
for a request is emitted before its `shell://ai-done`, so `completed` never resolves ahead of text
you have not seen yet.

The buffer is drained by whoever claims it first — one iterator, or one `onText` handler. Do not
expect two independent consumers to each receive the full backlog.

```javascript
const stream = await shell.ai.stream("Write a short poem about the sea.");

let text = "";
const stop = stream.onText((delta) => {
  text += delta;
  out.textContent = text;
});

try {
  const done = await stream.completed;
  console.log("finished on", done.model, "with", done.toolCalls.length, "tool calls");
} catch (error) {
  console.warn("stream failed:", error.message);
} finally {
  stop();
}
```

The same thing as an async iteration:

```javascript
const stream = await shell.ai.stream("List three desktop app ideas.");

for await (const delta of stream) {
  out.textContent += delta;
}

const { model } = await stream.completed;
```

> **Accumulate the text yourself if you need partial output.** On success `completed` carries the
> full `text`. On an error — cancellation included — it rejects, and the completion event's `text`
> is empty. Whatever you collected from `onText` is all you get.

## Cancellation

```javascript
const stream = await shell.ai.stream("Write a very long essay about ferries.");
cancelButton.onclick = () => stream.cancel();
```

Read this honestly:

- **Cancellation is best-effort, and it does not stop the model.** The underlying inference has no
  cancel API, so it keeps running to completion in the background; the shell stops forwarding its
  output and throws the result away. You get your UI back immediately; the machine does not get its
  cycles back.
- The stream stops emitting deltas as soon as the flag is set, and finishes with the error
  `cancelled` — so `completed` rejects with `Error("cancelled")`.
- `cancel()` on an already-finished stream is a no-op that resolves.
- **`cancel()` exists on the stream handle only.** There is no way to cancel a `generate` or
  `generateObject` call — those return a result, not a handle. If you need a bail-out button, use
  `stream()`.

## Errors

Every generating call rejects with a string. The ones worth recognising:

| Message | Cause |
|---|---|
| `ai unavailable: <reason> — <detail>` | The model is not usable. `reason` is a code from the [table](#reason-codes) |
| `unknown model "x" — this shell exposes one model id, "default"` | `options.model` was set to something else |
| `schema at "Root" must be a JSON object` (and the others in [Rejected shapes](#rejected-shapes)) | The schema is outside the supported subset |
| `model returned invalid JSON: <detail>` | `generateObject` could not parse the model's output |
| `cancelled` | The request was cancelled — as the rejection of `completed` |
| `ai request "<id>" is already in flight` | A request id was reused while still running (see [How it works](#how-it-works)) |
| `ai generate: …` / `ai generate object: …` / `ai stream: …` / `ai session: …` / `ai instructions: …` | The backend itself failed — a refusal by the model's safety guardrails, a rejected instructions string, or an internal error |

What does **not** reject:

- `info()`, `available()`, `models()` — ever.
- A tool handler that throws, times out, or does not exist. Those are reported to the model and
  recorded in `toolCalls`; the surrounding call still resolves.

A reasonable shape for an app:

```javascript
async function ask(prompt) {
  const info = await shell.ai.info();
  if (!info.available) {
    return `AI is unavailable (${info.reason}).`;
  }
  try {
    const { text } = await shell.ai.generate(prompt);
    return text;
  } catch (error) {
    await shell.log(`ai failed: ${error}`, "error");
    return "The model could not answer that.";
  }
}
```

## How it works

Useful when debugging, and when modifying the shell itself.

**Backend selection.** `src-tauri/src/ai.rs` holds the commands, wire types, tool bridge and schema
translator on every platform, and picks a backend at compile time behind one trait — so no command
body branches on the platform. `ai/backend_apple.rs` is compiled for macOS with the `ai-apple`
feature and talks to Apple's FoundationModels framework through the `foundation-models` crate;
`ai/backend_stub.rs` is compiled everywhere else and answers "unavailable" to everything. The
public surface is identical either way.

**Availability** is checked in two layers: `[ai] enabled = false` short-circuits to
`disabled-by-config` before the backend is consulted at all; otherwise the backend maps the OS's
own availability state onto the reason codes.

**Commands.** JS calls six Tauri commands — `shell_ai_info`, `shell_ai_generate`,
`shell_ai_generate_object`, `shell_ai_stream`, `shell_ai_tool_result`, `shell_ai_cancel` — each
allowlisted in `src-tauri/permissions/shell.toml`. `generate` and `generateObject` run the blocking
inference on a blocking task; `stream` returns immediately and runs on its own thread.

**The request id.** JS mints an opaque id (`ai-<counter>-<random>`) *before* it issues the invoke
and passes it in; Rust never invents one and never parses it. Every event for that request echoes
it back, so a tool call that arrives before the invoke has even resolved still binds to the right
handler map. The shell **rejects** a request id that is already in flight rather than overwriting
it — silently overwriting would cross-wire two callers' tool handlers. The built-in JS client
generates unique ids, so this guard is not something the public API can trip.

**Events** (all delivered to the `main` window, all carrying the request `id`):

| Event | Payload |
|---|---|
| `shell://ai-chunk` | `{ id, text }` — one text delta |
| `shell://ai-done` | `{ id, text, model, toolCalls, error }` — `error` is `null` on success |
| `shell://ai-tool-call` | `{ callId, id, name, arguments }` |

`shell.ai` subscribes to all three for you; there is no `onAi*` API to call yourself. The backend's
stream call only returns after the last delta has been handed to the shell, so the done event
cannot overtake a chunk.

**The tool bridge.** When the model calls a tool, Rust registers an unguessable `callId`, emits
`shell://ai-tool-call`, and blocks on a channel. JS looks up the handler by request id and tool
name, awaits it, and invokes `shell_ai_tool_result` with `{ callId, ok, value, error }`. Rust either
receives that or gives up after `toolTimeoutMs`, folds a failure into `{ "error": … }`, and hands
the model an answer either way. A result for an unknown or expired `callId` is a silent no-op.

**Sessions.** Each request opens a fresh model session with that request's instructions and tools —
sessions cannot be reconfigured after creation, and the framework rejects concurrent work on one
session. Concurrent `shell.ai` calls are therefore genuinely independent, at the cost of no
conversation state being carried between them.

## Limitations

- **There is no chat history.** Each call is a fresh session; the model remembers nothing between
  calls. Multi-turn behaviour has to be built by putting prior turns in the prompt yourself, within
  the model's context window.
- **One model, no catalog.** `models()` always returns exactly one entry with id `"default"`, and
  any other `model` value rejects.
- Text in, text out. No images, audio, embeddings, token counts, log-probs, or finish reasons.
- **Cancellation does not stop inference** — it stops delivery and discards the result.
- `cancel()` is on the stream handle only; `generate` and `generateObject` cannot be cancelled.
- `generateObject` does not stream, and `stream` does not take a schema — structured output and
  streaming are mutually exclusive.
- The JSON Schema subset is limited to the keywords listed above, with **no `$ref` support** —
  every schema is inline. And an absent `required` means "all required", the opposite of standard
  JSON Schema.
- `enum` supports string values only; `const` applies to strings only.
- `toolTimeoutMs` is per tool call, not an overall deadline on a generation. There is no per-tool override.
- Tool handlers are keyed by name per request; duplicate names within one request collapse to the
  last handler registered.
- Tool results are handed to the model as JSON text, so they must be JSON-serialisable —
  functions, DOM nodes, `Map`s and the like will not survive.
- On error or cancellation a stream's completion event carries an empty `text`; partial output is
  only what you accumulated from `onText`.
- No progress signal beyond text deltas: there is no token count, no time estimate, and no event
  for "the model started thinking".
- Requests have no overall deadline — only tool calls are timed out. A slow generation is your
  problem to surface in the UI.
- The model applies its own safety guardrails and can refuse; a refusal surfaces as a rejected
  promise (`ai generate: …`), not as a distinguishable result field.
