# Machines and materials

Scriber knows your laser cutter (its work area and laser modules) and keeps a **material library**:
the power, speed and passes that work for each material. Exports and the pre-flight check use it.

Scriber ships with machine profiles for many xTool machines, but **no material settings**: you build
your own library, import it from xTool Studio or get it from someone else.

## Pick the machine and material for a document

Laser tab → **Machine & material**:

1. the **machine** button: your laser cutter;
2. the **laser** module (for example *10 W blue laser*);
3. the **material** button: opens a picker with search and a filter by kind (wood, acrylic, …). The
   tags **C S E** next to each material show whether it has settings for cut, score and engrave on
   this machine and laser (filled = yes, outlined = no).

**Use work area** sets the material size to the machine's work area.

The choice is saved in the document. The library itself lives on your computer and is shared by all
documents.

## The Machines & materials window

Laser tab → **Machines & materials…** opens the library, with two pages.

### Machines

A list on the left, the selected machine's form on the right: name, brand, model, work area and its
laser modules (type, power in watts, what each can do: cut, score, engrave). You can change the
built-in machines; your version wins over the built-in one.

![The Machines & materials window on the Machines page: the machine list, and the form for the xTool M2 with its laser modules](/screenshots/library-machines.webp)

*The Machines page.*

### Materials

Per material: name, **kind** (paper & card, wood, plywood & MDF, acrylic, leather, fabric & felt,
metal, stone & tile, glass, other; used to sort and filter), thickness, **colour** (the sheet's
colour in the laser preview), notes and where the numbers came from.

Below that, the **laser settings** for one machine and laser at a time:

- pick the machine and laser; *Also set up for* shows the other machines that have settings;
- per operation (Cut, Score, Engrave): power %, speed mm/s, passes, and for engrave the **lines per
  cm** and **Both ways** (engrave in both directions);
- **Add profile** adds a named setting next to the default one, for example *Light*, *Deep* or
  *Photo*. Layers can then pick a profile (see [Layers](./layers#laser-settings-per-layer));
- **Copy from…** copies all profiles from another machine or laser as a starting point.

Press **Save** to keep your changes. Every delete asks first.

### Import and Export

- **Import ▾ → From xTool Studio…** and **xTool project (.xs)…**: see
  [Importing from xTool Studio](../importing-xtool-materials).
- **Import ▾ → Scriber library file…** and **Export ▾**: see [Share a library](../library/share-a-library).

## Where the library is stored

Your machines and materials are kept in `library.json` in Scriber's config folder
(`~/.config/scriber/` on Linux, `%APPDATA%\scriber\` on Windows). Set the environment variable
`SCRIBER_LIBRARY` to use another file. It only holds what you added or changed; unchanged built-in
machines always come fresh with the app.

To write materials by hand, see [Make your own material library](../library/make-a-material-library).
