# Session Model Profiles And Web Session Switching Design

## Summary

This change adds session-scoped model selection across all channels, backend-driven Web session switching, and provider-compatible request/message persistence needed to fix model-specific tool-call failures such as:

`provider error 400 Bad Request: thinking is enabled but reasoning_content is missing in assistant tool call message at index 36`

The design keeps model switching explicit and controlled. Users switch profiles with slash commands, but only to profiles declared in config. Each session persists its selected profile so the active model survives restarts and reconnects.

## Goals

- Support multiple configured model profiles.
- Support per-model request parameters such as `temperature`.
- Support session-scoped model switching via slash commands across CLI, Web, Telegram, and WeCom.
- Persist the active model profile in the session store.
- Expose backend-driven session listing and session creation for the Web UI.
- Preserve provider-specific assistant message fields across tool loops so reasoning-enabled models do not break on replay.

## Non-Goals

- No natural-language model switching.
- No per-message ad hoc request parameter overrides.
- No global default mutation when a user switches models inside a session.
- No new Web-only model picker in this slice; Web uses the same slash commands as other channels.
- No changes to the provider registry surface beyond what is needed for profile resolution and request parameter injection.

## Chosen Approach

### 1. Configured session model profiles

Model selection is based on an explicit profile map under `agents.profiles`. The user-facing key is the command value and session identifier for the selected model profile.

Example shape:

```json
{
  "agents": {
    "defaults": {
      "workspace": "/Users/sage/.nanobot-rs/workspace",
      "defaultProfile": "openai:gpt-4.1-mini",
      "maxToolIterations": 20
    },
    "profiles": {
      "openai:gpt-4.1-mini": {
        "provider": "openai",
        "model": "gpt-4.1-mini",
        "request": {
          "temperature": 0.7
        }
      },
      "openrouter:deepseek-r1": {
        "provider": "openrouter",
        "model": "deepseek/deepseek-r1",
        "request": {
          "temperature": 0.2,
          "reasoning": {
            "enabled": true
          }
        }
      }
    }
  }
}
```

This preserves a controlled config surface while still letting users switch by raw `provider:model` style keys.

### 2. Slash command control surface

All channels support two new commands:

- `/models`
- `/model <provider:model>`

`/models` lists configured profiles and marks the current session profile.

`/model <provider:model>` switches only the current session. It fails clearly if the requested profile key is not configured.

`/new` clears message history and resets the session back to `agents.defaults.defaultProfile`.

`/help` includes the new commands.

### 3. Session metadata persistence

Sessions persist an `active_profile` alongside the existing metadata. This value is separate from the message history and survives restarts.

Each session also keeps provider-specific message extension fields so assistant/tool messages can be replayed without losing model-required protocol state.

### 4. Backend-driven Web session switching

The Web UI no longer treats the current session as a single opaque `localStorage` value. Instead, the backend exposes session list/create endpoints and the UI renders the available sessions, including the active profile and recent preview metadata.

The browser still remembers the currently selected session ID locally, but the session list itself comes from the backend session store.

### 5. Provider request + history compatibility

The fixed `"temperature": 1.0` request body is replaced by request assembly from the active profile. Provider calls use:

- selected profile provider
- selected profile model
- selected profile `request` object merged into the request body

Assistant/tool messages in session history preserve unknown provider-specific fields so reasoning-enabled providers can validate follow-up tool-call turns.

## Alternatives Considered

### A. Direct unconfigured `provider:model` switching

Rejected because it weakens validation and makes request parameter behavior inconsistent. It would also make the reasoning/tool-call compatibility issue easier to reintroduce.

### B. Model-only switching while provider stays global

Rejected because it does not match the required `/model provider:model` semantics and would create confusing partial behavior across providers.

### C. Web-only session picker with local fake session list

Rejected because the requirement explicitly calls for backend-driven session listing, and local-only session enumeration would not reflect persisted server state.

## Detailed Design

### Config changes

`AgentDefaults` changes from:

- `workspace`
- `model`
- `provider`
- `maxToolIterations`

to:

- `workspace`
- `defaultProfile`
- `maxToolIterations`

Add:

- `AgentProfileConfig`
  - `provider: String`
  - `model: String`
  - `request: serde_json::Value`
- `AgentsConfig.profiles: HashMap<String, AgentProfileConfig>`

Rules:

- `defaultProfile` must exist in `profiles`.
- profile keys are exact-match, case-sensitive command targets.
- `request` defaults to `{}`.

### Session changes

`Session` metadata gains:

- `active_profile: String`

`SessionMessage` gains:

- `extra: Map<String, Value>`

`extra` stores assistant/tool message fields not already modeled by:

- `role`
- `content`
- `tool_calls`
- `tool_call_id`
- `name`

Compatibility rules:

- `extra` must deserialize with an empty default when absent so existing session records remain loadable.
- `active_profile` must default to `agents.defaults.defaultProfile` when absent in persisted session metadata.
- `extra` is only persisted for roles where unknown provider fields matter for replay, specifically `assistant` and `tool`. Other roles may leave it empty.

