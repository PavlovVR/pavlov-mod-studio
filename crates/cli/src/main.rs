use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use pavlov_mod_core::api::{Api, API_BASE};
use pavlov_mod_core::assets::{
    analyze_assets, asset_report, check_selection, load_selection, save_selection,
};
use pavlov_mod_core::community;
use pavlov_mod_core::cook::{cook, stage_local, CookOptions};
use pavlov_mod_core::create::{create_mod, setup_plugin, ModKind, NewMod, Workspace};
use pavlov_mod_core::definition::write_version;
use pavlov_mod_core::editor::{editor_state, restart_editor};
use pavlov_mod_core::eula::{accept_eula, read_eula};
use pavlov_mod_core::library::{check_token, load_library, mod_status, AccessMemory};
use pavlov_mod_core::manage::{self, all_files, edit_details, make_live, set_logo, ModEdit};
use pavlov_mod_core::package::analyze_pak;
use pavlov_mod_core::pipeline::{self, Release};
use pavlov_mod_core::progress::set_verbose;
use pavlov_mod_core::share::{check_share, save_share, share_report, share_zip};
use pavlov_mod_core::source::{browse, download};
use pavlov_mod_core::targets::{group, Target};
use pavlov_mod_core::upload::{prepare, publish, share_now, Options, PART_SIZE};

const DEFAULT_ENGINE: &str = r"C:\Program Files\Epic Games\UE_5.1\Engine";

#[derive(Parser)]
#[command(
    name = "pavlov-mod-studio",
    version,
    about = "Resumable mod publishing for Pavlov"
)]
struct Cli {
    /// Print all output from the Unreal tools, not just errors
    #[arg(long, short, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Do everything the modkit's Upload button does, from version bump to upload.
    Publish(PublishArgs),
    /// Show a mod's details and what's live on each platform.
    Status(ModArg),
    /// List a mod's files.
    Files(ModArg),
    /// Make a file live. This is also how a release is rolled back.
    MakeLive(MakeLiveArgs),
    /// Change a mod's name, summary or logo.
    Edit(EditArgs),
    /// List your mods and which ones you can manage.
    Mods(ModsArgs),
    /// Change a mod's page: description, homepage, web address, options.
    Page(PageArgs),
    /// Hide a mod from everyone but its team.
    Hide(ModArg),
    /// Make a hidden mod public.
    Show(ModArg),
    /// Close a mod. Only the mod.io website can restore it.
    Close(CloseArgs),
    /// Change a file's version label or changelog.
    EditFile(EditFileArgs),
    /// Delete a file for good. A live file can't be deleted.
    DeleteFile(MakeLiveArgs),
    /// Add screenshots to a mod's gallery.
    AddImages(AddImagesArgs),
    /// Add YouTube or Sketchfab links to a mod's gallery.
    AddLinks(LinksArgs),
    /// Remove screenshots or links from a mod's gallery.
    RemoveMedia(RemoveMediaArgs),
    /// Set the order of a mod's screenshots.
    ReorderImages(ReorderArgs),
    /// List, add or remove a mod's dependencies.
    Dependencies(DependenciesArgs),
    /// List, add or remove a mod's metadata pairs.
    Metadata(MetadataArgs),
    /// List a mod's team.
    Team(ModArg),
    /// List a mod's comments.
    Comments(CommentsArgs),
    /// Post a comment, or a reply with --reply-to.
    Comment(CommentArgs),
    /// Delete a comment.
    DeleteComment(DeleteCommentArgs),
    /// Show a mod's recent activity.
    Activity(ModArg),
    /// List a mod's assets and what uses them.
    Assets(AssetsArgs),
    /// Choose assets to leave out of a mod's builds.
    Exclude(ExcludeArgs),
    /// Choose exactly what a mod shares with other modders, check it, and share it.
    Share(ShareArgs),
    /// Browse source plugins.
    Browse(BrowseArgs),
    /// Download a source plugin into the modkit.
    Download(DownloadArgs),
    /// Build the PC client into a folder for offline testing.
    Stage(StageArgs),
    /// Show what takes up the space in a pak.
    Analyze(AnalyzeArgs),
    /// Create a new mod on mod.io and set it up locally.
    Create(CreateArgs),
    /// Create the local plugin and definition for a mod that already exists on mod.io.
    SetupPlugin(SetupArgs),
    /// Set the version in a mod's definition, then confirm it from a fresh session.
    SetVersion(SetVersionArgs),
    /// Show Pavlov's EULA from the modkit, or accept it with --accept. Nothing that uses the modkit runs until then.
    Eula(EulaArgs),
    /// Check that mod.io accepts the token in MODIO_TOKEN.
    CheckToken,
    /// Show whether Unreal is open and has unsaved work.
    EditorStatus(EditorArgs),
    /// Restart Unreal, which is how it loads a new mod. It asks about unsaved work first.
    RestartEditor(EditorArgs),
    /// Cook a mod with the same commands as the modkit.
    Cook(CookArgs),
    /// Upload builds already cooked by the modkit.
    Upload(UploadArgs),
}

