# Changelog

Every version needs a section here before it can be released. It becomes that release's notes, so write it for modders: what changed for them, one line each, under Added, Changed, Fixed or Removed.

Add your changes under Unreleased. When a version is released, that section takes the version's number.

## Unreleased

### Added

- Settings shows how many mod.io requests the app has used in the last minute.
- Browse for the Unreal Engine folder, and a warning when a saved folder doesn't look right.

### Changed

- The app paces itself under mod.io's request limit. If mod.io ever asks it to wait, the sidebar counts down and the app carries on by itself.
- Launching, refreshing and saving settings use far fewer mod.io requests. A launch no longer reads every mod's files and team.
- Files and comments load a page at a time, with Show more for the rest.
- Refresh rests for a few seconds after each click.
- New app icon.
- Publish and Stage stay on screen in a small window.
- The mod.io username is only asked for when mod.io can't say who you are.

### Fixed

- The steps for making a mod.io token now match mod.io's current Personal access tokens page, and Open mod.io goes straight to it.
- Token errors now say which boxes to tick on mod.io.
- A publish can't start while an update is installing, which would have ended it.
- Stopping a run no longer shows a failed step.
- A build's size is read again after any run, not only a finished publish.
- Changing the file filter clears your selection, so Delete can't include files you can't see.
- Enter can't save a file with a blank version.
- A failed set up no longer offers to restart Unreal.
- Switching back to the app no longer runs UnrealPak.
- An old error no longer stays on Browse source plugins after a good read.

## 0.3.0

The first public release. If you have an older test build, install this one by hand once. After that it updates itself.

### Added

- Publish a mod to Windows, Linux, Oculus and Mac in one click: version bump, cook, package, upload and go live.
- Uploads in 50 MiB chunks. A failed chunk is retried on its own, and Resume finishes a stopped release without cooking it again.
- A live log of Unreal's output, with the step and build named when something fails, and Stop to end a cook partway.
- A check of every pak after cooking and before upload, which stops a build containing another mod's files.
- An Explorer-style Cook tab for choosing what goes into a build, with Filter only used, Always keep, search and undo.
- Other mods in your modkit count as using an asset, so a build warns before leaving out something they need.
- Publishing, cooking and staging with Unreal open or closed, with a prompt to save unsaved work first.
- Manage mods without the website: files and rollback, page details, visibility, permissions, screenshots, videos, comments, dependencies and metadata.
- Overview of downloads, ratings, recent activity and what's live on each platform, plus your team.
- Create a new mod with its plugin and definition, then restart Unreal in one click to load it.
- Browse and download source plugins, and share your own.
- Stage a build to a folder for local testing.
- See what takes up the space in each of the four builds.
- Connect with a mod.io personal token, with the steps shown in Settings and the token checked straight away.
- Automatic updates, signed and installed at launch. You can turn them off and check by hand in Settings.
- A command line version of everything above, for building from source.
