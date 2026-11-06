# What the modkit's Upload Mod button does

Observed from `CustomMapTools` behaviour, editor logs and mod.io data. This is the
behaviour the uploader has to match, and it contains no decompiled code.

## 1. Before the button
- Log in through the mod.io SDK.
- Pick a mod you own, then tick Windows, Android and optionally Source.

## 2. Checks and version
1. Refuse if another upload is running.
2. Refuse if the mod isn't found, or isn't in your owned list.
3. New version = current mod.io version + 1, with a floor of 1.
4. Load the definition by mod type (Map, Mod or GameMode), fail if missing.
5. Write the version into the definition and save the package.
6. Maps: Android needs `QuestMap`, Windows needs a map. GameModes need game logic.
7. Send the mod's `metadata_blob`: `ModType`, `EngineVersion`, `VersionUUID`.

## 3. Build, in this order
Runs in the background. Any failure shows a message box and clears the queue.

| Order | Build | enum | Command | Base release | Folder |
|---|---|---|---|---|---|
| 1 | Windows client | 0 | `PavlovUGCPak` | `Pavlov_1.0.0` | `Windows` |
| 2 | PC Linux server | 2 | `PavlovUGCPak`, `-Server -NoClient` | `Pavlov_1.0.0` | `LinuxPavServer` |
| 3 | Quest client | 10 | `BuildCookRun`, flavor ASTC | `PavlovShack_1.0.0` | `Android_ASTC` |
| 4 | Shack server | 1 | `BuildCookRun`, `-Server -NoClient` | `PavlovShack_1.0.0` | `LinuxShackServer` |

- The Windows switch builds 1 and 2. The Android switch builds 3 and 4.
- `NeverCookDir` is `/Game`, `/CustomMapTools`, `/JsonUtils`, `/VRFramework`, `/modio`,
  plus each other UGC plugin that passes a plugin check (likely enabled). The mod
  itself is left out.
- Each finished build is queued by platform.

## 4. Upload, one platform at a time
1. Write the file `metadata_blob`: `ModType`, `EngineVersion`.
2. Call SDK `SubmitNewModFileForMod` with folder, version, changelog, platform and
   active set. The SDK zips to `%TEMP%\modfile_<id>.zip` and uploads 50 MiB parts.
3. Wait for the SDK's finished event.
   - Success: drop that platform, wait 1 second, start the next.
   - Error 20489 at 100% progress: treated as success.
   - Any other error: message box, queue cleared, upload stops.
4. Queue empty: show "Upload Complete".

## Platform names on mod.io
Windows = `windows`, PC server = `linux`, Quest = `oculus`, Shack server = `mac`.
