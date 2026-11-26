# Share a library

A Scriber library file (`.scrlib`) carries machines and materials from one Scriber to another: to a
friend, to your other computer, or into a repository.

## Export

Laser → **Machines & materials…** → **Export ▾**:

| Choice | Exports |
|---|---|
| **Material "…"** | The selected material, with the machines its settings use |
| **Machine "…"** | The selected machine |
| **Whole library…** | All your machines and materials |

AI agents: `export_library`.

## Import

**Import ▾ → Scriber library file…** merges a file into your library:

- entries that are the same as yours are left alone;
- when an id is already taken by a different machine or material, you choose:
  - **Replace**: use the file's versions;
  - **Keep both**: import them under new ids (materials follow a renamed machine);
  - **Skip**: keep yours and import only the new ones;
- settings for machines that neither the file nor your library has are kept, and the import tells
  you which machines they are.

Press **Save** in the window afterwards. AI agents: `import_library`.

## The file

The file is JSON with the same machine and material fields as described in
[Make your own material library](./make-a-material-library):

```json
{
  "format": "scriber-library",
  "version": 1,
  "machines": [ ... ],
  "materials": [ ... ]
}
```

A file from a newer Scriber (higher `version`) is refused with a request to update.

## Publish and subscribe

To publish a library or subscribe to libraries others publish, see
[Library repositories](/library-repositories).
