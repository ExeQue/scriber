# Copy and paste

| Keys | Does |
|---|---|
| <kbd>Ctrl+C</kbd> | Copy the selection |
| <kbd>Ctrl+X</kbd> | Cut the selection (one undo step) |
| <kbd>Ctrl+V</kbd> | Paste, centred at the pointer |
| <kbd>Ctrl+Shift+V</kbd> | Paste in place, at the original position |

Also in the Edit menu and the right-click menu (where *Paste* puts the copy at the point you
right-clicked).

- Copies go through the system clipboard, so they paste into another tab or another Scriber window.
- Pasted shapes get new ids and become the selection. They keep their layer (when it exists and is
  not locked; otherwise they go on the current layer), rotation, groups and images.
- Constraints between copied shapes come along. Parameter names that are already taken get the next
  free name.
- With the pointer off the canvas, the copy lands 10 mm right and down from the original.
- **SVG** text on the clipboard (copied from Inkscape or a text editor) is imported, centred at the
  pointer.

While you type in a text field, these keys work on the text instead.