#[derive(clap::Args)]
struct PublishArgs {
    #[command(flatten)]
    cook: CookArgs,
    #[arg(long, default_value = "")]
    changelog: String,
    /// Upload without making the files live
    #[arg(long)]
    inactive: bool,
    /// Store paks uncompressed in the zip
    #[arg(long)]
    no_compress: bool,
    /// Make all platforms live together at the end
    #[arg(long)]
    together: bool,
    /// Also share what's on this mod's share list
    #[arg(long)]
    share_source: bool,
    #[arg(long, default_value = API_BASE, hide = true)]
    api_base: String,
}

#[derive(clap::Args)]
struct EulaArgs {
    /// Accept the EULA shown by this command
    #[arg(long)]
    accept: bool,
    #[arg(long, default_value = ".")]
    project: PathBuf,
}

#[derive(clap::Args)]
struct ShareArgs {
    mod_id: u64,
    /// Paths inside the plugin folder, such as Content/Api/BP_Api.uasset. Adds them to the list
    paths: Vec<String>,
    /// Take these paths off the list
    #[arg(long)]
    remove: bool,
    /// Empty the list
    #[arg(long)]
    clear: bool,
    /// Show what these paths, or the list, point at that isn't shared. Changes nothing
    #[arg(long)]
    check: bool,
    /// Write the zip here instead of sharing it
    #[arg(long)]
    export: Option<PathBuf>,
    /// Share the list on mod.io
    #[arg(long)]
    upload: bool,
    #[arg(long, default_value = "")]
    changelog: String,
    #[arg(long, default_value = ".")]
    project: PathBuf,
}

#[derive(clap::Args)]
struct CommentsArgs {
    mod_id: u64,
    /// Skip this many of the newest comments.
    #[arg(long, default_value_t = 0)]
    offset: u64,
}

#[derive(clap::Args)]
struct ModArg {
    mod_id: u64,
}

#[derive(clap::Args)]
struct AssetsArgs {
    mod_id: u64,
    /// Work out what's used. This runs Unreal, so it takes a moment
    #[arg(long)]
    analyze: bool,
    #[arg(long, default_value = ".")]
    project: PathBuf,
    #[arg(long, default_value = DEFAULT_ENGINE)]
    engine: PathBuf,
}

#[derive(clap::Args)]
struct ExcludeArgs {
    mod_id: u64,
    /// Asset paths inside the mod's Content folder, without an extension
    paths: Vec<String>,
    /// Put these assets back
    #[arg(long)]
    include: bool,
    /// Put every asset back
    #[arg(long)]
    clear: bool,
    /// Build even if kept assets point at left-out ones
    #[arg(long)]
    confirm: bool,
    /// Always keep these assets, for when something uses them that can't be detected
    #[arg(long)]
    pin: bool,
    /// Stop always keeping these assets
    #[arg(long)]
    unpin: bool,
    #[arg(long, default_value = ".")]
    project: PathBuf,
}

#[derive(clap::Args)]
struct ModsArgs {
    /// Mods in your modkit, by id
    ids: Vec<u64>,
    /// Needed with a personal token, which can't ask mod.io who it belongs to
    #[arg(long)]
    username: Option<String>,
}

#[derive(clap::Args)]
struct PageArgs {
    mod_id: u64,
    #[arg(long)]
    description: Option<String>,
    #[arg(long)]
    homepage: Option<String>,
    /// The name used in the mod's web address
    #[arg(long)]
    name_id: Option<String>,
    /// Mature content flags, as mod.io's bitmask
    #[arg(long)]
    maturity: Option<u64>,
    /// Community options, as mod.io's bitmask
    #[arg(long)]
    community: Option<u64>,
    /// Credit and permission options, as mod.io's bitmask
    #[arg(long)]
    credits: Option<u64>,
}

