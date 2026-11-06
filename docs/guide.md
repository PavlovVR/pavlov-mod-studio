# User guide

Everything in Pavlov Mod Studio, tab by tab. If something's going wrong, jump to [Troubleshooting](#troubleshooting).

- [Install](#install)
- [Set up](#set-up)
- [Publish a mod](#publish-a-mod)
- [Choose what goes into a build](#choose-what-goes-into-a-build)
- [Test a build locally](#test-a-build-locally)
- [Manage a mod](#manage-a-mod)
- [Create a new mod](#create-a-new-mod)
- [Source plugins](#source-plugins)
- [Updates](#updates)
- [Troubleshooting](#troubleshooting)
- [Where things are stored](#where-things-are-stored)

## Install

You need:

- Windows 10 or 11
- The [Pavlov modkit](https://github.com/vankruptgames/PavlovVR-ModKit), and its [video series](https://www.youtube.com/playlist?list=PLsnITcSCPeMo37POY0NSJWHZ8qfH7tWHD) if you're new to it
- Unreal Engine 5.1
- A [mod.io](https://mod.io) account

Download the installer from the [latest release](https://github.com/PavlovVR/pavlov-mod-studio/releases/latest) and run it. It isn't code signed yet, so Windows SmartScreen may warn you. Choose **More info**, then **Run anyway**.

## Set up

Open **Settings** at the bottom left.

### Folders

| Folder | What it's for |
|---|---|
| **Modkit folder** | The folder containing `Pavlov.uproject` |
| **Unreal Engine folder** | Already filled in for a normal install of Unreal Engine 5.1 |
| **Staging folder** | Where local test builds go. You only need this for [local testing](#test-a-build-locally) |

Select **Save settings** when you're done.

### Connect mod.io

The app uses a mod.io personal token.

1. Select **Open mod.io**, which opens your access page. Log in if you need to.
2. Under OAuth access, create a token. Name it anything, and give it read and write access.
3. Copy the token, paste it into the app and select **Connect**.

The app checks the token with mod.io before keeping it. It's encrypted for your Windows account and is only ever sent to mod.io.

Tokens last a year. When yours runs out the app says so, and you make a new one the same way.

### Add your username

Once you're connected, enter your **mod.io username** and select **Save**. A personal token can't show who it belongs to, so this is how the app knows which mods are yours. Without it, every mod in your modkit is shown, including ones you can't manage.

## Publish a mod

Pick a mod from the list, then open **Publish**.

### Platforms

Each choice is a client and its server.

| Choice | Publishes | What they are |
|---|---|---|
| **PC** | Windows, Linux | PC client, PC server |
| **Quest** | Oculus, Mac | Quest client, Quest server |
| **Both** | All four | |

### Options

| Option | What it does |
|---|---|
| **Change note** | Shown with the file on mod.io |
| **Keep hidden after upload** | Uploads without going live. Make the files live later from the Files tab |
| **Go live on all platforms together** | Waits until every build has uploaded, instead of each going live as it finishes |
| **Skip compression** | Packages faster, but the upload is larger |
| **Share source with other modders** | Mods only. Lets others build this mod into theirs |
| **Show verbose log** | Opens the log with all of Unreal's output |

### What happens when you publish

The panel at the bottom shows each step:

1. **Check** makes sure the mod's definition is ready for the builds you chose.
2. **Version** works out the next version and saves it.
3. **Cook** builds each platform with Unreal, then checks every pak.
4. **Package** zips the builds.
5. **Upload** sends each build to mod.io in 50 MiB chunks.
6. **Go live** makes the builds live, if you chose to do that together.

Unreal can be open or closed. If you have unsaved work in Unreal, the app asks you to save it first, so it isn't left out of the build.

**Stop** ends the run. The **Log** button shows what's happening, and **Copy** puts the whole log on your clipboard, which helps if you need to report a bug.

### If an upload stops

A failed chunk is retried on its own. If the run stops anyway, the button changes to **Resume version 898** (with your version number):

- **Resume** skips finished builds and sends only the missing chunks, without cooking again.
- **Start over** throws away the cooked builds, and the next publish cooks everything again.

Resume is only offered when your platform and compression choices, and what you've left out of the build, are the same as before.

## Choose what goes into a build

Unreal cooks every asset in a mod's plugin, used or not. The **Cook** tab lets you leave some out. This only changes the build, and nothing is removed from your project.

### Filter only used

**Filter only used** works out what nothing uses, and leaves it out. The first time, it takes a moment to read Unreal's references.

Each asset then has a status:

| Status | Meaning | Filter only used |
|---|---|---|
| **Required** | The mod can't work without it | Keeps it |
| **Used** | Unreal's dependency tree reaches it from the mod's definition | Keeps it |
| **In a string** | Unreal's tree doesn't reach it, but another asset names it, so it may load while the game runs | Keeps it |
| **Other mods** | Another mod in your modkit uses it, or uses something that needs it | Keeps it |
| **Unused** | Nothing found uses it | Leaves it out |

> [!WARNING]
> Check your mod before publishing. An asset loaded by a name that's built while the game runs can't be seen, so it can look unused. Use **Always keep** for anything like that.

Mods outside your modkit can't be seen either. If one uses your assets, mark those **Always keep** too.

### Selecting by hand

The tree works like Windows Explorer:

| Action | How |
|---|---|
| Select | Click, Ctrl+click, Shift+click, or Ctrl+A for all |
| Move | Arrow keys |
| Include or leave out | Space, or the checkbox |
| Leave out | Delete |
| Always keep | P |
| Undo and redo | Ctrl+Z and Ctrl+Y |

**Always keep** protects an asset and everything it uses, even if nothing seems to use it.

The **All assets** menu narrows the list to what's left out, unused, only in a string, always kept or not in your last build. **Keep everything** puts every asset back.

If a kept asset points at something you left out, a warning appears with **Keep what they need**. If you publish anyway, the app asks you to confirm first.

## Test a build locally

**Stage for local testing** on the Publish tab builds the PC client into your staging folder without touching mod.io. You'll find it in `<staging folder>\UGC<mod id>\Data`.

## Manage a mod

Each mod loads once, so switching between mods is instant. **Refresh** reads the latest from mod.io.

| Tab | What you can do |
|---|---|
| **Overview** | See downloads, subscribers, rating, popularity, recent activity and what's live on each platform |
| **Files** | See every version. **Make live** rolls a build back to an older version. Edit a file's version or change note, download it, or delete it. mod.io can't delete a live file |
| **Page** | Edit the name, web address, summary, description, homepage and logo. Change visibility, community options, mature content flags, credits and permissions |
| **Media** | Add, reorder and remove screenshots, and add YouTube or Sketchfab links |
| **Comments** | Read, post, reply, edit your own and delete comments |
| **Team** | See who's on the team. Only the mod.io website can change it |
| **Advanced** | Manage dependencies and metadata, or close the mod |
| **Size** | See what takes up the space in each build, by folder and by largest file |

> [!CAUTION]
> **Close mod** on the Advanced tab removes the mod from the game and from this app. Only the mod.io website can restore it.

Mods you can manage that aren't in your modkit are listed under **Not in this modkit**. Use **Set up in modkit** on their Overview to publish them from here.

## Create a new mod

Select **+** at the top of the mod list, then fill in the name, summary, type and logo. The app creates the mod on mod.io and sets up its plugin and definition in your modkit.

Pick the type carefully. It's built into the mod's files, so you can't change it later.

Unreal only loads new plugins when it starts, so if it's open the app offers to restart it. Unreal asks about unsaved work first. The app also turns on **Show Plugin Content**, so your new mod shows up in the Content Browser.

## Source plugins

**Browse source plugins** at the bottom left lists content other modders have shared to build into your mods. **Download** puts a plugin in your modkit.

To share your own, tick **Share source with other modders** when you publish a mod.

## Updates

The **Updates** section in Settings shows your version.

- **Update automatically** is on by default. The app checks when it starts, then installs any new version and reopens.
- **Check for updates** checks straight away. If there's a new version, **Install version** installs it.

The app never updates during a run. If an update arrives while you're publishing, it waits, and **Settings** in the sidebar shows an Update badge.

Every update is signed, and the app refuses anything that doesn't match.

## Troubleshooting

| What you see | What to do |
|---|---|
| **mod.io didn't accept that token** | Copy it again without any extra spaces, or make a new one |
| **Your mod.io token has expired** | Tokens last a year. Disconnect in Settings and connect with a new one |
| **Your mod.io token can't make changes** | Make a new token with read and write access |
| **All mods are shown** | Add your mod.io username in Settings |
| **A new mod isn't in Unreal** | Restart Unreal, which is how it loads new plugins |
| **Unreal is open and has this mod's definition loaded, but it didn't answer** | Close Unreal and publish again |
| **A pak contains files from outside this mod** | The build picked up another mod's or the base game's files, so it stopped before uploading. Look for references to them in your mod |
| **Kept assets point at something you left out** | Select **Keep what they need**, or add those assets back in the Cook tab |
| **A cook failed** | Read Unreal's errors in the run panel, then select **Show log file** for everything else |
| **There's no build yet on the Size tab** | Publish the mod first, or cook that build |
| **Windows warns about the installer** | The app isn't code signed yet. Choose **More info**, then **Run anyway** |

Still stuck? [Open an issue](https://github.com/PavlovVR/pavlov-mod-studio/issues/new/choose) with the log from the run.

## Where things are stored

| What | Where |
|---|---|
| Settings and your encrypted token | `%APPDATA%\com.pavlovvr.modstudio` |
| What you've left out of each mod | `Saved\PavlovModStudio` in your modkit |
| Logs from each run | `Saved\Logs` in your modkit, such as `pavlov_cook_windows.log` |
