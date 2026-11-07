# Contributing

Thanks for helping. Bug reports, ideas, documentation and code are all useful, and you don't need to write code to contribute.

## Report a bug or suggest an idea

[Open an issue](https://github.com/PavlovVR/pavlov-mod-studio/issues/new/choose) and pick a template. For bugs, the log from the run is the most useful thing you can include.

- **Never paste your mod.io token** into an issue, a log or a screenshot.
- Problems with mod.io itself or with Pavlov belong with them, not here.
- To report a security problem, follow [SECURITY.md](SECURITY.md) instead of opening an issue.

## Rules for this project

These apply to every change.

- **Mod content only.** Nothing that bypasses Pavlov's restrictions.
- **No decompiled code.** Behaviour comes from the public mod.io API and from observing the modkit.
- **Personal tokens only.** The app logs in with a mod.io personal token, and never with anything else. Never commit a token or a key.
- **Match the modkit first, then improve on it.** [docs/modkit-flow.md](docs/modkit-flow.md) is what it has to match.
- **Never weaken the content check.** Every pak is audited after cooking and before upload. The exclusion list is derived, never typed in by a user.

## Set up

You need:

- [Rust](https://rustup.rs)
- [Node.js](https://nodejs.org) 22
- Visual Studio Build Tools with the C++ workload
- The Windows SDK, with `rc.exe` on your `PATH`
- Git Bash, to run the checks
- The [Pavlov modkit](https://github.com/vankruptgames/PavlovVR-ModKit) and Unreal Engine 5.1, if you're testing real builds

Run the app:

```
cd app
npm ci
npm run tauri dev
```

## Project layout

| Folder | What's in it |
|---|---|
| `crates/core` | All the logic |
| `crates/cli` | The command line, a thin wrapper around core |
| `app` | The Tauri desktop app, also a thin wrapper around core |
| `tests` | Upload and management tests against a fake mod.io |
| `scripts` | Checks and release helpers |
| `docs` | User guide, command line reference and design notes |

New behaviour goes in `crates/core` and gets exposed from both the app and the command line. Before changing anything that spans several files, read [docs/architecture.md](docs/architecture.md). It explains why things are the way they are, including the mod.io behaviour that looks wrong but isn't.

## Make a change

1. For anything big, open an issue first, so you don't spend time on something that won't fit.
2. Create a branch from `main`.
3. Keep the pull request to one change.
4. Add tests for new behaviour in `crates/core`, or in `tests/` if it talks to mod.io.
5. Add a line under **Unreleased** in [CHANGELOG.md](CHANGELOG.md) if modders would notice the change, and update the docs if it changes what they see.
6. Run the checks. CI runs the same ones, and a pull request needs them to pass:

```
bash scripts/check.sh
```

Individual steps:

```
cargo test                                # all unit tests
cargo test -p pavlov-mod-core rejects     # tests matching a name
cargo clippy --all-targets -- -D warnings
cargo build --release -p pavlov-mod-studio && python tests/upload_flow.py && python tests/manage_flow.py
```

`tests/upload_flow.py` and `tests/manage_flow.py` run the release command line against a fake mod.io server, so build first. Nothing in the checks touches real mod.io.

Don't test against real mods unless you mean to. Publishing changes what players download.

### Style

- Rust is formatted with `cargo fmt` and has to pass clippy with no warnings.
- The frontend is plain TypeScript with no framework. Anything typed goes in `state`.
- The look is neutral: greys, blue only for focus and progress, green and red only for outcomes. It follows the Windows light or dark setting, so check both.
- Write interface text in plain, active language, and name things the way a modder would. Error messages say what happened and what to do next.
- Comments explain why, not what.

For interface changes, include a screenshot in your pull request.

## Review

A maintainer reviews every pull request. Expect questions, especially about anything that touches uploads, tokens, the content check or updates, because a mistake there affects other people's mods.

By contributing, you agree that your work is licensed under the [MIT licence](LICENSE).

Please be respectful. Criticise the code, not the person, and remember that everyone here is modding in their spare time. Harassment isn't tolerated, and maintainers may remove comments or block people who are abusive.

## Release

For maintainers.

1. Set the new version in `app/src-tauri/tauri.conf.json`, `Cargo.toml`, `app/package.json` and `app/package-lock.json`.
2. Rename **Unreleased** in `CHANGELOG.md` to that version, and add a new empty **Unreleased**.
3. Commit, then push a matching tag:

```
git tag v0.3.2
git push origin v0.3.2
```

GitHub tests it, builds the installer, signs it and publishes the release with the changelog as its notes. Installed apps pick it up from there.

Releases need the `TAURI_SIGNING_PRIVATE_KEY` repository secret. Keep a safe copy of the key: installed apps only accept updates signed with it, so if it's lost, everyone has to reinstall by hand.