#[derive(clap::Args)]
struct CloseArgs {
    mod_id: u64,
    /// Confirm that this mod should be closed
    #[arg(long)]
    yes: bool,
}

#[derive(clap::Args)]
struct EditFileArgs {
    mod_id: u64,
    file_id: i64,
    #[arg(long)]
    file_version: Option<String>,
    #[arg(long)]
    changelog: Option<String>,
}

#[derive(clap::Args)]
struct AddImagesArgs {
    mod_id: u64,
    #[arg(required = true)]
    images: Vec<PathBuf>,
}

#[derive(clap::Args)]
struct LinksArgs {
    mod_id: u64,
    #[arg(long)]
    youtube: Vec<String>,
    #[arg(long)]
    sketchfab: Vec<String>,
}

#[derive(clap::Args)]
struct RemoveMediaArgs {
    mod_id: u64,
    /// Image file names
    #[arg(long)]
    image: Vec<String>,
    #[arg(long)]
    youtube: Vec<String>,
    #[arg(long)]
    sketchfab: Vec<String>,
}

#[derive(clap::Args)]
struct ReorderArgs {
    mod_id: u64,
    /// Every image file name, in the new order
    #[arg(required = true)]
    images: Vec<String>,
}

#[derive(clap::Args)]
struct DependenciesArgs {
    mod_id: u64,
    #[arg(long)]
    add: Vec<u64>,
    #[arg(long)]
    remove: Vec<u64>,
}

#[derive(clap::Args)]
struct MetadataArgs {
    mod_id: u64,
    /// key:value
    #[arg(long)]
    add: Option<String>,
    /// key:value
    #[arg(long)]
    remove: Option<String>,
}

#[derive(clap::Args)]
struct CommentArgs {
    mod_id: u64,
    content: String,
    #[arg(long)]
    reply_to: Option<u64>,
}

#[derive(clap::Args)]
struct DeleteCommentArgs {
    mod_id: u64,
    comment_id: u64,
}

#[derive(clap::Args)]
struct MakeLiveArgs {
    mod_id: u64,
    file_id: i64,
}

#[derive(clap::Args)]
struct EditArgs {
    mod_id: u64,
    #[arg(long)]
    name: String,
    #[arg(long)]
    summary: String,
    #[arg(long)]
    logo: Option<PathBuf>,
}

#[derive(clap::Args)]
struct BrowseArgs {
    #[arg(default_value = "")]
    search: String,
    #[arg(long, default_value = "-date_live")]
    sort: String,
}

#[derive(clap::Args)]
struct DownloadArgs {
    mod_id: u64,
    #[arg(long, default_value = ".")]
    project: PathBuf,
}

#[derive(clap::Args)]
struct StageArgs {
    mod_id: u64,
    /// Folder to build into
    folder: PathBuf,
    #[arg(long, default_value = ".")]
    project: PathBuf,
    #[arg(long, default_value = DEFAULT_ENGINE)]
    engine: PathBuf,
}

#[derive(clap::Args)]
struct AnalyzeArgs {
    /// A cooked .pak file
    pak: PathBuf,
    #[arg(long, default_value = DEFAULT_ENGINE)]
    engine: PathBuf,
}

#[derive(clap::Args)]
struct CreateArgs {
    name: String,
    /// Map, Mod or GameMode
    kind: String,
    #[arg(long)]
    summary: String,
    /// PNG or JPG logo
    #[arg(long)]
    logo: PathBuf,
    /// Create the listing hidden, so only your team can see it
    #[arg(long)]
    hidden: bool,
    #[arg(long, default_value = ".")]
    project: PathBuf,
    #[arg(long, default_value = DEFAULT_ENGINE)]
    engine: PathBuf,
}

#[derive(clap::Args)]
struct SetupArgs {
    mod_id: u64,
    /// Map, Mod or GameMode
    kind: String,
    #[arg(long, default_value = ".")]
    project: PathBuf,
    #[arg(long, default_value = DEFAULT_ENGINE)]
    engine: PathBuf,
}

#[derive(clap::Args)]
struct EditorArgs {
    #[arg(long, default_value = ".")]
    project: PathBuf,
    #[arg(long, default_value = DEFAULT_ENGINE)]
    engine: PathBuf,
}

