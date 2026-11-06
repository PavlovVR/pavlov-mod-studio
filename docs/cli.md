# Command line

Everything the app does is also available from a terminal, which is useful for scripts and automated builds. The app's installer doesn't include it, so build it from source:

```
cargo build --release -p pavlov-mod-studio
```

That gives you `target\release\pavlov-mod-studio.exe`. You'll need [Rust](https://rustup.rs) and the build tools listed in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Setup

Set your mod.io personal token before running anything that talks to mod.io:

```
set MODIO_TOKEN=<your token>
pavlov-mod-studio check-token
```

Commands that build need to know where the modkit is. Run them from the modkit folder, or add `--project`. If Unreal isn't in the default place, add `--engine`.

| Option | Default |
|---|---|
| `--project <folder>` | The current folder |
| `--engine <folder>` | `C:\Program Files\Epic Games\UE_5.1\Engine` |
| `-v`, `--verbose` | Prints all output from Unreal's tools, not just errors |

`<group>` means `pc`, `quest` or `both`. Each includes the client and its server.

## Publish

```
pavlov-mod-studio publish <mod id> <group> --project <modkit folder> --changelog "What changed"
```

| Option | What it does |
|---|---|
| `--changelog <text>` | The note shown with the file |
| `--inactive` | Uploads without making the files live |
| `--together` | Makes all platforms live together at the end |
| `--no-compress` | Stores paks uncompressed in the zip |
| `--share-source` | Also shares this mod's source |

If an upload stops, running the same command again resumes that release. It skips finished builds and sends only the missing chunks.

## Commands

### Build

| Command | What it does |
|---|---|
| `publish <mod> <group>` | Everything the modkit's Upload button does, from version bump to upload |
| `cook <mod> <group>` | Cooks with the same commands as the modkit |
| `upload <mod> <group>` | Uploads builds already cooked. Takes `--version`, `--changelog`, `--inactive`, `--no-compress`, `--skip-existing` and `--dry-run` |
| `stage <mod> <folder>` | Builds the PC client into a folder for offline testing |
| `analyze <pak>` | Shows what takes up the space in a pak |
| `set-version <mod> <version>` | Sets the version in a mod's definition, then confirms it from a fresh session |

### Choose what goes into a build

| Command | What it does |
|---|---|
| `assets <mod>` | Lists a mod's assets. Add `--analyze` to work out what's used, which runs Unreal |
| `exclude <mod> [paths]` | Leaves assets out. Paths are inside the mod's Content folder, without an extension |

`exclude` takes `--include` to put assets back, `--clear` to put everything back, `--confirm` to build even if kept assets point at left-out ones, and `--pin` or `--unpin` to always keep assets.

### Create and set up

| Command | What it does |
|---|---|
| `create <name> <kind> --summary <text> --logo <file>` | Creates a mod on mod.io and sets it up locally. `<kind>` is `Map`, `Mod` or `GameMode`. Add `--hidden` to keep the listing hidden |
| `setup-plugin <mod> <kind>` | Creates the local plugin and definition for a mod that's already on mod.io |

### Files and page

| Command | What it does |
|---|---|
| `mods [ids]` | Lists your mods and which ones you can manage. A personal token needs `--username` |
| `status <mod>` | Shows a mod's details and what's live on each platform |
| `files <mod>` | Lists a mod's files |
| `make-live <mod> <file>` | Makes a file live, which is also how you roll back |
| `edit-file <mod> <file>` | Changes a file's `--file-version` or `--changelog` |
| `delete-file <mod> <file>` | Deletes a file for good. A live file can't be deleted |
| `edit <mod> --name <text> --summary <text>` | Changes a mod's name and summary. Add `--logo <file>` for the logo |
| `page <mod>` | Changes `--description`, `--homepage`, `--name-id`, `--maturity`, `--community` or `--credits` |
| `hide <mod>` | Hides a mod from everyone but its team |
| `show <mod>` | Makes a hidden mod public |
| `close <mod> --yes` | Closes a mod. Only the mod.io website can restore it |

### Media, community and metadata

| Command | What it does |
|---|---|
| `add-images <mod> <files>` | Adds screenshots |
| `add-links <mod>` | Adds `--youtube` or `--sketchfab` links |
| `remove-media <mod>` | Removes `--image`, `--youtube` or `--sketchfab` items |
| `reorder-images <mod> <names>` | Sets the order, listing every image file name |
| `comments <mod>` | Lists comments |
| `comment <mod> <text>` | Posts a comment, or a reply with `--reply-to <id>` |
| `delete-comment <mod> <id>` | Deletes a comment |
| `team <mod>` | Lists the team |
| `activity <mod>` | Shows recent activity |
| `dependencies <mod>` | Lists dependencies. Add `--add <id>` or `--remove <id>` to change them |
| `metadata <mod>` | Lists metadata. Add `--add key:value` or `--remove key:value` to change it |

### Source plugins

| Command | What it does |
|---|---|
| `browse [search]` | Browses source plugins. Add `--sort` to change the order |
| `download <mod>` | Downloads a source plugin into the modkit |

### Unreal and your account

| Command | What it does |
|---|---|
| `check-token` | Checks that mod.io accepts the token in `MODIO_TOKEN` |
| `editor-status` | Shows whether Unreal is open and has unsaved work |
| `restart-editor` | Restarts Unreal, which is how it loads a new mod. It asks about unsaved work first |

Run `pavlov-mod-studio <command> --help` for every option.
