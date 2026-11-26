# The recent-projects file

Every Zen IDE remembers the projects it opened in one JSON file. The IDE shows the list on its
Welcome screen and in its Recent Projects menu, and opens the projects that were still open when it
quit at the next start. Zen Toolbox reads the files of all IDEs to list their projects in its
Projects tab ([below](#zen-toolbox)). The format is shared between these programs, so it changes only as described in
[Compatibility](#compatibility) ([ADR-0041](adr/0041-recent-projects-file.md)).

The code is `zen_project::recent` (`crates/platform/project/src/recent.rs`).

## Where the file is

One file per IDE, in a folder named after the IDE's application id, inside the user's application
data folder:

| System | Path |
|---|---|
| macOS | `~/Library/Application Support/<app id>/recent-projects.json` |
| Windows | `%APPDATA%\<app id>\recent-projects.json` |
| Linux and others | `$XDG_DATA_HOME/<app id>/recent-projects.json`, or `~/.local/share/<app id>/recent-projects.json` when `XDG_DATA_HOME` is not set |

| IDE | Application id |
|---|---|
| GoGarden | `space.ifzenelse.gogarden` |
| RustRonin | `space.ifzenelse.rustronin` |
| WebWave | `space.ifzenelse.webwave` |
| JvmJizo | `space.ifzenelse.jvmjizo` |
| SqlScroll | `space.ifzenelse.sqlscroll` |
| DevopsDaruma | `space.ifzenelse.devopsdaruma` |

When the environment variable `ZEN_DATA_DIR` is set and not empty, it replaces the application data
folder on every system: the file is `$ZEN_DATA_DIR/<app id>/recent-projects.json`. Tests and UI
snapshots use it to keep away from the user's real data. `zen_project::recent::default_file(app_id)`
returns the path; `zen_project::recent::read(app_id)` reads the list;
`zen_project::recent::file_in(data_dir, app_id)` gives the file inside a known application data
folder.

## Format

A UTF-8 JSON object with one field, `projects`: the remembered projects, most recently opened first,
at most 30. Each project is an object:

| Field | Type | Meaning |
|---|---|---|
| `path` | string | The project's root folder, an absolute path. The folder may no longer exist (a disconnected drive, a deleted project); such entries are kept and shown as missing. |
| `opened` | integer | When the project was last opened, in seconds since the Unix epoch (UTC). |
| `open` | boolean | Whether the project was open when the IDE last quit, or is open now. |

An example:

```json
{
  "projects": [
    {
      "path": "/Users/ada/src/garden",
      "opened": 1792224000,
      "open": true
    },
    {
      "path": "/Users/ada/src/notes",
      "opened": 1792137600,
      "open": false
    }
  ]
}
```

The test `the_documented_example_is_the_format` in `recent.rs` parses this example and checks that
the IDE writes the same text for it, so the example and the code cannot drift apart.

## Writers and readers

- **The writer is the IDE that owns the file, and only it.** The IDE rewrites the whole file each
  time the list changes, atomically (a temporary file renamed over the old one, `zen_vfs::write_atomic`),
  so a reader never sees a half-written file. Several windows of one IDE share one process and one
  list. A second copy of a running IDE (Toolbox starts one when it opens a project, see
  [release-manifest.md](release-manifest.md)) keeps its own list in memory, and the copy that writes
  last wins; the file stays valid either way.
- **Readers, such as Zen Toolbox, only read.** They never write, rename or delete the file. To open a
  project, Toolbox starts the owning IDE with the project folder as an argument, and the IDE records
  the opening itself.
- A missing, empty or unreadable file reads as an empty list. A reader shows no error for it.

### Zen Toolbox

Zen Toolbox reads the files of every product in its catalog, installed or not, through two commands
in `zen-toolbox` (`crates/platform/toolbox/src/projects.rs`):

- `toolbox.projects.list { "query"?: string }` returns the projects, newest first. A folder listed
  by several IDEs is one row, owned by the IDE that opened it most recently (`toolId`, `toolName`,
  `opened`); the other IDEs are in `alsoIn`, most recent first. `exists` tells whether the folder is
  a directory now; missing projects stay in the list. `query` keeps the rows whose name or path
  contains it, ignoring case.
- `toolbox.project.open { "path": string, "toolId"?: string }` starts the IDE with the folder, the
  same way as `toolbox.tool.launch` with `path`. Without `toolId` it uses the IDE that opened the
  folder most recently. It fails for a missing folder, for a folder that no IDE lists when `toolId`
  is not given, and for an IDE that is not installed.

Toolbox looks for the files where the IDEs do, `ZEN_DATA_DIR` included
(`zen_toolbox::Config::data_dir` can name the folder instead, for tests). Zen Toolbox has no
automation endpoint yet, so agents reach these commands through its command registry, like every
other `toolbox.*` command.

## Compatibility

- Readers ignore fields they do not know, at the top level and in each project.
- The format changes only by adding fields. An added field has a default that keeps the old meaning,
  so a new reader reads an old file and an old reader reads a new file.
- Renaming or removing a field, changing its type or meaning, or moving the file needs a new ADR and
  a new file name.