#[derive(clap::Args)]
struct SetVersionArgs {
    mod_id: u64,
    version: i64,
    #[arg(long, default_value = ".")]
    project: PathBuf,
    #[arg(long, default_value = DEFAULT_ENGINE)]
    engine: PathBuf,
    /// Asset to change, instead of the mod's definition
    #[arg(long, hide = true)]
    asset: Option<String>,
}

#[derive(clap::Args)]
struct CookArgs {
    mod_id: u64,
    /// pc, quest or both; each includes client and server
    group: String,
    /// The modkit folder containing Pavlov.uproject
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// The Unreal Engine folder
    #[arg(long, default_value = DEFAULT_ENGINE)]
    engine: PathBuf,
}

#[derive(clap::Args)]
struct UploadArgs {
    mod_id: u64,
    /// pc, quest or both; each includes client and server
    group: String,
    /// The modkit folder containing Pavlov.uproject
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Set this version on the mod, as the modkit does before cooking
    #[arg(long)]
    version: Option<String>,
    #[arg(long, default_value = "")]
    changelog: String,
    /// Upload without making the files live
    #[arg(long)]
    inactive: bool,
    /// Store paks uncompressed in the zip
    #[arg(long)]
    no_compress: bool,
    /// Skip builds already on mod.io at this version
    #[arg(long)]
    skip_existing: bool,
    /// Check and zip only; no network
    #[arg(long)]
    dry_run: bool,
    #[arg(long, default_value = API_BASE, hide = true)]
    api_base: String,
    #[arg(long, default_value_t = PART_SIZE, hide = true)]
    part_size: u64,
    #[arg(long, default_value_t = 10, hide = true)]
    poll_seconds: u64,
    #[arg(long, hide = true)]
    skip_audit: bool,
    #[arg(long, default_value = DEFAULT_ENGINE)]
    engine: PathBuf,
}

fn upload(args: UploadArgs) -> Result<()> {
    let targets = group(&args.group)?;
    let options = Options {
        project: args.project,
        engine: args.engine,
        mod_id: args.mod_id,
        version: args.version,
        changelog: args.changelog,
        inactive: args.inactive,
        compress: !args.no_compress,
        skip_existing: args.skip_existing,
        together: false,
        part_size: args.part_size,
        poll_seconds: args.poll_seconds,
        skip_audit: args.skip_audit,
    };
    let zips = prepare(&options, targets)?;
    if args.dry_run {
        println!("dry run: nothing uploaded");
        return Ok(());
    }
    let token = std::env::var("MODIO_TOKEN").context("set MODIO_TOKEN to your mod.io token")?;
    publish(&Api::new(&args.api_base, &token)?, &options, &zips)
}

fn cook_options(args: CookArgs) -> Result<(CookOptions, &'static [Target])> {
    let targets = group(&args.group)?;
    let options = CookOptions {
        project: args.project,
        engine: args.engine,
        mod_id: args.mod_id,
    };
    Ok((options, targets))
}

fn definition_asset(mod_id: u64) -> String {
    format!("/UGC{mod_id}/definition")
}

fn token_api() -> Result<Api> {
    let token = std::env::var("MODIO_TOKEN").context("set MODIO_TOKEN to your mod.io token")?;
    let base = std::env::var("PMS_API_BASE").unwrap_or_else(|_| API_BASE.to_string());
    Api::new(&base, &token)
}

fn run_publish(args: PublishArgs) -> Result<()> {
    let token = std::env::var("MODIO_TOKEN").context("set MODIO_TOKEN to your mod.io token")?;
    let api = Api::new(&args.api_base, &token)?;
    let (cook, targets) = cook_options(args.cook)?;
    let release = Release {
        cook,
        changelog: args.changelog,
        inactive: args.inactive,
        compress: !args.no_compress,
        together: args.together,
        share_source: args.share_source,
    };
    pipeline::run(&api, release, targets)
}

