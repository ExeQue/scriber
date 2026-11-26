# Connecting AI agents

Scriber is made to be driven by AI agents (Claude Code, Codex, Cursor, …). They talk to the editor over
[MCP](https://modelcontextprotocol.io) and edit the document that is open in your window, so you see
every change as it happens and can undo it.

## Connect with one click

**AI agents → Connect AI agents…** lists the agents Scriber found on this computer. Click
**Connect** next to an agent, then restart that agent. **Remove** takes Scriber out again.

From a terminal:

```sh
scriber agents             # list agents and whether Scriber is connected
scriber agents add         # connect all detected agents
scriber agents add codex   # or only some, by id
scriber agents remove      # disconnect
```

## Supported agents

| Agent | Id | Where Scriber adds itself |
|---|---|---|
| Claude Code | `claude-code` | `claude mcp add -s user` |
| Claude Desktop | `claude-desktop` | `claude_desktop_config.json` (through `scriber mcp`, see below) |
| Cursor | `cursor` | `~/.cursor/mcp.json` |
| VS Code | `vscode` | `Code/User/mcp.json` in your config folder |
| Windsurf | `windsurf` | `~/.codeium/windsurf/mcp_config.json` |
| Gemini CLI | `gemini` | `~/.gemini/settings.json` |
| Codex | `codex` | `~/.codex/config.toml` |
| opencode | `opencode` | `~/.config/opencode/opencode.json` |
| GitHub Copilot CLI | `copilot` | `~/.copilot/mcp-config.json` |
| Zed | `zed` | `zed/settings.json` in your config folder |
| LM Studio | `lmstudio` | `~/.lmstudio/mcp.json` |

Scriber only rewrites a config file it can read. If a file has comments or is not valid JSON, it
leaves the file alone and shows you the snippet to paste. Before its first change it writes a
backup next to the file (`*.scriber.bak`).

## How the connection works

The editor serves MCP at `http://127.0.0.1:47821/mcp` (the environment variable `SCRIBER_PORT`
changes the port). Only your own computer can reach it.

- Agents that take a URL are given that address.
- Agents that only start programs get `scriber mcp`, a small bridge that passes messages to the
  editor and starts the editor if it is not running.
- `scriber mcp --headless` runs a server without a window, for scripts and servers.

Agents keep working when you restart the editor.

## Windows and WSL

On Windows, Scriber also finds agents inside WSL distributions (for example Claude Code or Codex
installed in Ubuntu). When WSL can reach Windows' `127.0.0.1` (mirrored networking), the agent gets
the same URL; otherwise it gets the bridge through the Windows program. The agents window and
`scriber agents add` say which one was used.

## The Scriber skill

For Claude Code and Codex, **Connect** also installs the *Scriber skill*
(`~/.claude/skills/scriber/SKILL.md`, `~/.codex/skills/scriber/SKILL.md`). It teaches the agent how
to work with Scriber: units and layers, looking at the result often, leaving your selection alone,
following your review comments and getting a design ready for the laser. Connect updates it,
Remove deletes it. An agent with a missing or older skill shows as *outdated*; Connect again.

## Next

- [How agents work with Scriber](./working-with-agents)
- [MCP tool reference](./mcp-tools)