During `to_llm_message()`, `extra` is merged back into the outgoing provider message object without overwriting modeled fields.

This is the protocol-preservation mechanism for fields such as `reasoning_content`.

### Agent runtime changes

The agent resolves the effective model profile per session:

1. load session
2. determine `active_profile` or fall back to `defaultProfile`
3. build request config from profile
4. run provider call with profile-specific provider/model/request parameters

Command behavior:

- `/help`
  - includes `/models` and `/model <provider:model>`
- `/models`
  - returns configured profile list with current profile marker
- `/model <provider:model>`
  - validates target exists
  - updates session `active_profile`
  - persists session immediately
  - returns confirmation
- `/new`
  - mutates the current session in place; it does not allocate a new session ID
  - clears messages
  - resets `active_profile` to default profile
  - persists session

The command surface applies uniformly because all channels already funnel through `process_message`.

### Provider/runtime changes

The provider interface needs to accept an explicit request descriptor instead of only a plain `model` string. The minimum new runtime input is:

- provider name
- model name
- request extras JSON

The provider implementation merges:

- `"model": <profile.model>`
- `"messages": ...`
- `"tools": ...`
- profile `request` object

Merge rule:

- required transport keys supplied by runtime win over conflicting profile keys
- profile keys fill the rest of the body

This preserves central control over `messages/tools/model` while still allowing per-model extras like `temperature` or reasoning toggles.

### Web API changes

Add:

- `GET /api/sessions`
- `POST /api/sessions`

`GET /api/sessions` returns Web-channel sessions with:

- `sessionId`
- `updatedAt`
- `activeProfile`
- `preview`

`preview` is derived from the latest user or assistant text message, truncated for UI display.

`POST /api/sessions` creates a new Web session, initialized to the default profile, and returns the created session summary.

Existing `POST /api/chat` continues to accept `sessionId`. The response should also include `activeProfile` so the UI can refresh session metadata after `/model ...`.

### Web UI changes

The page adds a backend-backed session list and create flow:

- fetch session list on load
- render current session list
- allow selecting a session
- keep selected `sessionId` in local storage
- create a new session via `POST /api/sessions`
- refresh session list after send/new/model-change

The existing transcript area stays per selected session.

This slice does not add a dedicated model dropdown. Users switch models with `/model ...` in the composer, keeping behavior consistent across channels.

## Error Handling

### Invalid model profile

`/model <provider:model>` returns:

- clear error if missing argument
- clear error if profile not found

No session state is mutated on failure.

### Config compatibility

This slice keeps compatibility with existing configs during migration:

- if `agents.defaults.defaultProfile` and `agents.profiles` are present, they are authoritative
- if the new fields are absent but legacy `agents.defaults.provider` and `agents.defaults.model` are present, config loading synthesizes a single profile entry using the key `<provider>:<model>` and uses it as the default profile
- writing a fresh config or onboarding output uses only the new shape

This keeps current installations bootable while letting the implementation move the system toward the profile-based configuration surface.

### Bad config

Startup/config load should fail clearly if:

- `defaultProfile` is missing from `profiles`
- a profile references an unknown provider
- a profile `request` value is not an object

### Reasoning/tool-call protocol failures

If a provider still returns an unsupported or malformed message shape, the error should surface as a provider error, but the new session message persistence must not silently drop unknown assistant fields.

## Testing Strategy

### Config/profile tests

- defaults include `defaultProfile`
- `defaultProfile` must exist in profiles
- profile request objects round-trip from config

### Session tests

- session metadata saves/restores `active_profile`
- `/new` resets `active_profile`
- `SessionMessage.extra` round-trips unknown assistant fields
- `to_llm_message()` restores merged extra fields

### Agent command tests

- `/models` lists configured profiles and marks current
- `/model` switches current session only
- invalid `/model` target returns error
- switching a session does not alter another session
- `/help` includes the new commands

### Provider/runtime tests

- profile request extras are merged into request body
- required runtime keys override colliding profile keys
- provider request body no longer hardcodes `temperature: 1.0`
- assistant message extra fields survive tool-loop replay

### Web tests

- `GET /api/sessions` returns session summaries
- `POST /api/sessions` creates a new session
- `POST /api/chat` returns `activeProfile`
- page loads and renders session list
- page can switch session and update stored `sessionId`
- new session creation updates UI state

## Implementation Notes

The work should be split into two implementation tracks after the backend contract is defined:

1. backend profile/session/provider changes
2. Web session list/create/switch UI

They are related but can proceed in parallel once the session API contract and response shapes are fixed.

## Acceptance Criteria

- Users can switch models with `/model <provider:model>` in CLI, Web, Telegram, and WeCom.
- The selected model affects only the current session.
- The selected model survives process restart for that session.
- Each configured model can carry request extras such as `temperature`.
- Web lists persisted sessions from the backend and allows switching among them.
- The known provider 400 reasoning/tool-call replay failure is prevented by preserving assistant message extension fields during session persistence and replay.
