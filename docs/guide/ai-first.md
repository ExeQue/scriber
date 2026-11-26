# Why AI-first

Scriber is made to be driven by an AI agent, with you watching and stepping in when you want to.
You tell an agent (Claude Code, Codex, Cursor, …) what you need. The agent draws it, adds
constraints, sets up layers and laser settings, and checks the result through Scriber's
[MCP tools](../agents/mcp-tools). Every change shows up live in your Scriber window.

The window is your view of the work and your way to steer it. It is not the only way in: anything
you can do there, the agent can do too, and the other way round.

## The loop

1. **Ask.** Tell the agent what you want, in plain words:
   - "Draw a 120 × 80 mm box lid with finger joints for 3 mm plywood, cut layer, and check it."
   - "Put my logo from `logo.svg` in the middle and engrave it at 30 % power."
   - "Make every hole 0.1 mm bigger to make up for the kerf."
2. **Watch.** The agent draws in the open document. You see shapes, layers and dimensions appear
   as it works. It renders a picture of the drawing to look at its own result.
3. **Step in.** Whenever you like:
   - pin a [review comment](./comments) on the canvas ("make this slot 5 mm", "round these
     corners"); the agent picks it up with `wait_for_annotations`, makes the change, replies and
     resolves it;
   - edit by hand with the full CAD toolset: [drawing](./drawing), [editing](./editing),
     [constraints](./constraints);
   - press <kbd>Ctrl+Z</kbd> to undo a change, the agent's or yours;
   - or take over entirely and finish the drawing yourself.
4. **Follow up.** The agent sees your edits and comments the next time it reads the document, and
   carries on from there.

## What the agent can do

Everything the window can do has an MCP tool: shapes and text, fillet, offset, boolean, arrays,
trim, constraints and named parameters, layers and laser operations, machines and material
settings, the laser preview and pre-flight check, SVG, PNG and xTool Studio export, and importing
from xTool Studio. See the [MCP tool reference](../agents/mcp-tools) for the full list.

## What stays yours

- **Your selection.** The agent does not change what you have selected, and shapes it adds are not
  selected. When you say "this one", it reads your selection.
- **Your tab.** With several documents open, the agent works on the active tab or names another
  one. It does not switch your tab unless you ask.
- **Undo is shared.** <kbd>Ctrl+Z</kbd> undoes the last change, whoever made it.
- **Your files.** The agent saves, opens and closes files only when you ask.
- **Locked and hidden layers** stay as you set them.

More in [How agents work with Scriber](../agents/working-with-agents).

## When to draw by hand

Sometimes it is quicker to do it yourself: nudging a shape into place, placing something by eye,
trying out a quick idea, or working without an agent at all. Scriber is a complete CAD program on
its own, with a command line, object snaps, grips and constraints. You can start by hand and ask an
agent to finish, or the other way round.

## Next

- [Connecting agents](../agents/connecting): connect Claude Code, Codex and others in one click.
- [Getting started](./getting-started): your first design, with an agent or by hand.
