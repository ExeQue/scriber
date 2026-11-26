# How agents work with Scriber

An agent connected to Scriber works in the same document you have open. It is like a colleague at
your desk: it can draw, change and check things, and you see each change live.

## What an agent can do

Everything you can do in the window has an MCP tool: drawing shapes and text, the editing tools
(fillet, offset, boolean, arrays, trim, …), layers, constraints and parameters, machines and
materials, preview and check, import and export. It can also render a picture of the drawing to look
at its own work. The full list is in the [MCP tool reference](./mcp-tools).

Try asking things like:

- "Draw a 100 × 60 mm plate with a 6 mm hole 8 mm in from each corner, and lock the width."
- "Make the selected shapes 20 % smaller and put them on the score layer."
- "Set up the M2 with the 20 W laser and my birch plywood, then check if the design is ready."
- "Add my test results to the library: 3 mm acrylic cuts at 100 % and 4 mm/s."

## Ground rules the agent follows

- **Your selection stays yours.** Agent changes do not change what you have selected; shapes it adds
  are not selected. It only selects things when you ask it to show you something. When you say "this
  one", the agent reads your selection.
- **Tabs.** With several documents open, the agent works on the active tab unless it names another
  one. It opens and creates documents in new tabs and does not switch your tab unless you ask.
- **Your files.** It saves, opens and closes files only when you ask.
- **Undo is shared.** Ctrl+Z undoes the last change, whoever made it. The agent's own `undo` does
  the same, so it only undoes its own step right after making it.
- **Locked and hidden layers** stay as you set them; the agent cannot edit shapes on a locked layer.
- **Constraints hold.** If an agent's change would break a locked size or another constraint, it is
  refused, and the agent gets a message naming the conflict.

## The review loop

The easiest way to steer an agent is with [review comments](../guide/comments):

```
 you                         agent
  │  pin a comment            │
  │ ─────────────────────────▶│  wait_for_annotations returns it
  │                           │  makes the change, looks at the result
  │  reply + resolved         │
  │ ◀─────────────────────────│  reply_annotation (resolve: true)
  │  next comment …           │  wait_for_annotations again
```

1. Ask the agent to "follow my comments" (or "review mode").
2. The agent waits with `wait_for_annotations`. This call blocks until you add, reply to, resolve or
   reopen a comment, so nothing happens until you are ready.
3. Pin comments on the canvas: "make this hole 5 mm", "round these corners".
4. The agent makes each change, replies saying what it did, and resolves the comment. If a comment
   is unclear it asks in a reply and leaves it open.
5. Reopen a comment if you are not happy; the agent picks it up again.

Agents can also leave comments for you (`add_annotation`), for example to point out something it
was unsure about.

## Without a window

`scriber mcp --headless` runs the same tools without the editor, with one empty document. Useful for
scripts that generate designs and export them.