fn share(args: ShareArgs) -> Result<()> {
    let report = share_report(&args.project, args.mod_id)?;
    if args.check {
        let files = if args.paths.is_empty() {
            report.files
        } else {
            args.paths
        };
        let check = check_share(&args.project, args.mod_id, &files)?;
        for link in &check.missing {
            println!("{} needs {}", link.from, link.needs);
        }
        for path in &check.add {
            println!("to add: {path}");
        }
        for need in &check.plugins {
            println!(
                "points at {}: {} shared assets, such as {}",
                need.plugin, need.assets, need.from
            );
        }
        if check.add.is_empty() && check.plugins.is_empty() {
            println!("Nothing shared points at anything missing.");
        }
        return Ok(());
    }
    let mut files = report.files;
    if args.clear {
        files.clear();
    } else if args.remove {
        files.retain(|path| !args.paths.contains(path));
    } else {
        if let Some(path) = args
            .paths
            .iter()
            .find(|path| !report.items.iter().any(|item| item.path == **path))
        {
            bail!("{path} isn't in this mod, or can't be shared");
        }
        files.extend(args.paths.iter().cloned());
    }
    if args.clear || !args.paths.is_empty() {
        save_share(&args.project, args.mod_id, &files)?;
    }
    if let Some(path) = &args.export {
        let made = share_zip(&args.project, args.mod_id, "", path)?;
        println!(
            "{}: {} files, {} bytes",
            path.display(),
            made.files,
            made.bytes
        );
    } else if args.upload {
        share_now(&token_api()?, &args.project, args.mod_id, args.changelog)?;
    } else if files.is_empty() {
        println!("The share list is empty.");
    } else {
        println!("{}", report.descriptor);
        for path in &files {
            println!("{path}");
        }
    }
    Ok(())
}

fn set_visible(mod_id: u64, visible: bool) -> Result<()> {
    let edit = ModEdit {
        visible: Some(visible),
        ..ModEdit::default()
    };
    manage::edit_mod(&token_api()?, mod_id, &edit)?;
    println!(
        "Mod {mod_id} is {}",
        if visible { "public" } else { "hidden" }
    );
    Ok(())
}

