# Agent activity integrations

Enable **Settings → Appearance → Terminal activity** in the workspace. In the desktop manager, open **Agent activity**, select an agent and choose **Generate integration**. Copy and review the result before merging it into the agent's existing settings. Generation does not modify those settings or approve tools. Restart the agent after installing its hooks.

The manager keeps the hook executable at a stable path in its own data directory, including for AppImage installations. Generate the integration again after updating gtmux to refresh that executable. If the manager data directory is removed, regenerate the integration. Hooks are inactive outside a gtmux terminal.

## Supported signals

| Agent | Working | Completed | Input needed | Setup |
|---|---|---|---|---|
| Claude Code | Prompt/tool hooks | Stop | PermissionRequest, permission notification | Merge hooks into `~/.claude/settings.json` |
| Codex | Prompt/tool hooks | Stop | PermissionRequest | Merge into `~/.codex/hooks.json`, then review and trust in `/hooks` |
| Gemini CLI | BeforeAgent/tool hooks | AfterAgent | ToolPermission notification | Merge hooks into `~/.gemini/settings.json` |
| Copilot CLI | Prompt/tool hooks | agentStop | Not classified by this adapter | `.github/hooks/gtmux-activity.json` |
| Cursor | Prompt/tool hooks | stop with completed status | Not classified by this adapter | Merge into `~/.cursor/hooks.json` |
| Aider | Ordinary terminal activity | Completion notification | Not independently classified | Merge generated keys into Aider YAML or use its notification command flag |
| OpenCode | session.status | session.idle | Permission/question events | Save generated JavaScript as `.opencode/plugins/gtmux-activity.js` |

On Windows, `~` means your user profile. Use the gtmux build for the same operating environment as the agent. An agent installed only in WSL is not a Windows-native executable. Hook event availability depends on the agent version. Copilot configuration uses its executable/arguments hook format; Cursor CLI may expose fewer events than the desktop product.

Subagent completion is excluded where the protocol identifies it. A stop hook reports that the agent reached a turn boundary; another hook may still request continuation. Cancellation and failures are not reported as successful completion. The adapter reads event metadata from the hook payload; it does not open transcripts or save/transmit prompts or API keys. Unsupported events remain unknown. Ordinary output-based estimates are still labelled estimated.

## CLI and embedding

Run `gtmux agent hooks claude` to print configuration; replace `claude` with `codex`, `gemini`, `copilot`, `cursor`, `aider`, or `opencode`. This never overwrites an existing hook file. The generated command invokes `gtmux agent event` with JSON on stdin. Hosts can also send `gtmux terminal report working`, `completed`, `needs_input`, or `unknown` from a gtmux pane.

To disable integration, remove only the gtmux entries or its OpenCode plugin. To restore it, generate the configuration again. The Activity UI can be disabled independently. Unread counts in tab titles are off by default and can be restored in Appearance settings.

## Sources and validation scope

The adapters follow the official [Claude hooks](https://code.claude.com/docs/en/hooks), [Codex hooks](https://learn.chatgpt.com/docs/hooks), [Gemini hooks](https://geminicli.com/docs/hooks/reference/), [Copilot hooks](https://docs.github.com/en/copilot/reference/hooks-reference), [Cursor hooks](https://cursor.com/docs/hooks), [Aider notifications](https://aider.chat/docs/usage/notifications.html), and [OpenCode plugins](https://opencode.ai/docs/plugins/) references. Protocol fixtures and real gtmux CLI/server transport are tested. This does not mean every installed agent version has been exercised through a paid model conversation.

## Install and restore from the manager

Choose **Review installation** to see the exact target, then **Apply integration change**. The manager appends hooks and backs up existing settings, retaining permissions and other hooks. Restart the agent and accept its own hook trust prompt where required. It never trusts hooks on your behalf. **Review removal** restores the original settings only if they have not changed since installation; otherwise remove only gtmux entries manually. Invalid JSON, unsupported comment syntax, an existing Aider notification command, conflicting plugins and concurrent edits are reported without overwriting them. Custom agent configuration directories (for example `CODEX_HOME`) require the manual generated configuration route.
