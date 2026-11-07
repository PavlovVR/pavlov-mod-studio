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
| **Unreal Engine folder** | Already filled in for a normal install of Unreal Engine 5.1. If yours is elsewhere, select **Browse** and choose its `Engine` folder |
| **Staging folder** | Where [local test builds](#test-a-build-locally) go. It starts on the same folder as the modkit, so most people never change it |

Select **Save settings** when you're done. The app then shows Pavlov's EULA from your modkit, and you have to accept it before you can do anything else. It asks again if the EULA changes.

### Connect mod.io

The app uses a mod.io personal token, which you make on mod.io.

1. Select **Open mod.io**, which opens the **Personal access tokens** tab of your access page. Log in if you need to.
2. If mod.io asks you to agree to its API Access Terms, do that first. **Generate token** stays off until you do.
3. Select **Generate token** and fill in the form:

   | Field | What to choose |
   |---|---|
   | **Token name** | Anything, such as Mod Studio |
   | **My resources** | Leave it empty |
   | **Permissions** | Tick **All my mods** and **User actions** |
   | **Scope** | Tick **Read** and **Write**. Only Read is ticked at first, and that can't publish |
   | **Expiry** | **1 Year**. It starts on 30 Days |

4. Select **Generate** and copy the token. mod.io only shows it once.
5. Paste it into the app and select **Connect**.

The app checks the token with mod.io before keeping it. That only proves the token is real, not which boxes you ticked, so a token without Write still connects and then fails at your first change. It's encrypted for your Windows account and is only ever sent to mod.io.

When your token runs out the app says so, and you make a new one the same way.

### Add your username

If Settings asks for your **mod.io username** once you're connected, enter it and select **Save**. A personal token can't always show who it belongs to, so this is how the app knows which mods are yours. Without it, every mod in your modkit is shown, including ones you can't manage.

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
| **Share source with other modders** | Mods only. Also shares what's ticked in the [Source tab](#source-plugins) |
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

Each mod loads the first time you open it, and switching back to it is instant. **Refresh** reads the latest from mod.io, and rests for a few seconds after each click.

Files and comments load a page at a time, so a mod with thousands of them still opens quickly. Select **Show more** for the rest.

mod.io limits how many requests an account can make each minute. The app paces itself under that limit, and Settings shows how much of it you've used. If mod.io ever asks it to wait, the sidebar counts down and the app carries on by itself.

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

Select **+** at the top of the mod list, then fill in the name, summary and type. The logo is optional, and without one the mod gets Pavlov's default, the same as in the modkit. The app creates the mod on mod.io and sets up its plugin and definition in your modkit.

Pick the type carefully. It's built into the mod's files, so you can't change it later.

If Unreal is open, your new mod appears in it straight away, and the Content Browser goes to it. If Unreal can't load it while running, the app offers a restart instead, and Unreal asks about unsaved work first. The app also turns on **Show Plugin Content**, so your new mod shows up in the Content Browser.

## Source plugins

**Browse source plugins** at the bottom left lists content other modders have shared to build into your mods. **Download** puts a plugin in your modkit.

Each plugin says which platforms it's built for, because some are only for PC or only for Quest, and whether it's installed. **Update** appears when mod.io has a newer version, and it replaces your copy, including any changes you made. Your old copy moves to `Saved/Downloads` until the next download. **Installed** lists only what's in your modkit, so an update is easy to find. Your own mods say so, and are never offered as an update.

### Sharing your own

The **Source** tab on a content mod is where you tick exactly what other modders get. Maps and game modes can't be shared, the same as in the modkit.

Nothing is shared until you tick it. Only the files you tick leave your PC, plus the plugin's `.uplugin`, which a download can't be installed without. A new file is never ticked by itself, even in a folder you ticked. The zip is checked against your list before it's sent.

This is how you share part of a mod, such as the structs and interfaces other modders need, without handing over the rest.

| Control | What it does |
|---|---|
| **Clear** | Unticks everything |
| **Show** | Lists what your shared files need that isn't shared |
| **Add them** | Ticks what's needed, and what that needs in turn. Nothing is added until you select this |
| **Save as zip** | Saves exactly what would be shared, so you can look through it first |
| **Share on mod.io** | Shares the list now, without cooking or publishing a new version |

The tab tells you what's changed since your last share. It also warns when your files point at another mod, because whoever downloads them needs it too.

The shared copy is named `[SOURCE]` plus your mod's name, the same as other shared plugins. Your own plugin doesn't change.

To share with every release, tick **Share source with other modders** on the Publish tab. It sends the same list.

Older shares stay on mod.io, and what they hold can still be downloaded. If you once shared more than you meant to, the tab points you to them, and you can delete them from the Files tab. The newest share can't be deleted, because mod.io doesn't let a live file go.

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
| **Your mod.io token has expired** | Disconnect in Settings and connect with a new one |
| **Your mod.io token can't make changes** | Make a new token with **Read** and **Write** ticked |
| **Your mod.io token doesn't cover this** | Make a new token with **All my mods** and **User actions** ticked |
| **All mods are shown** | Add your mod.io username in Settings |
| **Waiting on mod.io's request limit** | Nothing. The app counts down and carries on by itself |
| **Settings saved, but check them** | A folder doesn't look right. The message says which one |
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