fn split_pair(pair: &str) -> Result<(&str, &str)> {
    pair.split_once(':').context("use key:value")
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    set_verbose(cli.verbose);
    match cli.command {
        Command::Assets(args) => {
            let report = if args.analyze {
                analyze_assets(&args.project, &args.engine, args.mod_id)?
            } else {
                asset_report(&args.project, args.mod_id)?
            };
            for consumer in &report.consumers {
                println!(
                    "used by mod {}: {} assets",
                    consumer.mod_id, consumer.assets
                );
            }
            for asset in &report.assets {
                let left_out = report.excluded.contains(&asset.path);
                println!(
                    "{}  {} bytes  {:?}{}",
                    asset.path,
                    asset.size,
                    asset.status,
                    if left_out { "  left out" } else { "" }
                );
            }
            Ok(())
        }
        Command::Eula(args) => {
            if args.accept {
                accept_eula(&args.project)?;
                println!("Accepted.");
            } else {
                let eula = read_eula(&args.project)?;
                println!("{}", eula.text);
                println!("Accepted: {}", if eula.accepted { "yes" } else { "no" });
            }
            Ok(())
        }
        Command::Share(args) => share(args),
        Command::Exclude(args) => {
            let mut selection = load_selection(&args.project, args.mod_id)?;
            if args.pin {
                selection.pinned.extend(args.paths.iter().cloned());
                selection.pinned.sort();
                selection.pinned.dedup();
                selection.excluded.retain(|path| !args.paths.contains(path));
            } else if args.unpin {
                selection.pinned.retain(|path| !args.paths.contains(path));
            } else if args.clear {
                selection.excluded.clear();
            } else if args.include {
                selection.excluded.retain(|path| !args.paths.contains(path));
            } else {
                let pinned = selection.pinned.clone();
                selection
                    .excluded
                    .extend(args.paths.iter().filter(|p| !pinned.contains(p)).cloned());
                selection.excluded.sort();
                selection.excluded.dedup();
            }
            selection.confirmed = args.confirm;
            save_selection(&args.project, args.mod_id, &selection)?;
            let saved = load_selection(&args.project, args.mod_id)?;
            println!("{} assets left out", saved.excluded.len());
            for missing in check_selection(&args.project, args.mod_id)? {
                println!("{} uses {}", missing.kept, missing.missing);
            }
            Ok(())
        }
        Command::Mods(args) => {
            let mut memory = AccessMemory::default();
            let library = load_library(
                &token_api()?,
                &args.ids,
                args.username.as_deref(),
                &mut memory,
            )?;
            if let Some(note) = &library.note {
                println!("note: {note}");
            }
            for item in &library.mods {
                let access = if !library.access_known {
                    "unknown"
                } else if library.owned.contains(&item.id) {
                    "yours"
                } else {
                    "not yours"
                };
                println!(
                    "{}  {}  by {}  {} downloads  {} subscribers  {access}",
                    item.id,
                    item.name,
                    item.author,
                    item.stats.downloads_total,
                    item.stats.subscribers
                );
            }
            Ok(())
        }
        Command::Page(args) => {
            let edit = ModEdit {
                description: args.description,
                homepage_url: args.homepage,
                name_id: args.name_id,
                maturity_option: args.maturity,
                community_options: args.community,
                credit_options: args.credits,
                ..ModEdit::default()
            };
            manage::edit_mod(&token_api()?, args.mod_id, &edit)?;
            println!("Page saved");
            Ok(())
        }
        Command::Hide(args) => set_visible(args.mod_id, false),
        Command::Show(args) => set_visible(args.mod_id, true),
        Command::Close(args) => {
            if !args.yes {
                anyhow::bail!("Closing removes the mod from the game. Pass --yes to confirm.");
            }
            manage::delete_mod(&token_api()?, args.mod_id)?;
            println!("Mod {} is closed", args.mod_id);
            Ok(())
        }
        Command::EditFile(args) => {
            manage::edit_file(
                &token_api()?,
                args.mod_id,
                args.file_id,
                args.file_version.as_deref(),
                args.changelog.as_deref(),
            )?;
            println!("File {} saved", args.file_id);
            Ok(())
        }
        Command::DeleteFile(args) => {
            manage::delete_file(&token_api()?, args.mod_id, args.file_id)?;
            println!("File {} deleted", args.file_id);
            Ok(())
        }
        Command::AddImages(args) => {
            manage::add_images(&token_api()?, args.mod_id, &args.images)?;
            println!("Added {} images", args.images.len());
            Ok(())
        }
        Command::AddLinks(args) => {
            manage::add_links(&token_api()?, args.mod_id, &args.youtube, &args.sketchfab)?;
            println!("Links added");
            Ok(())
        }
        Command::RemoveMedia(args) => {
            let media = manage::MediaList {
                images: args.image,
                youtube: args.youtube,
                sketchfab: args.sketchfab,
            };
            manage::delete_media(&token_api()?, args.mod_id, &media)?;
            println!("Media removed");
            Ok(())
        }
        Command::ReorderImages(args) => {
            let media = manage::MediaList {
                images: args.images,
                ..manage::MediaList::default()
            };
            manage::reorder_media(&token_api()?, args.mod_id, &media)?;
            println!("Order saved");
            Ok(())
        }
        Command::Dependencies(args) => {
            let api = token_api()?;
            if !args.add.is_empty() {
                manage::add_dependencies(&api, args.mod_id, &args.add)?;
            }
            if !args.remove.is_empty() {
                manage::remove_dependencies(&api, args.mod_id, &args.remove)?;
            }
            for item in manage::dependencies(&api, args.mod_id)? {
                println!("{}  {}", item.id, item.name);
            }
            Ok(())
        }
        Command::Metadata(args) => {
            let api = token_api()?;
            if let Some(pair) = &args.add {
                let (key, value) = split_pair(pair)?;
                manage::add_metadata(&api, args.mod_id, key, value)?;
            }
            if let Some(pair) = &args.remove {
                let (key, value) = split_pair(pair)?;
                manage::remove_metadata(&api, args.mod_id, key, value)?;
            }
            for pair in manage::metadata(&api, args.mod_id)? {
                println!("{}: {}", pair.key, pair.value);
            }
            Ok(())
        }
        Command::Team(args) => {
            for member in community::team(&token_api()?, args.mod_id)? {
                println!(
                    "{}  level {}  {}",
                    member.username, member.level, member.position
                );
            }
            Ok(())
        }
        Command::Comments(args) => {
            let page = community::comments(&token_api()?, args.mod_id, args.offset)?;
            for comment in &page.comments {
                println!("{}  {}: {}", comment.id, comment.username, comment.content);
            }
            if let Some(next) = page.next {
                println!("{} in total. Add --offset {next} for more.", page.total);
            }
            Ok(())
        }
        Command::Comment(args) => {
            community::add_comment(&token_api()?, args.mod_id, &args.content, args.reply_to)?;
            println!("Comment posted");
            Ok(())
        }
        Command::DeleteComment(args) => {
            community::delete_comment(&token_api()?, args.mod_id, args.comment_id)?;
            println!("Comment deleted");
            Ok(())
        }
        Command::Activity(args) => {
            for event in community::events(&token_api()?, args.mod_id)? {
                println!("{}  {}", event.date_added, event.event_type);
            }
            Ok(())
        }
        Command::Publish(args) => run_publish(args),
        Command::Status(args) => {
            let status = mod_status(&token_api()?, args.mod_id)?;
            println!("{}: {}", status.name, status.summary);
            for platform in status.platforms {
                let version = platform.version.unwrap_or_else(|| "unknown".to_string());
                println!(
                    "{:>8}  version {version}, file {}",
                    platform.platform, platform.file_id
                );
            }
            Ok(())
        }
        Command::Files(args) => {
            for file in all_files(&token_api()?, args.mod_id)? {
                println!(
                    "{}  v{}  {:?}  {} bytes  scanned={}",
                    file.id, file.version, file.platforms, file.filesize, file.scanned
                );
            }
            Ok(())
        }
        Command::MakeLive(args) => {
            make_live(&token_api()?, args.mod_id, args.file_id)?;
            println!("File {} is live", args.file_id);
            Ok(())
        }
        Command::Edit(args) => {
            let api = token_api()?;
            edit_details(&api, args.mod_id, &args.name, &args.summary)?;
            if let Some(logo) = args.logo {
                set_logo(&api, args.mod_id, &logo)?;
            }
            println!("Details saved");
            Ok(())
        }
        Command::Browse(args) => {
            let page = browse(&token_api()?, &args.search, &args.sort, 0, 24)?;
            println!("{} source plugins", page.total);
            for item in page.mods {
                println!(
                    "{}  {}  by {}, {} bytes",
                    item.id, item.name, item.author, item.filesize
                );
            }
            Ok(())
        }
        Command::Download(args) => download(&token_api()?, &args.project, args.mod_id),
        Command::Stage(args) => stage_local(
            &CookOptions {
                project: args.project,
                engine: args.engine,
                mod_id: args.mod_id,
            },
            &args.folder,
        ),
        Command::Analyze(args) => {
            let report = analyze_pak(&args.engine, &args.pak)?;
            println!(
                "{} files, {} MB",
                report.files,
                report.total_bytes / 1_000_000
            );
            for item in report.folders.iter().take(10) {
                println!("{:>8} MB  {}", item.bytes / 1_000_000, item.name);
            }
            println!("Largest files:");
            for item in report.largest.iter().take(10) {
                println!("{:>8} MB  {}", item.bytes / 1_000_000, item.name);
            }
            Ok(())
        }
        Command::Create(args) => {
            let new = NewMod {
                name: args.name,
                summary: args.summary,
                kind: ModKind::parse(&args.kind)?,
                logo: args.logo,
                hidden: args.hidden,
            };
            let workspace = Workspace {
                project: args.project,
                engine: args.engine,
            };
            let id = create_mod(&token_api()?, &workspace, &new)?;
            println!("Created mod {id}");
            Ok(())
        }
        Command::SetupPlugin(args) => {
            let workspace = Workspace {
                project: args.project,
                engine: args.engine,
            };
            setup_plugin(&workspace, args.mod_id, ModKind::parse(&args.kind)?)
        }
        Command::SetVersion(args) => {
            let asset = args.asset.unwrap_or_else(|| definition_asset(args.mod_id));
            let saved = write_version(&args.engine, &args.project, &asset, args.version)?;
            println!("{asset} ({}) is now version {}", saved.class, saved.version);
            Ok(())
        }
        Command::CheckToken => {
            check_token(&token_api()?)?;
            println!("mod.io accepted the token");
            Ok(())
        }
        Command::EditorStatus(args) => {
            let state = editor_state(&args.project);
            println!(
                "open: {}, connected: {}, unsaved: {}",
                state.open, state.connected, state.unsaved
            );
            Ok(())
        }
        Command::RestartEditor(args) => {
            let restarted = restart_editor(&args.project, &args.engine)?;
            println!(
                "{}",
                if restarted {
                    "restarted"
                } else {
                    "stayed open"
                }
            );
            Ok(())
        }
        Command::Cook(args) => {
            let (options, targets) = cook_options(args)?;
            cook(&options, targets)
        }
        Command::Upload(args) => upload(args),
    }
}
