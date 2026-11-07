import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { check, type Update } from "@tauri-apps/plugin-updater";
import morphdom from "morphdom";
import logo from "./logo.png";
import "./styles.css";

type LocalMod = { id: number; name: string; kind: string; source: boolean };
type LiveFile = { platform: string; file_id: number };
type ModStats = {
  downloads_total: number; downloads_today: number; subscribers: number;
  ratings_positive: number; ratings_negative: number; rating_text: string; rank: number; rank_total: number;
};
type ModInfo = {
  id: number; name: string; name_id: string; summary: string; description: string; homepage_url: string;
  logo: string | null; profile_url: string | null; visible: boolean; kind: string; author: string;
  date_added: number; date_updated: number; maturity_option: number; community_options: number; credit_options: number;
  stats: ModStats; platforms: LiveFile[]; images: { filename: string; thumb: string }[]; youtube: string[]; sketchfab: string[];
};
type Library = { mods: ModInfo[]; access_known: boolean; owned: number[]; user: string | null; note: string | null };
type Settings = { project: string; engine: string; stage: string; username: string };
type ModFile = {
  id: number; version: string; filesize: number; date_added: number; platforms: string[]; scanned: boolean;
  filename: string; changelog: string; md5: string; uncompressed: number; flagged: boolean;
  pending: string[]; denied: string[]; download: string | null;
};
type TeamMember = { user_id: number; username: string; avatar: string | null; profile_url: string | null; level: number; position: string; pending: boolean };
type Comment = { id: number; reply_id: number; username: string; avatar: string | null; date_added: number; karma: number; content: string };
type ModEvent = { id: number; user_id: number; date_added: number; event_type: string };
type MetadataPair = { key: string; value: string };
type PakItem = { name: string; bytes: number };
type PakReport = { files: number; total_bytes: number; folders: PakItem[]; largest: PakItem[] };
type FilePage = { files: ModFile[]; total: number; next: number | null };
type CommentPage = { comments: Comment[]; total: number; next: number | null };
type FolderCheck = { engine: string; problems: string[] };
type Usage = { requests: number; writes: number; request_limit: number; write_limit: number; wait_seconds: number };
type SourceMod = { id: number; name: string; summary: string; author: string; logo: string | null; filesize: number };
type Tab = "overview" | "publish" | "cook" | "source" | "files" | "page" | "media" | "comments" | "team" | "advanced" | "size";
type AssetStatus = "required" | "used" | "in_string" | "other_mods" | "unused" | "unknown";
type ShareStatus = "needed" | "changed" | "added";
type Eula = { text: string; accepted: boolean };
type Asset = { path: string; size: number; status: AssetStatus; used_by: string[]; uses: number[]; outside: string[] };
type Consumer = { mod_id: number; assets: number };
type AssetReport = {
  assets: Asset[]; analysed: boolean; rooted: boolean; excluded: string[]; confirmed: boolean;
  pinned: string[]; consumers: Consumer[];
};
type MissingReference = { kept: string; missing: string };
type EditorState = { open: boolean; connected: boolean; unsaved: number };
type TreeFolder = { path: string; name: string; folders: TreeFolder[]; assets: Asset[]; all: string[]; size: number };
type TreeRow = { key: string; depth: number; folder: TreeFolder | null; asset: Asset | null };
type AssetView = "all" | "left-out" | "unused" | "in_string" | "not-built" | "pinned" | "shared" | "needed" | "changed";
type ShareReport = {
  items: { path: string; size: number; change: "" | "changed" | "added" }[];
  descriptor: string; files: string[]; gone: string[]; removed: string[];
};
type ShareCheck = {
  missing: { from: string; needs: string }[]; add: string[];
  plugins: { plugin: string; from: string; assets: number }[];
};
/** What the Source tab adds to the tree it shares with Cook. */
type ShareState = {
  descriptor: string; changeNote: string; check: ShareCheck | null; needed: Set<string>;
  checking: boolean; ticket: number; changes: Map<string, "changed" | "added">; gone: string[]; removed: string[];
};
type CookView = {
  report: AssetReport | null;
  root: TreeFolder | null;
  error: string;
  loading: boolean;
  analysing: boolean;
  excluded: Set<string>;
  pinned: Set<string>;
  expanded: Set<string>;
  selected: Set<string>;
  anchor: string;
  cursor: string;
  query: string;
  view: AssetView;
  undo: string[][];
  redo: string[][];
  built: Set<string> | null;
  note: Note | null;
  share: ShareState | null;
};
type Group = "pc" | "quest" | "both";
type Note = { ok: boolean; text: string };
type UpdateStatus = "idle" | "checking" | "current" | "available" | "installing" | "failed";
type Slot<T> = { value?: T; error?: string; busy?: boolean };
type Entry = { id: number; name: string; kind: string; local: LocalMod | null; info: ModInfo | null };
type PageDraft = {
  name: string; name_id: string; summary: string; description: string; homepage_url: string; logo: string;
  visible: boolean; maturity_option: number; community_options: number; credit_options: number;
};
type Modal =
  | { kind: "edit-file"; file: ModFile; version: string; changelog: string }
  | { kind: "edit-comment"; comment: Comment; content: string }
  | { kind: "close-mod"; typed: string };
type LogLine = { text: string; level: "step" | "info" | "warning" | "error" };
type Failure = {
  kind: string; target: string | null; tool: string; summary: string;
  errors: string[]; exit_code: number | null; log_path: string | null;
};
type Progress =
  | { kind: "log"; line: string }
  | { kind: "output"; lines: { tool: string; text: string; level: LogLine["level"] }[] }
  | { kind: "stage"; stage: string; target: string | null }
  | { kind: "parts"; target: string; done: number; total: number }
  | { kind: "finished"; ok: boolean; message: string; failure: Failure | null }
  | { kind: "waiting"; seconds: number };
type Run = {
  modId: number;
  modName: string;
  action: "publish" | "stage" | "share";
  inactive: boolean;
  together: boolean;
  targets: string[];
  builds: Record<string, { done: number; total: number }>;
  stage: string;
  target: string;
  log: LogLine[];
  errors: number;
  warnings: number;
  verbose: boolean;
  result: { ok: boolean; message: string; failure: Failure | null } | null;
  startedAt: number;
  endedAt: number | null;
  stopping: boolean;
  showLog: boolean;
  copyLabel: string;
};

/** Named as on mod.io. The icon shows what each build is for: Linux and Mac are the game's servers. */
const PLATFORMS: Record<string, { name: string; icon: keyof typeof ICONS; role: string }> = {
  windows: { name: "Windows", icon: "monitor", role: "PC client" },
  linux: { name: "Linux", icon: "server", role: "PC server" },
  oculus: { name: "Oculus", icon: "headset", role: "Quest client" },
  mac: { name: "Mac", icon: "server", role: "Quest server" },
  source: { name: "Source", icon: "code", role: "Source plugin" },
};
const LIVE_PLATFORMS = ["windows", "linux", "oculus", "mac"];
const STAGES = [
  { id: "check", label: "Check" },
  { id: "version", label: "Version" },
  { id: "cook", label: "Cook" },
  { id: "package", label: "Package" },
  { id: "upload", label: "Upload" },
  { id: "go-live", label: "Go live" },
];
const TARGET_PLATFORMS: Record<string, string> = {
  windows: "windows", linux: "linux", android: "oculus", shack: "mac",
};
const GROUP_TARGETS: Record<Group, string[]> = {
  pc: ["windows", "linux"],
  quest: ["android", "shack"],
  both: ["windows", "linux", "android", "shack"],
};
const GROUP_HINTS: Record<Group, string> = {
  pc: "Publishes Windows and Linux.",
  quest: "Publishes Oculus and Mac.",
  both: "Publishes Windows, Linux, Oculus and Mac.",
};
const TAB_NAMES: Record<Tab, string> = {
  overview: "Overview", publish: "Publish", cook: "Cook", source: "Source", files: "Files", page: "Page", media: "Media",
  comments: "Comments", team: "Team", advanced: "Advanced", size: "Size",
};
const KIND_NAMES: Record<string, string> = { GameMode: "Game mode" };
const LEVEL_NAMES: Record<number, string> = { 1: "Member", 4: "Manager", 8: "Leader" };
const EVENT_NAMES: Record<string, string> = {
  MODFILE_CHANGED: "Live file changed",
  MOD_AVAILABLE: "Mod became available",
  MOD_UNAVAILABLE: "Mod became unavailable",
  MOD_EDITED: "Page edited",
  MOD_DELETED: "Mod deleted",
  MOD_TEAM_CHANGED: "Team changed",
  MOD_COMMENT_ADDED: "Comment added",
  MOD_COMMENT_DELETED: "Comment deleted",
};
const MATURITY_FLAGS = [[1, "Alcohol"], [2, "Drugs"], [4, "Violence"], [8, "Explicit"]] as const;
const COMMUNITY_FLAGS = [
  [1, "Allow comments", "Players can comment on the mod page."],
  [1024, "Allow dependencies", "Other mods can list this mod as a dependency."],
  [131072, "Allow in collections", "Players can add this mod to collections."],
] as const;
const CREDIT_FLAGS = [
  [1, "Show credits"],
  [2, "All assets are original or used with permission"],
  [4, "Others can redistribute it with credit"],
  [8, "Others can port it with credit"],
] as const;
const PATCH_CHOICES = [[0, "Not allowed"], [64, "With permission"], [32, "With credit"], [16, "Without credit"]] as const;
const REPACK_CHOICES = [[0, "Not allowed"], [512, "With permission"], [256, "With credit"], [128, "Without credit"]] as const;
const PATCH_MASK = 16 | 32 | 64;
const REPACK_MASK = 128 | 256 | 512;
const NO_BUILD = "no build";
const STATUS_NAMES: Record<AssetStatus | ShareStatus, string> = {
  required: "Required", used: "Used", in_string: "In a string", other_mods: "Other mods", unused: "Unused", unknown: "",
  needed: "Needed", changed: "Changed", added: "New",
};
const STATUS_HINTS: Record<AssetStatus, string> = {
  required: "The mod can't work without this.",
  used: "Unreal's dependency tree reaches this from the mod's definition.",
  in_string: "Unreal's tree doesn't reach this, but another asset names it. It may be loaded while the game runs, so Filter only used keeps it.",
  other_mods: "Another mod in this modkit uses this, or something it needs uses this.",
  unused: "Nothing found uses this. It could still be loaded by a name built while the game runs.",
  unknown: "Use Filter only used to find out what uses this.",
};
const HISTORY_LIMIT = 100;
const LOG_LIMIT = 50000;
const LOG_SHOWN = 1000;
const ERRORS_SHOWN = 8;
const VERBOSE_SETTING = "verbose-log";
const FAILURE_NAMES: Record<string, string> = {
  compile: "Compile failed", cook: "Cook failed", stage: "Staging failed", package: "Packaging failed",
  audit: "Content check failed", definition: "Definition update failed",
};
const STAGE_FAILURES: Record<string, string> = {
  check: "Check failed", version: "Version update failed", cook: "Cook failed",
  package: "Packaging failed", upload: "Upload failed", "go-live": "Going live failed",
};
const FILES_SHOWN = 50;
/** Refresh is a click away, so it needs a rest or a few clicks could use up mod.io's limit. */
const REFRESH_SECONDS = 10;
/** Shorter waits come and go too fast to be worth a notice. */
const WAIT_SHOWN_FROM = 3;
const USAGE_SECONDS = 2;
const SEARCH_DELAY = 500;
const SAVED_LIBRARY = "library-v1";
const ACCESS_HINT_SETTING = "access-hint-hidden";
const TOKEN_PAGE = "https://mod.io/me/access#tokens";
const RELEASES_PAGE = "https://github.com/PavlovVR/pavlov-mod-studio/releases/latest";
const MANUAL_UPDATES_SETTING = "manual-updates";
const NOTE_SECONDS = 6;

const ICONS = {
  plus: '<path d="M5 12h14M12 5v14"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  settings: '<path d="M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4"/>',
  library: '<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/>',
  external: '<path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  close: '<path d="M18 6 6 18M6 6l12 12"/>',
  alert: '<circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/>',
  refresh: '<path d="M21 12a9 9 0 0 1-15 6.7L3 16M3 12a9 9 0 0 1 15-6.7L21 8"/><path d="M21 3v5h-5M3 21v-5h5"/>',
  pin: '<path d="M12 17v5M9 3h6l-1 7 3 3v2H7v-2l3-3Z"/>',
  edit: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/>',
  trash: '<path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2M19 6l-1 14a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1L5 6M10 11v6M14 11v6"/>',
  download: '<path d="M12 3v12M7 10l5 5 5-5M4 21h16"/>',
  monitor: '<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>',
  server: '<rect x="2" y="3" width="20" height="8" rx="2"/><rect x="2" y="13" width="20" height="8" rx="2"/><path d="M6 7h.01M6 17h.01"/>',
  headset: '<path d="M2 11a3 3 0 0 1 3-3h14a3 3 0 0 1 3 3v4a3 3 0 0 1-3 3h-3l-2.2-2.6a2.4 2.4 0 0 0-3.6 0L8 18H5a3 3 0 0 1-3-3Z"/>',
  code: '<path d="m16 18 6-6-6-6M8 6l-6 6 6 6"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>',
  file: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Z"/><path d="M14 3v5h5"/>',
  filter: '<path d="M3 5h18l-7 8v6l-4 2v-8Z"/>',
  undo: '<path d="M9 14 4 9l5-5"/><path d="M4 9h11a5 5 0 0 1 0 10h-3"/>',
  chevron: '<path d="m6 9 6 6 6-6"/>',
  expand: '<path d="m7 15 5 5 5-5M7 9l5-5 5 5"/>',
  collapse: '<path d="m7 20 5-5 5 5M7 4l5 5 5-5"/>',
  left: '<path d="m15 18-6-6 6-6"/>',
  right: '<path d="m9 18 6-6-6-6"/>',
};

function icon(name: keyof typeof ICONS, size = 16): string {
  return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" class="shrink-0">${ICONS[name]}</svg>`;
}

type ModData = {
  files: Slot<ModFile[]>; team: Slot<TeamMember[]>; comments: Slot<Comment[]>;
  deps: Slot<ModInfo[]>; metadata: Slot<MetadataPair[]>; events: Slot<ModEvent[]>;
  reports: Record<string, Slot<PakReport>>;
  /** Files and comments arrive a page at a time, because a mod can have thousands. */
  filesTotal: number; filesNext: number | null; commentsNext: number | null;
};
const COMMANDS = {
  team: "mod_team", deps: "mod_dependencies", metadata: "mod_metadata", events: "mod_events",
} as const;
type SlotKey = "files" | "comments" | keyof typeof COMMANDS;
const TAB_DATA: Partial<Record<Tab, SlotKey[]>> = {
  overview: ["files", "events"], publish: ["files"], source: ["files"], files: ["files"],
  comments: ["comments"], team: ["team"], advanced: ["deps", "metadata"],
};

/** What mod.io said about each mod. Kept until a refresh or a change, so switching mods is free. */
const cache = new Map<number, ModData>();
const drafts = new Map<number, PageDraft>();
const cookViews = new Map<number, CookView>();
const shareViews = new Map<number, CookView>();
/** A change note belongs to the mod it was written for. */
const changelogs = new Map<number, string>();

function readVerbose(): boolean {
  try {
    return localStorage.getItem(VERBOSE_SETTING) === "1";
  } catch {
    return false;
  }
}

function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

function saveFlag(key: string): void {
  try {
    localStorage.setItem(key, "1");
  } catch {
    // It comes back next launch.
  }
}

function saveAutoUpdate(on: boolean): void {
  try {
    localStorage.setItem(MANUAL_UPDATES_SETTING, on ? "0" : "1");
  } catch {
    // It comes back next launch.
  }
}

function saveVerbose(verbose: boolean): void {
  state.verbose = verbose;
  try {
    localStorage.setItem(VERBOSE_SETTING, verbose ? "1" : "0");
  } catch {
    // It stays on for this session.
  }
}

const state = {
  settings: { project: "", engine: "", stage: "", username: "" } as Settings,
  settingsDraft: { project: "", engine: "", stage: "", username: "" } as Settings,
  tokenDraft: "",
  hasToken: false,
  tokenBusy: false,
  autoUpdate: !readFlag(MANUAL_UPDATES_SETTING),
  update: { status: "idle" as UpdateStatus, version: "", percent: null as number | null, error: "" },
  settingsNote: null as Note | null,
  eula: null as Eula | null,
  eulaNote: null as Note | null,
  accountNote: null as Note | null,
  mods: [] as LocalMod[],
  infos: {} as Record<number, ModInfo>,
  access: { known: false, owned: new Set<number>(), user: null as string | null, note: null as string | null },
  loading: false,
  libraryError: "",
  showOthers: false,
  filter: "",
  selectedId: null as number | null,
  view: "mod" as "mod" | "settings" | "create" | "source",
  tab: "publish" as Tab,
  refreshing: false,
  cooling: { all: false, mod: false },
  waitUntil: 0,
  usage: null as Usage | null,
  acting: false,
  checking: false,
  accessHintHidden: readFlag(ACCESS_HINT_SETTING),
  version: "",
  note: null as Note | null,
  modal: null as Modal | null,
  modalError: "",
  reportTarget: "windows",
  fileFilter: "all",
  filesShown: FILES_SHOWN,
  pickedFiles: new Set<number>(),
  commentDraft: "",
  replyTo: null as number | null,
  replyDraft: "",
  linkDraft: "",
  dependencyDraft: "",
  metadataDraft: { key: "", value: "" },
  source: { search: "", sort: "-date_live", offset: 0, total: 0, mods: [] as SourceMod[], busy: false, downloading: 0, note: null as Note | null },
  together: false,
  pending: null as number | null,
  shareSource: false,
  failedSetup: null as { id: number; kind: string } | null,
  draft: { name: "", summary: "", kind: "Map", logo: "" },
  creating: false,
  createError: "",
  group: "both" as Group,
  changelog: "",
  inactive: false,
  compress: true,
  run: null as Run | null,
  verbose: readVerbose(),
};

const app = document.querySelector<HTMLElement>("#app")!;
app.className = "grid h-screen grid-cols-[248px_minmax(0,1fr)]";
app.spellcheck = false;
app.innerHTML = `
  <aside id="rail" class="flex min-h-0 flex-col border-r border-line bg-panel"></aside>
  <div class="flex min-h-0 min-w-0 flex-col">
    <main id="main" class="min-h-0 flex-1 overflow-y-auto"></main>
    <section id="run" class="max-h-[62vh] shrink-0 overflow-y-auto" aria-label="Current run"></section>
  </div>
  <div id="announce" class="sr-only" role="status" aria-live="polite"></div>
  <dialog id="modal" aria-labelledby="modal-title" class="m-auto w-[min(32rem,calc(100vw-2rem))] rounded-lg border border-line bg-canvas p-0 text-ink backdrop:bg-black/50"></dialog>`;
const rail = app.querySelector<HTMLElement>("#rail")!;
const main = app.querySelector<HTMLElement>("#main")!;
const runPanel = app.querySelector<HTMLElement>("#run")!;
const announce = app.querySelector<HTMLElement>("#announce")!;
const modal = app.querySelector<HTMLDialogElement>("#modal")!;

function escape(value: string): string {
  const entities: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
  return value.replace(/[&<>"']/g, (char) => entities[char]);
}

function platformName(platform: string): string {
  return PLATFORMS[platform]?.name ?? platform;
}

function targetName(target: string): string {
  return platformName(TARGET_PLATFORMS[target] ?? target);
}

/** A build's name with its icon. The tooltip and screen reader text say what it's for. */
function platformHtml(platform: string): string {
  const known = PLATFORMS[platform];
  if (!known) return escape(platform);
  return `<span class="inline-flex min-w-0 items-center gap-1.5" title="${known.role}"><span class="text-ink-subtle">${icon(known.icon, 15)}</span><span class="truncate">${known.name}</span><span class="sr-only">, ${known.role}</span></span>`;
}

function kindName(kind: string): string {
  return KIND_NAMES[kind] ?? kind;
}

function isRunning(): boolean {
  return Boolean(state.run && !state.run.result);
}

function megabytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${Math.max(Math.round(bytes / 1e3), 1)} KB`;
}

function count(value: number): string {
  return value.toLocaleString();
}

const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

function when(seconds: number): string {
  return seconds ? dateFormat.format(new Date(seconds * 1000)) : "Unknown";
}

/** mod.io comments are written in Markdown, and bold is the part that reads badly as raw text. */
function richText(text: string): string {
  return escape(text).replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>");
}

function duration(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  const pad = (value: number) => String(value).padStart(2, "0");
  const clock = `${pad(Math.floor(seconds / 60) % 60)}:${pad(seconds % 60)}`;
  return seconds >= 3600 ? `${Math.floor(seconds / 3600)}:${clock}` : clock;
}

function noteHtml(note: Note | null): string {
  if (!note) return "";
  return `<div class="notice ${note.ok ? "notice-ok" : "notice-failed"}">${icon(note.ok ? "check" : "alert")}<span class="select-text">${escape(note.text)}</span></div>`;
}

function failedHtml(text: string): string {
  return noteHtml({ ok: false, text });
}

function loadingHtml(): string {
  if (!state.hasToken) return `<p class="py-2 text-ink-muted">Connect your mod.io account in Settings to see this.</p>`;
  return `<p class="flex items-center gap-2.5 py-2 text-ink-subtle"><span class="spinner"></span>Loading</p>`;
}

function locked(): string {
  return state.acting ? "disabled" : "";
}

/** Results float over the page, so they're seen however far down it's scrolled. */
function toastHtml(): string {
  const note = state.note;
  if (!note) return "";
  return `
    <div id="toast" class="notice toast ${note.ok ? "notice-ok" : "notice-failed"}" role="${note.ok ? "status" : "alert"}">
      ${icon(note.ok ? "check" : "alert")}
      <span class="flex-1 select-text">${escape(note.text)}</span>
      <button class="-my-1 -mr-1 cursor-pointer rounded p-1.5 hover:bg-raised" data-action="dismiss-note" aria-label="Dismiss" title="Dismiss">${icon("close", 14)}</button>
    </div>`;
}

function noListingHtml(): string {
  return state.hasToken
    ? `<p class="text-ink-muted">mod.io has no public listing for this mod.</p>`
    : `<p class="text-ink-muted">Connect your mod.io account in Settings to see this.</p>`;
}

/* The mod list */

function dataFor(id: number): ModData {
  let data = cache.get(id);
  if (!data) {
    data = { files: {}, team: {}, comments: {}, deps: {}, metadata: {}, events: {}, reports: {}, filesTotal: 0, filesNext: null, commentsNext: null };
    cache.set(id, data);
  }
  return data;
}

function canManage(id: number): boolean {
  return !state.access.known || state.access.owned.has(id);
}

function allEntries(): Entry[] {
  const entries: Entry[] = state.mods.map((local) => {
    const info = state.infos[local.id] ?? null;
    return { id: local.id, name: info?.name || local.name, kind: info?.kind ?? local.kind, local, info };
  });
  const seen = new Set(entries.map((entry) => entry.id));
  for (const id of state.access.owned) {
    const info = state.infos[id];
    if (info && !seen.has(id)) entries.push({ id, name: info.name, kind: info.kind, local: null, info });
  }
  return entries.sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true }));
}

function selectedEntry(): Entry | null {
  return allEntries().find((entry) => entry.id === state.selectedId) ?? null;
}

function railHtml(): string {
  const row = (entry: Entry) => {
    const current = state.view === "mod" && state.selectedId === entry.id;
    const busy = isRunning() && state.run?.modId === entry.id;
    const hidden = entry.info && !entry.info.visible;
    return `
      <button class="nav-row" data-mod="${entry.id}" title="${escape(entry.name)}" ${current ? 'aria-current="page"' : ""}>
        <span class="min-w-0 flex-1">
          <span class="block truncate font-medium">${escape(entry.name)}</span>
          <span class="block text-xs text-ink-subtle">${escape(kindName(entry.kind))}${hidden ? ", hidden" : ""}</span>
        </span>
        ${busy ? `<span class="spinner" aria-label="Running"></span>` : ""}
      </button>`;
  };
  const all = allEntries();
  const query = all.length > 6 ? state.filter.trim().toLowerCase() : "";
  const match = (entry: Entry) => entry.name.toLowerCase().includes(query) || String(entry.id).includes(query);
  const known = state.access.known;
  const manageable = all.filter((entry) => canManage(entry.id));
  const others = all.filter((entry) => !canManage(entry.id));
  const inKit = manageable.filter((entry) => entry.local && !(entry.local.source && !known));
  const sources = known ? [] : manageable.filter((entry) => entry.local?.source);
  const remote = manageable.filter((entry) => !entry.local);
  const heading = (text: string) => `<div class="px-2.5 pt-4 pb-1 text-xs font-medium text-ink-subtle">${text}</div>`;
  const group = (title: string, items: Entry[]) => {
    const shown = items.filter(match);
    return shown.length ? `${title ? heading(title) : ""}${shown.map(row).join("")}` : "";
  };
  const showing = [...inKit, ...sources, ...remote, ...(state.showOthers ? others : [])];
  const empty = !state.settings.project
    ? `<p class="hint px-2.5 py-2">Choose a modkit folder to see your mods.</p>`
    : !all.length ? `<p class="hint px-2.5 py-2">${state.loading ? "Loading" : "No mods found in the modkit folder."}</p>`
    : !showing.filter(match).length ? `<p class="hint px-2.5 py-2">No mods match that filter.</p>` : "";
  const toggle = others.length
    ? `<button class="btn btn-ghost btn-sm mt-2 w-full" data-action="toggle-others" aria-expanded="${state.showOthers}">
        ${state.showOthers ? "Hide" : "Show"} ${others.length} ${others.length === 1 ? "mod" : "mods"} you can't manage
      </button>`
    : "";
  const wait = waiting()
    ? `<div class="notice mx-2 mb-2" role="status">
        <span class="spinner size-3.5 mt-0.5"></span>
        <span>Waiting on mod.io's request limit. Carrying on in <span id="wait-seconds" class="tabular-nums">${secondsLeft()}</span>s.</span>
      </div>`
    : "";
  const status = waiting() ? ""
    : state.libraryError
    ? `<div class="notice notice-failed mx-2 mb-2 flex-col">
        <span>${Object.keys(state.infos).length ? "Showing saved data. " : ""}${escape(state.libraryError)}</span>
        <button class="btn btn-secondary btn-sm" data-action="refresh-all" ${state.cooling.all ? "disabled" : ""}>Try again</button>
      </div>`
    : state.hasToken && !state.loading && !known && !state.accessHintHidden
      ? `<div class="notice mx-2 mb-2 flex-col">
          <span>All mods are shown. Add your mod.io username in Settings to hide mods you can't manage.</span>
          <span class="flex gap-2">
            <button class="btn btn-secondary btn-sm" data-action="settings">Open Settings</button>
            <button class="btn btn-ghost btn-sm" data-action="hide-access-hint">Dismiss</button>
          </span>
        </div>`
      : "";
  return `
    <div class="flex h-14 shrink-0 items-center justify-between gap-1 pr-1.5 pl-3">
      <span class="flex min-w-0 items-center gap-2">
        <img src="${logo}" alt="" class="size-5 shrink-0 rounded" />
        <span class="truncate text-sm font-semibold tracking-tight">Pavlov Mod Studio</span>
      </span>
      <span class="flex">
        <button class="btn btn-ghost btn-sm btn-icon" data-action="refresh-all" aria-label="Refresh all mods" title="Refresh all mods" ${state.loading || !state.hasToken || state.cooling.all || waiting() ? "disabled" : ""}>
          ${state.loading ? `<span class="spinner"></span>` : icon("refresh")}
        </button>
        <button class="btn btn-ghost btn-sm btn-icon" data-action="new-mod" aria-label="New mod" title="New mod">${icon("plus")}</button>
      </span>
    </div>
    ${all.length > 6 ? `
      <div class="px-2 pb-1">
        <input id="mod-filter" class="input h-8" autocomplete="off" value="${escape(state.filter)}" placeholder="Filter mods" aria-label="Filter mods" />
      </div>` : ""}
    <nav id="mod-list" class="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-2 pb-2" aria-label="Mods">
      ${group("", inKit)}
      ${group("Source plugins", sources)}
      ${group("Not in this modkit", remote)}
      ${state.showOthers ? group("Mods you can't manage", others) : ""}
      ${empty}
      ${toggle}
    </nav>
    ${wait}${status}
    <div class="space-y-0.5 border-t border-line p-2">
      <button class="nav-row" data-action="source" ${state.view === "source" ? 'aria-current="page"' : ""}>${icon("library")}Browse source plugins</button>
      <button class="nav-row" data-action="settings" ${state.view === "settings" ? 'aria-current="page"' : ""}>${icon("settings")}Settings${state.update.status === "available" ? `<span class="badge ml-auto">Update</span>` : state.update.status === "installing" ? `<span class="badge badge-plain ml-auto">Updating</span>` : ""}</button>
    </div>`;
}

function pageHeader(title: string, subtitle: string): string {
  return `<header class="mb-6"><h1 class="text-xl font-semibold tracking-tight">${title}</h1>${subtitle ? `<p class="mt-1 text-ink-muted">${subtitle}</p>` : ""}</header>`;
}

/* Overview */

function liveHtml(entry: Entry): string {
  if (!entry.info) return `<div class="card p-4">${state.loading ? loadingHtml() : noListingHtml()}</div>`;
  const files = dataFor(entry.id).files;
  const rows = LIVE_PLATFORMS.map((platform) => {
    const live = entry.info!.platforms.find((p) => p.platform === platform);
    const version = live && files.value?.find((file) => file.id === live.file_id)?.version;
    const value = !live
      ? `<span class="text-ink-subtle">Not published</span>`
      : version ? `<span class="font-medium tabular-nums">Version ${escape(version)}</span>`
      : files.value || files.error ? `<span class="font-medium tabular-nums">File ${live.file_id}</span>`
      : `<span class="spinner size-3.5"></span>`;
    return `<li class="flex items-center justify-between gap-3 border-t border-line px-4 py-2.5 first:border-t-0"><span class="min-w-0 text-ink-muted">${platformHtml(platform)}</span>${value}</li>`;
  }).join("");
  return `
    <section class="card" aria-labelledby="live-title">
      <h2 id="live-title" class="border-b border-line px-4 py-2.5 text-[13px] font-semibold">Live now</h2>
      <ul>${rows}</ul>
    </section>`;
}

function eventName(type: string): string {
  const words = type.toLowerCase().replace(/_/g, " ");
  return EVENT_NAMES[type] ?? `${words[0]?.toUpperCase() ?? ""}${words.slice(1)}`;
}

function overviewHtml(entry: Entry): string {
  const info = entry.info;
  const stat = (label: string, value: string) => `
    <div class="card min-w-0 px-4 py-3">
      <div class="text-[13px] text-ink-subtle">${label}</div>
      <div class="mt-0.5 truncate font-semibold tracking-tight tabular-nums ${value.length > 14 ? "pt-1 text-sm leading-6" : "text-lg"}" title="${value}">${value}</div>
    </div>`;
  const stats = info ? `
    <div class="mb-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
      ${stat("Downloads", count(info.stats.downloads_total))}
      ${stat("Downloads today", count(info.stats.downloads_today))}
      ${stat("Subscribers", count(info.stats.subscribers))}
      ${stat("Rating", escape(info.stats.rating_text || "Unrated"))}
    </div>` : "";
  const events = dataFor(entry.id).events;
  const activity = events.error ? `<div class="p-3">${failedHtml(events.error)}</div>`
    : !events.value ? `<div class="px-4 py-2">${loadingHtml()}</div>`
    : !events.value.length ? `<p class="px-4 py-3 text-ink-muted">Nothing has happened yet.</p>`
    : `<ul>${events.value.slice(0, 12).map((event) => `
        <li class="flex justify-between gap-3 border-t border-line px-4 py-2.5 first:border-t-0">
          <span>${escape(eventName(event.event_type))}</span>
          <span class="shrink-0 text-ink-subtle">${when(event.date_added)}</span>
        </li>`).join("")}</ul>`;
  const setup = !entry.local && canManage(entry.id) && ["Map", "Mod", "GameMode"].includes(entry.kind) && state.settings.project
    ? `<div class="notice mb-6 items-center justify-between">
        <span>This mod isn't in your modkit, so it can't be published from here yet.</span>
        <button class="btn btn-secondary btn-sm" data-action="setup-local" ${locked()}>Set up in modkit</button>
      </div>` : "";
  const details = info ? `
    <dl class="mt-4 space-y-2 text-[13px]">
      <div class="flex justify-between gap-3"><dt class="text-ink-subtle">Author</dt><dd class="truncate">${escape(info.author)}</dd></div>
      <div class="flex justify-between gap-3"><dt class="text-ink-subtle">Popularity</dt><dd class="tabular-nums">${info.stats.rank ? `${count(info.stats.rank)} of ${count(info.stats.rank_total)}` : "Unranked"}</dd></div>
      <div class="flex justify-between gap-3"><dt class="text-ink-subtle">Ratings</dt><dd class="tabular-nums">${count(info.stats.ratings_positive)} up, ${count(info.stats.ratings_negative)} down</dd></div>
      <div class="flex justify-between gap-3"><dt class="text-ink-subtle">Created</dt><dd>${when(info.date_added)}</dd></div>
      <div class="flex justify-between gap-3"><dt class="text-ink-subtle">Updated</dt><dd>${when(info.date_updated)}</dd></div>
    </dl>` : "";
  return `
    ${setup}
    ${stats}
    <div class="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_288px]">
      <section class="card min-w-0" aria-labelledby="activity-title">
        <h2 id="activity-title" class="border-b border-line px-4 py-2.5 text-[13px] font-semibold">Recent activity</h2>
        ${activity}
      </section>
      <div class="min-w-0">${liveHtml(entry)}${details}</div>
    </div>`;
}

/* Files */

function matchingFiles(files: ModFile[]): ModFile[] {
  return files.filter((f) => state.fileFilter === "all" || f.platforms.includes(state.fileFilter));
}

function filesHtml(entry: Entry): string {
  const data = dataFor(entry.id);
  const slot = data.files;
  if (slot.error) return failedHtml(slot.error);
  if (!slot.value) return loadingHtml();
  if (!slot.value.length) return `<p class="text-ink-muted">No files yet. Publish this mod to add one.</p>`;
  const manage = canManage(entry.id);
  const live = new Set(entry.info?.platforms.map((p) => p.file_id));
  const builds = [...new Set(slot.value.flatMap((f) => f.platforms))];
  const shown = matchingFiles(slot.value);
  const page = shown.slice(0, state.filesShown);
  const total = state.fileFilter === "all" ? Math.max(data.filesTotal, shown.length) : shown.length;
  const pickable = page.filter((f) => !live.has(f.id));
  const allPicked = pickable.length > 0 && pickable.every((f) => state.pickedFiles.has(f.id));
  const somePicked = pickable.some((f) => state.pickedFiles.has(f.id));
  const flags = (f: ModFile) => [
    f.flagged ? `<span class="badge badge-failed">Flagged</span>` : "",
    f.denied.length ? `<span class="badge badge-failed">Denied</span>` : "",
    f.pending.length ? `<span class="badge badge-plain">Pending</span>` : "",
  ].join("");
  const rows = page.map((f) => {
    const isLive = live.has(f.id);
    const where = f.platforms.map(platformHtml).join(`<span class="text-ink-subtle">, </span>`);
    const makeLive = isLive
      ? `<span class="badge">Live</span>`
      : `<button class="btn btn-secondary btn-sm" data-publish-file="${f.id}" ${f.scanned && !state.acting ? "" : "disabled"}>${f.scanned ? "Make live" : "Scanning"}</button>`;
    return `<tr>
      ${manage ? `<td class="w-8">${isLive ? "" : `<input type="checkbox" data-pick="${f.id}" aria-label="Select version ${escape(f.version)}" ${state.pickedFiles.has(f.id) ? "checked" : ""} />`}</td>` : ""}
      <td class="min-w-0">
        <div class="flex items-center gap-2"><span class="font-medium tabular-nums">${escape(f.version)}</span>${flags(f)}</div>
        ${f.changelog ? `<div class="mt-0.5 line-clamp-1 max-w-32 text-[13px] lg:max-w-64 text-ink-subtle" title="${escape(f.changelog)}">${escape(f.changelog)}</div>` : ""}
      </td>
      <td class="text-ink-muted max-lg:[&_.truncate]:hidden">${where}</td>
      <td class="text-right whitespace-nowrap text-ink-muted tabular-nums max-lg:hidden">${megabytes(f.filesize)}</td>
      <td class="whitespace-nowrap text-ink-muted max-[1150px]:hidden">${when(f.date_added)}</td>
      <td>
        <div class="flex items-center justify-end gap-1">
          ${manage ? makeLive : isLive ? `<span class="badge">Live</span>` : ""}
          ${f.download ? `<button class="btn btn-ghost btn-sm btn-icon" data-download-file="${f.id}" aria-label="Download version ${escape(f.version)}" title="Download">${icon("download")}</button>` : ""}
          ${manage ? `
            <button class="btn btn-ghost btn-sm btn-icon" data-edit-file="${f.id}" aria-label="Edit version ${escape(f.version)}" title="Edit" ${locked()}>${icon("edit")}</button>
            <button class="btn btn-ghost btn-sm btn-icon" data-delete-file="${f.id}" aria-label="Delete version ${escape(f.version)}" title="${isLive ? "Make another file live before deleting this one" : "Delete"}" ${isLive || state.acting ? "disabled" : ""}>${icon("trash")}</button>` : ""}
        </div>
      </td></tr>`;
  }).join("");
  const picked = [...state.pickedFiles].filter((id) => slot.value!.some((f) => f.id === id)).length;
  return `
    <div class="mb-3 flex items-center justify-between gap-3">
      <select id="file-filter" class="input w-44" aria-label="Filter by build">
        <option value="all">All builds</option>
        ${builds.map((b) => `<option value="${escape(b)}" ${state.fileFilter === b ? "selected" : ""}>${escape(platformName(b))}</option>`).join("")}
      </select>
      <span class="flex items-center gap-3">
        <span class="hint tabular-nums">${count(total)} ${total === 1 ? "file" : "files"}</span>
        ${picked ? `<button class="btn btn-secondary btn-sm" data-action="delete-picked" ${locked()}>Delete ${picked} selected</button>` : ""}
      </span>
    </div>
    <div class="card overflow-x-auto overflow-y-hidden">
      <table class="data-table">
        <thead><tr>${manage ? `<th class="w-8"><input type="checkbox" id="pick-all" aria-label="Select all files shown" ${allPicked ? "checked" : ""} ${somePicked && !allPicked ? "data-mixed" : ""} ${pickable.length ? "" : "disabled"} /></th>` : ""}<th>Version</th><th>Build</th><th class="text-right max-lg:hidden">Size</th><th class="max-[1150px]:hidden">Added</th><th><span class="sr-only">Actions</span></th></tr></thead>
        <tbody>${rows || `<tr><td colspan="6" class="text-ink-muted">No files for this build.</td></tr>`}</tbody>
      </table>
    </div>
    ${shown.length > page.length || data.filesNext !== null ? `<button class="btn btn-secondary btn-sm mt-3" data-action="more-files" ${slot.busy ? "disabled" : ""}>${slot.busy ? "Loading" : "Show more"}</button>` : ""}
    ${manage ? `<p class="hint mt-3">Make an older file live to roll back that build. mod.io can't delete a live file.</p>` : ""}`;
}

/* Page */

function draftFor(entry: Entry): PageDraft | null {
  if (!entry.info) return null;
  let draft = drafts.get(entry.id);
  if (!draft) {
    const { name, name_id, summary, description, homepage_url, visible, maturity_option, community_options, credit_options } = entry.info;
    draft = { name, name_id, summary, description, homepage_url, logo: "", visible, maturity_option, community_options, credit_options };
    drafts.set(entry.id, draft);
  }
  return draft;
}

function lines(value: string): string {
  return value.replace(/\r\n/g, "\n");
}

/** An untouched draft is only a copy, so it must not outlive what it was copied from. */
function dropCleanDraft(id: number): void {
  const draft = drafts.get(id);
  const info = state.infos[id];
  if (draft && (!info || (!draft.logo && !Object.keys(pageChanges(info, draft)).length))) drafts.delete(id);
}

/** What changed, because a text box turns CRLF into LF and that alone isn't an edit. */
function pageChanges(info: ModInfo, draft: PageDraft): Partial<PageDraft> {
  const changes: Partial<PageDraft> = {};
  if (draft.name.trim() !== info.name) changes.name = draft.name;
  if (draft.name_id.trim() !== info.name_id) changes.name_id = draft.name_id;
  if (draft.summary.trim() !== lines(info.summary).trim()) changes.summary = draft.summary;
  if (lines(draft.description) !== lines(info.description)) changes.description = draft.description;
  if (draft.homepage_url.trim() !== info.homepage_url) changes.homepage_url = draft.homepage_url;
  if (draft.visible !== info.visible) changes.visible = draft.visible;
  if (draft.maturity_option !== info.maturity_option) changes.maturity_option = draft.maturity_option;
  if (draft.community_options !== info.community_options) changes.community_options = draft.community_options;
  if (draft.credit_options !== info.credit_options) changes.credit_options = draft.credit_options;
  return changes;
}

function flagHtml(group: string, bit: number, label: string, hint: string, value: number): string {
  return `
    <label class="flex cursor-pointer items-start gap-2.5">
      <input type="checkbox" class="mt-0.5" data-flag="${group}" data-bit="${bit}" ${value & bit ? "checked" : ""} />
      <span><span class="block">${label}</span>${hint ? `<span class="hint block">${hint}</span>` : ""}</span>
    </label>`;
}

function choiceHtml(id: string, label: string, choices: readonly (readonly [number, string])[], value: number): string {
  return `
    <label class="field"><span class="label">${label}</span>
      <select id="${id}" class="input">
        ${choices.map(([bit, text]) => `<option value="${bit}" ${value === bit ? "selected" : ""}>${text}</option>`).join("")}
      </select></label>`;
}

function logoField(label: string, path: string, action: string, placeholder: string): string {
  return `
    <div class="field">
      <span class="label">${label}</span>
      <div class="flex gap-2">
        <input class="input" autocomplete="off" value="${escape(path)}" readonly aria-label="${label} file" placeholder="${placeholder}" />
        <button class="btn btn-secondary" data-action="${action}">Choose image</button>
      </div>
    </div>`;
}

function pageHtml(entry: Entry): string {
  const draft = draftFor(entry);
  if (!draft) return noListingHtml();
  const dirty = Object.keys(pageChanges(entry.info!, draft)).length > 0 || Boolean(draft.logo);
  const section = (title: string, body: string) => `<section class="card space-y-3 p-5"><h2 class="font-semibold">${title}</h2>${body}</section>`;
  return `
    <div class="max-w-2xl space-y-5">
      <section class="card space-y-4 p-5">
        <h2 class="font-semibold">Listing</h2>
        <label class="field"><span class="label">Name</span>
          <input id="page-name" class="input" autocomplete="off" maxlength="80" value="${escape(draft.name)}" /></label>
        <label class="field"><span class="label">Web address name</span>
          <input id="page-name-id" class="input" autocomplete="off" maxlength="80" value="${escape(draft.name_id)}" />
          <span class="hint mt-1 block">Changing this changes the mod's link on mod.io.</span></label>
        <label class="field"><span class="label">Summary</span>
          <textarea id="page-summary" class="input" spellcheck="true" rows="3" maxlength="250">${escape(draft.summary)}</textarea></label>
        <label class="field"><span class="label">Description</span>
          <textarea id="page-description" class="input" spellcheck="true" rows="10">${escape(draft.description)}</textarea>
          <span class="hint mt-1 block">HTML is allowed.</span></label>
        <label class="field"><span class="label">Homepage</span>
          <input id="page-homepage" class="input" autocomplete="off" type="url" value="${escape(draft.homepage_url)}" placeholder="https://" /></label>
        ${logoField("New logo", draft.logo, "pick-page-logo", "Keep the current logo")}
        <div class="field"><span class="label">Type</span>
          <p class="text-ink-muted">${escape(kindName(entry.kind))}. Create a new mod to use another type, because the type is built into its files.</p></div>
      </section>
      ${section("Visibility", `
        <div class="segmented" role="group" aria-label="Visibility">
          <button data-visible="1" aria-pressed="${draft.visible}">Public</button>
          <button data-visible="0" aria-pressed="${!draft.visible}">Hidden</button>
        </div>
        <p class="hint">${draft.visible ? "Everyone can find and download this mod." : "Only your team can see this mod. Players can't find it."}</p>`)}
      ${section("Community", COMMUNITY_FLAGS.map(([bit, label, hint]) => flagHtml("community_options", bit, label, hint, draft.community_options)).join(""))}
      ${section("Mature content", MATURITY_FLAGS.map(([bit, label]) => flagHtml("maturity_option", bit, label, "", draft.maturity_option)).join(""))}
      ${section("Credits and permissions", `
        ${CREDIT_FLAGS.map(([bit, label]) => flagHtml("credit_options", bit, label, "", draft.credit_options)).join("")}
        <div class="grid gap-3 pt-1 sm:grid-cols-2">
          ${choiceHtml("credit-patch", "Patching", PATCH_CHOICES, draft.credit_options & PATCH_MASK)}
          ${choiceHtml("credit-repack", "Repackaging", REPACK_CHOICES, draft.credit_options & REPACK_MASK)}
        </div>`)}
      <div class="sticky bottom-0 -mx-1 flex items-center gap-2 bg-canvas px-1 py-3">
        <button class="btn btn-primary" data-action="save-page" ${dirty && !state.acting ? "" : "disabled"}>${state.acting ? "Saving" : "Save changes"}</button>
        <button class="btn btn-secondary" data-action="reset-page" ${dirty && !state.acting ? "" : "disabled"}>Discard</button>
      </div>
    </div>`;
}

/* Media */

function mediaHtml(entry: Entry): string {
  const info = entry.info;
  if (!info) return noListingHtml();
  const image = (item: ModInfo["images"][number], index: number) => `
    <li class="card overflow-hidden">
      <img src="${escape(item.thumb)}" alt="" class="aspect-video w-full bg-raised object-cover" />
      <div class="flex items-center justify-between gap-1 p-1.5">
        <span class="flex">
          <button class="btn btn-ghost btn-sm btn-icon" data-move-image="${index}" data-by="-1" aria-label="Move ${escape(item.filename)} earlier" title="Move earlier" ${index === 0 || state.acting ? "disabled" : ""}>${icon("left")}</button>
          <button class="btn btn-ghost btn-sm btn-icon" data-move-image="${index}" data-by="1" aria-label="Move ${escape(item.filename)} later" title="Move later" ${index === info.images.length - 1 || state.acting ? "disabled" : ""}>${icon("right")}</button>
        </span>
        <button class="btn btn-ghost btn-sm btn-icon" data-remove-image="${escape(item.filename)}" aria-label="Remove ${escape(item.filename)}" title="Remove" ${locked()}>${icon("trash")}</button>
      </div>
    </li>`;
  const links = [...info.youtube.map((url) => ["youtube", url]), ...info.sketchfab.map((url) => ["sketchfab", url])];
  return `
    <section class="mb-6">
      <div class="mb-3 flex items-center justify-between">
        <h2 class="font-semibold">Screenshots</h2>
        <button class="btn btn-secondary btn-sm" data-action="add-images" ${locked()}>${icon("plus", 14)}Add images</button>
      </div>
      ${info.images.length
        ? `<ul class="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-3">${info.images.map(image).join("")}</ul>`
        : `<p class="text-ink-muted">No screenshots yet. PNG or JPG, up to 8 MB each.</p>`}
    </section>
    <section class="max-w-2xl">
      <h2 class="mb-3 font-semibold">Videos and 3D models</h2>
      ${links.length ? `
        <ul class="card mb-3">
          ${links.map(([kind, url]) => `
            <li class="flex items-center justify-between gap-3 border-t border-line px-4 py-2 first:border-t-0">
              <span class="min-w-0 truncate" title="${escape(url)}">${escape(url)}</span>
              <button class="btn btn-ghost btn-sm btn-icon" data-remove-link="${escape(url)}" data-link-kind="${kind}" aria-label="Remove ${escape(url)}" title="Remove" ${locked()}>${icon("trash")}</button>
            </li>`).join("")}
        </ul>` : ""}
      <div class="flex gap-2">
        <input id="link-draft" class="input" autocomplete="off" value="${escape(state.linkDraft)}" placeholder="YouTube or Sketchfab link" aria-label="YouTube or Sketchfab link" />
        <button class="btn btn-secondary" data-action="add-link" ${state.linkDraft.trim() && !state.acting ? "" : "disabled"}>Add link</button>
      </div>
    </section>`;
}

/* Comments */

function commentHtml(entry: Entry, comment: Comment, replies: Comment[]): string {
  const mine = Boolean(state.access.user) && comment.username.toLowerCase() === state.access.user!.toLowerCase();
  const manage = canManage(entry.id);
  const avatar = comment.avatar
    ? `<img src="${escape(comment.avatar)}" alt="" class="size-8 shrink-0 rounded-full bg-raised object-cover" />`
    : `<span class="size-8 shrink-0 rounded-full bg-raised"></span>`;
  const replying = state.replyTo === comment.id;
  const open = !entry.info || entry.info.community_options & 1;
  return `
    <li class="flex gap-3 py-3">
      ${avatar}
      <div class="min-w-0 flex-1">
        <div class="flex flex-wrap items-baseline gap-x-2">
          <strong class="font-semibold">${escape(comment.username || "Deleted user")}</strong>
          <span class="text-xs text-ink-subtle">${when(comment.date_added)}</span>
          ${comment.karma ? `<span class="text-xs text-ink-subtle tabular-nums">${comment.karma > 0 ? "+" : ""}${comment.karma}</span>` : ""}
        </div>
        <p class="mt-1 whitespace-pre-wrap break-words select-text">${richText(comment.content)}</p>
        <div class="mt-1 flex gap-1">
          ${open && state.hasToken ? `<button class="btn btn-ghost btn-sm" data-reply="${comment.id}">Reply</button>` : ""}
          ${mine ? `<button class="btn btn-ghost btn-sm" data-edit-comment="${comment.id}" ${locked()}>Edit</button>` : ""}
          ${mine || manage ? `<button class="btn btn-ghost btn-sm" data-delete-comment="${comment.id}" ${locked()}>Delete</button>` : ""}
        </div>
        ${replying ? `
          <div class="mt-2 space-y-2">
            <textarea id="reply-draft" class="input" spellcheck="true" rows="2" aria-label="Your reply">${escape(state.replyDraft)}</textarea>
            <div class="flex gap-2">
              <button class="btn btn-primary btn-sm" data-action="post-reply" ${state.replyDraft.trim() && !state.acting ? "" : "disabled"}>Post reply</button>
              <button class="btn btn-ghost btn-sm" data-action="cancel-reply">Cancel</button>
            </div>
          </div>` : ""}
        ${replies.length ? `<ul class="mt-1 border-l border-line pl-4">${replies.map((reply) => commentHtml(entry, reply, [])).join("")}</ul>` : ""}
      </div>
    </li>`;
}

function commentsHtml(entry: Entry): string {
  const data = dataFor(entry.id);
  const slot = data.comments;
  const enabled = !entry.info || entry.info.community_options & 1;
  const off = enabled ? "" : `
    <div class="notice mb-4 items-center justify-between">
      <span>Comments are turned off for this mod.</span>
      ${canManage(entry.id) ? `<button class="btn btn-secondary btn-sm" data-tab="page">Open Page</button>` : ""}
    </div>`;
  const composer = enabled && state.hasToken ? `
    <div class="mb-5 space-y-2">
      <textarea id="comment-draft" class="input" spellcheck="true" rows="3" placeholder="Write a comment" aria-label="Write a comment">${escape(state.commentDraft)}</textarea>
      <button class="btn btn-primary btn-sm" data-action="post-comment" ${state.commentDraft.trim() && !state.acting ? "" : "disabled"}>Post comment</button>
    </div>` : "";
  const wrap = (body: string) => `<div class="max-w-3xl">${off}${composer}${body}</div>`;
  if (slot.error) return wrap(failedHtml(slot.error));
  if (!slot.value) return wrap(loadingHtml());
  const all = slot.value;
  const byId = new Map(all.map((c) => [c.id, c]));
  const rootOf = (comment: Comment): number => {
    let current = comment;
    for (let parent = byId.get(current.reply_id); parent; parent = byId.get(current.reply_id)) current = parent;
    return current.id;
  };
  const threads = all.filter((c) => rootOf(c) === c.id);
  const repliesTo = (id: number) => all.filter((c) => c.id !== id && rootOf(c) === id).sort((a, b) => a.id - b.id);
  const older = data.commentsNext === null ? ""
    : `<button class="btn btn-secondary btn-sm mt-3" data-action="more-comments" ${slot.busy ? "disabled" : ""}>${slot.busy ? "Loading" : "Show older comments"}</button>`;
  return wrap(threads.length
    ? `<ul class="divide-y divide-line">${threads.map((c) => commentHtml(entry, c, repliesTo(c.id))).join("")}</ul>${older}`
    : `<p class="text-ink-muted">No comments yet.</p>`);
}

/* Team */

function teamHtml(entry: Entry): string {
  const slot = dataFor(entry.id).team;
  if (slot.error) return failedHtml(slot.error);
  if (!slot.value) return loadingHtml();
  const rows = slot.value.map((member) => `
    <li class="flex items-center gap-3 border-t border-line px-4 py-3 first:border-t-0">
      ${member.avatar ? `<img src="${escape(member.avatar)}" alt="" class="size-8 rounded-full bg-raised object-cover" />` : `<span class="size-8 rounded-full bg-raised"></span>`}
      <span class="min-w-0 flex-1">
        <span class="block truncate font-medium">${escape(member.username)}</span>
        <span class="block truncate text-[13px] text-ink-subtle">${escape(member.position || "Team member")}</span>
      </span>
      ${member.pending ? `<span class="badge badge-plain">Invited</span>` : ""}
      <span class="text-ink-muted">${LEVEL_NAMES[member.level] ?? "Member"}</span>
    </li>`).join("");
  return `
    <div class="max-w-2xl">
      ${slot.value.length ? `<ul class="card">${rows}</ul>` : `<p class="text-ink-muted">This mod has no team members.</p>`}
      <div class="mt-3 flex items-center justify-between gap-3">
        <p class="hint">mod.io only allows team changes on its website.</p>
        ${entry.info?.profile_url ? `<button class="btn btn-secondary btn-sm" data-action="open-profile">Open on mod.io${icon("external", 14)}</button>` : ""}
      </div>
    </div>`;
}

/* Advanced */

function advancedHtml(entry: Entry): string {
  const data = dataFor(entry.id);
  const deps = data.deps.error ? failedHtml(data.deps.error)
    : !data.deps.value ? loadingHtml()
    : data.deps.value.length ? `
      <ul class="card mb-3">
        ${data.deps.value.map((dep) => `
          <li class="flex items-center justify-between gap-3 border-t border-line px-4 py-2 first:border-t-0">
            <span class="min-w-0 truncate"><span class="font-medium">${escape(dep.name)}</span> <span class="text-ink-subtle tabular-nums">${dep.id}</span></span>
            <button class="btn btn-ghost btn-sm btn-icon" data-remove-dependency="${dep.id}" aria-label="Remove ${escape(dep.name)}" title="Remove" ${locked()}>${icon("trash")}</button>
          </li>`).join("")}
      </ul>` : `<p class="mb-3 text-ink-muted">This mod has no dependencies.</p>`;
  const metadata = data.metadata.error ? failedHtml(data.metadata.error)
    : !data.metadata.value ? loadingHtml()
    : data.metadata.value.length ? `
      <ul class="card mb-3">
        ${data.metadata.value.map((pair, index) => `
          <li class="flex items-center justify-between gap-3 border-t border-line px-4 py-2 first:border-t-0">
            <span class="min-w-0 truncate"><span class="font-medium">${escape(pair.key)}</span> <span class="text-ink-muted">${escape(pair.value)}</span></span>
            <button class="btn btn-ghost btn-sm btn-icon" data-remove-metadata="${index}" aria-label="Remove ${escape(pair.key)}" title="Remove" ${locked()}>${icon("trash")}</button>
          </li>`).join("")}
      </ul>` : `<p class="mb-3 text-ink-muted">This mod has no metadata.</p>`;
  return `
    <div class="max-w-2xl space-y-7">
      <section>
        <h2 class="font-semibold">Dependencies</h2>
        <p class="hint mb-3">Mods that players need alongside this one.</p>
        ${deps}
        <div class="flex gap-2">
          <input id="dependency-draft" class="input" autocomplete="off" inputmode="numeric" value="${escape(state.dependencyDraft)}" placeholder="Mod ID" aria-label="Mod ID" />
          <button class="btn btn-secondary" data-action="add-dependency" ${state.dependencyDraft.trim() && !state.acting ? "" : "disabled"}>Add</button>
        </div>
      </section>
      <section>
        <h2 class="font-semibold">Metadata</h2>
        <p class="hint mb-3">Key and value pairs stored on the mod.</p>
        ${metadata}
        <div class="flex gap-2">
          <input id="metadata-key" class="input" autocomplete="off" maxlength="255" value="${escape(state.metadataDraft.key)}" placeholder="Key" aria-label="Metadata key" />
          <input id="metadata-value" class="input" autocomplete="off" maxlength="255" value="${escape(state.metadataDraft.value)}" placeholder="Value" aria-label="Metadata value" />
          <button class="btn btn-secondary" data-action="add-metadata" ${state.metadataDraft.key.trim() && state.metadataDraft.value && !state.acting ? "" : "disabled"}>Add</button>
        </div>
      </section>
      <section class="card border-danger/40 p-5">
        <h2 class="font-semibold">Close this mod</h2>
        <p class="mt-1 mb-3 text-ink-muted">The mod disappears from the game and from this app. Only the mod.io website can restore it.</p>
        <button class="btn btn-danger" data-action="close-mod" ${locked()}>Close mod</button>
      </section>
    </div>`;
}

/* Size */

function sizeHtml(entry: Entry): string {
  const pick = (target: string) =>
    `<button data-report="${target}" aria-pressed="${state.reportTarget === target}">${platformHtml(TARGET_PLATFORMS[target])}</button>`;
  const head = `<div class="segmented mb-5" role="group" aria-label="Build">${Object.keys(TARGET_PLATFORMS).map(pick).join("")}</div>`;
  const slot = dataFor(entry.id).reports[state.reportTarget] ?? {};
  if (slot.error === NO_BUILD) return `${head}<p class="text-ink-muted">There's no ${platformName(TARGET_PLATFORMS[state.reportTarget])} build yet. Publish this mod to see its size.</p>`;
  if (slot.error) return `${head}${failedHtml(slot.error)}`;
  if (!slot.value) return `${head}<p class="flex items-center gap-2.5 py-2 text-ink-subtle"><span class="spinner"></span>Reading the build</p>`;
  const report = slot.value;
  const bar = (item: PakItem) => `
    <li class="grid grid-cols-[minmax(0,2fr)_minmax(0,3fr)_72px] items-center gap-3 px-4 py-2">
      <span class="truncate" title="${escape(item.name)}">${escape(item.name.split("/").pop() ?? item.name)}</span>
      <span class="meter meter-plain"><i style="width:${Math.max((item.bytes / Math.max(report.total_bytes, 1)) * 100, 0.5)}%"></i></span>
      <span class="text-right tabular-nums text-ink-muted">${megabytes(item.bytes)}</span>
    </li>`;
  const list = (title: string, items: PakItem[]) => `
    <section class="card">
      <h2 class="border-b border-line px-4 py-2.5 text-[13px] font-semibold">${title}</h2>
      <ul class="py-1">${items.map(bar).join("")}</ul>
    </section>`;
  return `${head}
    <p class="mb-4 text-ink-muted">${count(report.files)} files, ${megabytes(report.total_bytes)} in total.</p>
    <div class="space-y-4">
      ${list("By folder", report.folders.slice(0, 8))}
      ${list("Largest files", report.largest.slice(0, 12))}
    </div>`;
}

/* Source plugins */

function sourceHtml(): string {
  const { search, sort, mods, offset, total, busy, note } = state.source;
  const sorts = [["-date_live", "Newest"], ["-downloads_total", "Most downloads"], ["-subscribers_total", "Most subscribers"], ["-ratings_weighted_aggregate", "Top rated"], ["name", "Name"]];
  const card = (m: SourceMod) => `
    <li class="card flex flex-col overflow-hidden">
      ${m.logo ? `<img src="${escape(m.logo)}" alt="" class="aspect-video w-full bg-raised object-cover" />` : `<div class="aspect-video w-full bg-raised"></div>`}
      <div class="flex flex-1 flex-col p-3.5">
        <strong class="truncate font-semibold" title="${escape(m.name)}">${escape(m.name)}</strong>
        <span class="mt-0.5 truncate text-xs text-ink-subtle">${escape(m.author)}, ${megabytes(m.filesize)}</span>
        <p class="mt-2 line-clamp-2 flex-1 text-[13px] text-ink-muted">${escape(m.summary)}</p>
        <button class="btn btn-secondary btn-sm mt-3 w-full" data-download="${m.id}" ${busy ? "disabled" : ""}>${state.source.downloading === m.id ? `<span class="spinner size-3.5"></span>Downloading` : "Download"}</button>
      </div>
    </li>`;
  return `
    <div class="mx-auto max-w-5xl px-8 py-7">
      ${pageHeader("Source plugins", "Content shared by other modders to build into your mods.")}
      <div class="mb-4 flex gap-2">
        <div class="relative min-w-0 flex-1">
          <span class="pointer-events-none absolute top-2.5 left-3 text-ink-subtle">${icon("search")}</span>
          <input id="source-search" class="input pl-9" autocomplete="off" value="${escape(search)}" placeholder="Search source plugins" aria-label="Search source plugins" />
        </div>
        <select id="source-sort" class="input w-44 shrink-0" aria-label="Sort by">
          ${sorts.map(([id, label]) => `<option value="${id}" ${sort === id ? "selected" : ""}>${label}</option>`).join("")}
        </select>
      </div>
      ${note ? `<div class="mb-4">${noteHtml(note)}</div>` : ""}
      ${busy && !mods.length ? loadingHtml() : ""}
      ${!busy && !mods.length && !note ? `<p class="text-ink-muted">Nothing matches that search.</p>` : ""}
      <ul class="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">${mods.map(card).join("")}</ul>
      ${total ? `
        <div class="mt-5 flex items-center justify-between text-ink-muted">
          <button class="btn btn-secondary btn-sm" data-page="-24" ${offset === 0 ? "disabled" : ""}>Previous</button>
          <span class="tabular-nums">${offset + 1} to ${Math.min(offset + 24, total)} of ${total}</span>
          <button class="btn btn-secondary btn-sm" data-page="24" ${offset + 24 >= total ? "disabled" : ""}>Next</button>
        </div>` : ""}
    </div>`;
}

/* The run panel */

function wasStopped(run: Run): boolean {
  return Boolean(run.result?.message.startsWith("Stopped"));
}

/** Name what broke and where, such as "Cook failed on Linux". */
function failureTitle(run: Run): string {
  const failure = run.result?.failure;
  const title = failure
    ? FAILURE_NAMES[failure.kind] ?? "Build failed"
    : STAGE_FAILURES[run.stage] ?? (run.action === "stage" ? "Staging failed" : run.action === "share" ? "Sharing failed" : "Publish failed");
  const target = failure ? failure.target : ["cook", "upload"].includes(run.stage) ? run.target : "";
  return target ? `${title} on ${targetName(target)}` : title;
}

function headline(run: Run): string {
  const build = targetName(run.target);
  if (run.result) {
    if (run.result.ok) return run.action === "stage" ? "Staged" : run.action === "share" ? "Shared" : run.inactive ? "Uploaded" : "Published";
    if (wasStopped(run)) return "Stopped";
    return failureTitle(run);
  }
  if (run.stopping) return "Stopping";
  switch (run.stage) {
    case "check": return "Checking the mod";
    case "version": return "Updating the version";
    case "cook": return build ? `Cooking ${build}` : "Cooking";
    case "package": return "Packaging";
    case "upload": return build ? `Uploading ${build}` : "Uploading";
    case "go-live": return "Going live";
    default: return "Working";
  }
}

function resultDetail(run: Run): string {
  if (!run.result) return "";
  if (run.result.ok) {
    if (run.action === "stage") return "Your build is ready to test.";
    if (run.action === "share") return "Other modders can now find it under Browse source plugins.";
    return run.inactive ? "Make the files live from the Files tab." : "";
  }
  if (wasStopped(run)) return run.result.message.replace(/^Stopped\.\s*/, "");
  return run.result.failure?.summary ?? run.result.message;
}

/** The errors the tool printed, and where to read the rest. */
function failureHtml(run: Run): string {
  const failure = run.result?.failure;
  if (!failure) return "";
  const extra = failure.errors.filter((line) => line !== failure.summary);
  const shown = extra.slice(0, ERRORS_SHOWN);
  const facts = [failure.tool, failure.exit_code === null ? "" : `exit code ${failure.exit_code}`].filter(Boolean).join(", ");
  return `
    <div class="mt-2 pl-7">
      ${shown.length ? `
        <ul class="max-h-28 space-y-0.5 overflow-y-auto font-mono text-xs break-words text-danger select-text">
          ${shown.map((line) => `<li>${escape(line)}</li>`).join("")}
        </ul>
        ${extra.length > shown.length ? `<p class="hint mt-1">${extra.length - shown.length} more in the log.</p>` : ""}` : ""}
      <div class="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1">
        <span class="hint">${escape(facts)}</span>
        ${failure.log_path ? `<button class="btn btn-secondary btn-sm" data-action="reveal-log">Show log file</button>` : ""}
      </div>
    </div>`;
}

function stepsHtml(run: Run): string {
  const steps = STAGES.filter((s) => s.id !== "go-live" || (run.together && !run.inactive));
  const current = steps.findIndex((s) => s.id === run.stage);
  const items = steps.map((s, index) => {
    const ended = index === current && run.result && !run.result.ok;
    const stopped = ended && wasStopped(run);
    const failed = ended && !stopped;
    const done = run.result?.ok || index < current;
    const active = index === current && !run.result;
    const marker = stopped
      ? `<span class="text-ink-subtle">${icon("alert", 14)}</span>`
      : failed
      ? `<span class="text-danger">${icon("close", 14)}</span>`
      : done ? `<span class="text-ok">${icon("check", 14)}</span>`
      : active ? `<span class="spinner size-3.5"></span>`
      : `<span class="mx-[3px] size-2 rounded-full bg-line-strong"></span>`;
    const tone = active || stopped ? "font-medium text-ink" : done ? "text-ink-muted" : failed ? "font-medium text-danger" : "text-ink-subtle";
    return `<li class="flex items-center gap-1.5 text-[13px] ${tone}" ${active ? 'aria-current="step"' : ""}>${marker}${s.label}</li>`;
  });
  return `<ol class="flex flex-wrap items-center gap-x-5 gap-y-1">${items.join("")}</ol>`;
}

function buildsHtml(run: Run): string {
  if (run.stage !== "upload" && run.stage !== "go-live" && !run.result) return "";
  if (!Object.keys(run.builds).length) return "";
  const columns = run.targets.map((target) => {
    const build = run.builds[target];
    const finished = run.result?.ok || (build && build.total > 0 && build.done >= build.total);
    const share = finished ? 100 : build?.total ? (build.done / build.total) * 100 : 0;
    const name = targetName(target);
    const text = finished ? "Done" : build ? `${build.done} of ${build.total} chunks` : "Waiting";
    return `
      <div class="min-w-0">
        <div class="mb-1.5 flex items-baseline justify-between gap-2 text-[13px]">
          <span class="min-w-0 font-medium">${platformHtml(TARGET_PLATFORMS[target] ?? target)}</span>
          <span class="shrink-0 text-xs text-ink-subtle tabular-nums">${text}</span>
        </div>
        <span class="meter ${finished ? "complete" : ""}" role="progressbar" aria-label="${name}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.round(share)}">
          <i style="width:${share}%"></i>
        </span>
      </div>`;
  }).join("");
  return `<div class="mt-3.5 grid grid-cols-2 gap-x-6 gap-y-3 ${run.targets.length > 2 ? "lg:grid-cols-4" : ""}">${columns}</div>`;
}

function runHtml(run: Run): string {
  const lead = !run.result
    ? `<span class="spinner"></span>`
    : run.result.ok ? `<span class="text-ok">${icon("check")}</span>`
    : wasStopped(run) ? `<span class="text-ink-subtle">${icon("alert")}</span>`
    : `<span class="text-danger">${icon("alert")}</span>`;
  const detail = resultDetail(run);
  const failure = run.result && !run.result.ok && !wasStopped(run);
  const counts = [
    run.errors ? `${count(run.errors)} ${run.errors === 1 ? "error" : "errors"}` : "",
    run.warnings ? `${count(run.warnings)} ${run.warnings === 1 ? "warning" : "warnings"}` : "",
  ].filter(Boolean).join(", ");
  return `
    <div class="border-t border-line bg-panel px-8 py-3.5">
      <div class="flex items-center gap-3">
        ${lead}
        <div class="min-w-0 flex-1">
          <div class="flex items-baseline gap-2">
            <strong class="shrink-0 font-semibold">${escape(headline(run))}</strong>
            <span class="min-w-0 truncate text-[13px] text-ink-subtle">${escape(run.modName)}</span>
          </div>
        </div>
        <span id="run-elapsed" class="shrink-0 text-[13px] text-ink-subtle tabular-nums">${duration((run.endedAt ?? Date.now()) - run.startedAt)}</span>
        <button class="btn btn-ghost btn-sm" data-action="toggle-log" aria-expanded="${run.showLog}">Log</button>
        ${run.result
          ? `<button class="btn btn-ghost btn-sm btn-icon" data-action="dismiss-run" aria-label="Dismiss" title="Dismiss">${icon("close")}</button>`
          : `<button class="btn btn-secondary btn-sm" data-action="cancel" ${run.stopping ? "disabled" : ""}>Stop</button>`}
      </div>
      ${detail ? `<p class="mt-1.5 max-h-24 overflow-y-auto pl-7 text-[13px] break-words select-text ${failure ? "text-danger" : "text-ink-muted"}">${escape(detail)}</p>` : ""}
      ${failureHtml(run)}
      ${run.action === "publish" ? `<div class="mt-3">${stepsHtml(run)}</div>` : ""}
      ${buildsHtml(run)}
      ${run.showLog ? `
        <div class="mt-3">
          <div class="mb-1.5 flex items-center justify-between gap-3">
            <label class="flex cursor-pointer items-center gap-2 text-[13px]">
              <input type="checkbox" id="run-verbose" ${run.verbose ? "checked" : ""} />Verbose
            </label>
            <span class="flex min-w-0 items-center gap-3">
              <span id="run-counts" class="truncate text-xs text-ink-subtle tabular-nums">${counts}</span>
              <button class="btn btn-secondary btn-sm" data-action="copy-log">${run.copyLabel}</button>
            </span>
          </div>
          <pre id="run-log" class="h-[min(12rem,24vh)] overflow-y-auto rounded-md border border-line bg-canvas p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap text-ink-muted" tabindex="0" aria-label="Log"></pre>
        </div>` : ""}
    </div>`;
}

/* The mod view */

function tabsFor(entry: Entry): Tab[] {
  if (!canManage(entry.id)) return ["overview", "files", "comments", "team"];
  const remote: Tab[] = ["overview", "files", "page", "media", "comments", "team", "advanced"];
  const shares: Tab[] = entry.kind === "Mod" ? ["source"] : [];
  return entry.local ? ["overview", "publish", "cook", ...shares, ...remote.slice(1), "size"] : remote;
}

function activeTab(entry: Entry): Tab {
  return tabsFor(entry).includes(state.tab) ? state.tab : "overview";
}

function modHtml(entry: Entry): string {
  const info = entry.info;
  const logo = info?.logo
    ? `<img src="${escape(info.logo)}" alt="" class="aspect-video w-24 shrink-0 rounded-md bg-raised object-cover" />`
    : "";
  const tab = activeTab(entry);
  const panels: Record<Tab, () => string> = {
    overview: () => overviewHtml(entry), publish: () => publishHtml(entry), cook: () => cookHtml(entry), source: () => shareHtml(entry), files: () => filesHtml(entry),
    page: () => pageHtml(entry), media: () => mediaHtml(entry), comments: () => commentsHtml(entry),
    team: () => teamHtml(entry), advanced: () => advancedHtml(entry), size: () => sizeHtml(entry),
  };
  const readOnly = !canManage(entry.id)
    ? `<div class="notice mb-5">You aren't on this mod's team, so you can only view it.</div>` : "";
  return `
    <div class="mx-auto max-w-5xl px-8 pt-7 pb-8">
      <header class="mb-5 flex items-center gap-4">
        ${logo}
        <div class="min-w-0 flex-1">
          <h1 class="truncate text-xl font-semibold tracking-tight" title="${escape(entry.name)}">${escape(entry.name)}</h1>
          <p class="mt-0.5 flex items-center gap-2 whitespace-nowrap text-ink-subtle tabular-nums">${escape(kindName(entry.kind))} ${entry.id}${info && !info.visible ? `<span class="badge badge-plain">Hidden</span>` : ""}</p>
        </div>
        <button class="btn btn-secondary btn-sm" data-action="refresh-mod" aria-label="Refresh" title="Refresh this mod" ${state.refreshing || !state.hasToken || state.cooling.mod || waiting() ? "disabled" : ""}>
          ${state.refreshing ? `<span class="spinner size-3.5"></span>` : icon("refresh", 14)}<span class="max-lg:hidden">Refresh</span>
        </button>
        ${info?.profile_url ? `<button class="btn btn-secondary btn-sm" data-action="open-profile" aria-label="View on mod.io" title="View on mod.io"><span class="max-lg:hidden">View on mod.io</span>${icon("external", 14)}</button>` : ""}
      </header>
      ${readOnly}
      <div class="mb-6 flex flex-wrap gap-x-5 gap-y-1 border-b border-line" role="tablist" aria-label="Mod sections">
        ${tabsFor(entry).map((id) => `<button class="tab" role="tab" id="tab-${id}" data-tab="${id}" aria-selected="${tab === id}" aria-controls="tab-panel" tabindex="${tab === id ? 0 : -1}">${TAB_NAMES[id]}</button>`).join("")}
      </div>
      <div id="tab-panel" role="tabpanel" aria-labelledby="tab-${tab}">${panels[tab]()}</div>
    </div>`;
}

/* Cook: what goes into the build */

function blankTree(): CookView {
  return {
    report: null, root: null, error: "", loading: false, analysing: false, excluded: new Set(), pinned: new Set(), expanded: new Set([""]),
    selected: new Set(), anchor: "", cursor: "", query: "", view: "all", undo: [], redo: [], built: null, note: null, share: null,
  };
}

function cookFor(id: number): CookView {
  let view = cookViews.get(id);
  if (!view) {
    view = blankTree();
    cookViews.set(id, view);
  }
  return view;
}

function shareFor(id: number): CookView {
  let view = shareViews.get(id);
  if (!view) {
    view = blankTree();
    view.expanded.add("Content");
    view.share = {
      descriptor: "", changeNote: "", check: null, needed: new Set(), checking: false, ticket: 0,
      changes: new Map(), gone: [], removed: [],
    };
    shareViews.set(id, view);
  }
  return view;
}

/** Cook and Source use one tree, and only one of them is ever on screen. */
function treeFor(id: number): CookView {
  return activeTabIs("source") ? shareFor(id) : cookFor(id);
}

function buildTree(assets: Asset[]): TreeFolder {
  const root: TreeFolder = { path: "", name: "", folders: [], assets: [], all: [], size: 0 };
  const byPath = new Map<string, TreeFolder>([["", root]]);
  for (const asset of assets) {
    const parts = asset.path.split("/");
    let folder = root;
    root.all.push(asset.path);
    root.size += asset.size;
    for (let depth = 0; depth < parts.length - 1; depth += 1) {
      const path = parts.slice(0, depth + 1).join("/");
      let next = byPath.get(path);
      if (!next) {
        next = { path, name: parts[depth], folders: [], assets: [], all: [], size: 0 };
        byPath.set(path, next);
        folder.folders.push(next);
      }
      next.all.push(asset.path);
      next.size += asset.size;
      folder = next;
    }
    folder.assets.push(asset);
  }
  const order = (a: { name?: string; path: string }, b: { name?: string; path: string }) =>
    a.path.localeCompare(b.path, undefined, { sensitivity: "base", numeric: true });
  for (const folder of byPath.values()) {
    folder.folders.sort(order);
    folder.assets.sort(order);
  }
  return root;
}

function shownInView(cook: CookView, asset: Asset): boolean {
  const query = cook.query.trim().toLowerCase();
  if (query && !asset.path.toLowerCase().includes(query)) return false;
  switch (cook.view) {
    case "left-out": return cook.excluded.has(asset.path);
    case "unused": return asset.status === "unused";
    case "in_string": return asset.status === "in_string";
    case "not-built": return Boolean(cook.built) && !cook.built!.has(asset.path);
    case "pinned": return cook.pinned.has(asset.path);
    case "shared": return !cook.excluded.has(asset.path);
    case "needed": return Boolean(cook.share?.needed.has(asset.path));
    case "changed": return Boolean(cook.share?.changes.has(asset.path)) && !cook.excluded.has(asset.path);
    default: return true;
  }
}

/** The rows on screen, in order. A search or view opens every folder that has a match. */
function visibleRows(cook: CookView): TreeRow[] {
  const rows: TreeRow[] = [];
  const filtering = Boolean(cook.query.trim()) || cook.view !== "all";
  const assetsByPath = new Map(cook.report?.assets.map((asset) => [asset.path, asset]));
  const visit = (folder: TreeFolder, depth: number) => {
    for (const child of folder.folders) {
      if (filtering && !child.all.some((path) => shownInView(cook, assetsByPath.get(path)!))) continue;
      rows.push({ key: `d:${child.path}`, depth, folder: child, asset: null });
      if (filtering || cook.expanded.has(child.path)) visit(child, depth + 1);
    }
    for (const asset of folder.assets) {
      if (!filtering || shownInView(cook, asset)) rows.push({ key: `a:${asset.path}`, depth, folder: null, asset });
    }
  };
  if (cook.root) visit(cook.root, 0);
  return rows;
}

function assetsOf(row: TreeRow): string[] {
  return row.folder ? row.folder.all : [row.asset!.path];
}

function selectedAssets(cook: CookView, rows: TreeRow[]): string[] {
  const paths = new Set<string>();
  for (const row of rows) {
    if (cook.selected.has(row.key)) assetsOf(row).forEach((path) => paths.add(path));
  }
  return [...paths];
}

function assetRowHtml(cook: CookView, row: TreeRow, index: number): string {
  const paths = assetsOf(row);
  const left = paths.filter((path) => cook.excluded.has(path)).length;
  const checked = left === 0;
  const mixed = left > 0 && left < paths.length;
  const pinned = Boolean(row.asset) && cook.pinned.has(row.asset!.path);
  const required = row.asset?.path === "definition" || pinned;
  const name = row.folder ? row.folder.name : row.asset!.path.split("/").pop()!;
  const open = row.folder && (cook.expanded.has(row.folder.path) || cook.query.trim() || cook.view !== "all");
  const rowStatus = row.asset && (cook.share ? shareStatus(cook, row.asset) : row.asset.status);
  const status = rowStatus ? STATUS_NAMES[rowStatus] : `${count(paths.length - left)} of ${count(paths.length)}`;
  const size = row.folder ? row.folder.size : row.asset!.size;
  const built = cook.built && row.asset ? (cook.built.has(row.asset.path) ? icon("check", 14) : "") : "";
  const selected = cook.selected.has(row.key);
  return `
    <div class="asset-row ${selected ? "asset-row-selected" : ""} ${cook.cursor === row.key ? "asset-row-cursor" : ""} ${left === paths.length ? "text-ink-subtle" : ""}"
      role="treeitem" id="asset-row-${index}" data-row="${escape(row.key)}" aria-level="${row.depth + 1}" aria-selected="${selected}"
      ${row.folder ? `aria-expanded="${Boolean(open)}"` : ""} style="padding-left:${row.depth * 18 + 6}px">
      <span class="asset-twist" ${row.folder ? "data-twist" : ""}>${row.folder ? `<span class="${open ? "" : "-rotate-90"} inline-block">${icon("chevron", 14)}</span>` : ""}</span>
      <input type="checkbox" data-cook tabindex="-1" aria-label="${cook.share ? `Share ${escape(name)}` : `Include ${escape(name)} in the build`}" ${checked ? "checked" : ""} ${mixed ? "data-mixed" : ""} ${required ? "disabled" : ""} ${cook.share && required ? `title="Always shared, because a download can't be installed without it"` : ""} />
      <span class="text-ink-subtle">${icon(row.folder ? "folder" : "file", 15)}</span>
      <span class="asset-name" title="${escape(name)}">${escape(name)}</span>
      ${pinned && !cook.share ? `<span class="text-accent" title="Always kept">${icon("pin", 14)}</span>` : ""}
      <span class="asset-status status-${rowStatus ?? "folder"}">${status}</span>
      ${cook.share ? "" : `<span class="asset-built text-ok">${built}</span>`}
      <span class="asset-size">${megabytes(size)}</span>
    </div>`;
}

function modNameOf(id: number): string {
  return state.infos[id]?.name || state.mods.find((m) => m.id === id)?.name || `Mod ${id}`;
}

/** Turn `UGC123:path` into words. */
function outsideName(label: string): string {
  const [plugin, ...rest] = label.split(":");
  return `${modNameOf(Number(plugin.replace("UGC", "")))}: ${rest.join(":")}`;
}

function broken(cook: CookView): { kept: string; missing: Asset }[] {
  const assets = cook.report?.assets ?? [];
  const found: { kept: string; missing: Asset }[] = [];
  for (const asset of assets) {
    if (cook.excluded.has(asset.path)) {
      asset.outside.forEach((label) => found.push({ kept: outsideName(label), missing: asset }));
      continue;
    }
    for (const index of asset.uses) {
      if (cook.excluded.has(assets[index].path)) found.push({ kept: asset.path, missing: assets[index] });
    }
  }
  return found;
}

function usedByHtml(cook: CookView): string {
  const consumers = cook.report?.consumers ?? [];
  if (!consumers.length) return "";
  const rows = consumers.map((consumer) => `
    <li class="py-1">
      <span class="font-medium">${escape(modNameOf(consumer.mod_id))}</span>
      <span class="text-ink-subtle">uses ${count(consumer.assets)} ${consumer.assets === 1 ? "asset" : "assets"} directly</span>
    </li>`).join("");
  return `<section class="card mb-3 px-3 py-2 text-[13px]" aria-label="Mods that use this one"><ul class="divide-y divide-line">${rows}</ul></section>`;
}

function assetDetailHtml(cook: CookView, rows: TreeRow[]): string {
  if (cook.share) return shareDetailHtml(cook, rows);
  const row = rows.find((item) => item.key === cook.cursor);
  if (!row) return `<p class="hint">Select an asset to see what uses it.</p>`;
  if (row.folder) {
    const left = row.folder.all.filter((path) => cook.excluded.has(path)).length;
    return `<p class="font-medium break-words">${escape(row.folder.path)}</p>
      <p class="hint">${count(row.folder.all.length)} assets, ${megabytes(row.folder.size)}, ${count(left)} left out.</p>`;
  }
  const asset = row.asset!;
  return `
    <p class="font-medium break-words">${escape(asset.path)}</p>
    <p class="hint">${cook.pinned.has(asset.path) ? "Always kept, along with everything it uses." : STATUS_HINTS[asset.status]}</p>
    ${asset.outside.length ? `
      <p class="mt-2 text-[13px] font-medium">Used by other mods</p>
      <ul class="hint">${asset.outside.map((label) => `<li class="truncate" title="${escape(outsideName(label))}">${escape(outsideName(label))}</li>`).join("")}</ul>` : ""}
    ${asset.used_by.length ? `
      <p class="mt-2 text-[13px] font-medium">Used by</p>
      <ul class="hint">${asset.used_by.map((name) => `<li class="truncate" title="${escape(name)}">${escape(name)}</li>`).join("")}</ul>` : ""}`;
}

function cookSummary(cook: CookView): string {
  if (cook.share) return shareSummary(cook);
  const assets = cook.report?.assets ?? [];
  const kept = assets.filter((asset) => !cook.excluded.has(asset.path));
  const size = (items: Asset[]) => items.reduce((total, asset) => total + asset.size, 0);
  return `${count(kept.length)} of ${count(assets.length)} assets in the build, ${megabytes(size(kept))} of ${megabytes(size(assets))}`;
}

function treeHtml(cook: CookView): string {
  const rows = visibleRows(cook);
  if (!rows.length) return `<p class="p-4 text-ink-muted">Nothing matches.</p>`;
  return rows.map((row, index) => assetRowHtml(cook, row, index)).join("");
}

function cookHtml(entry: Entry): string {
  const cook = cookFor(entry.id);
  if (cook.error && !cook.report) return failedHtml(cook.error);
  if (!cook.report) return `<p class="flex items-center gap-2.5 py-2 text-ink-subtle"><span class="spinner"></span>Reading the mod's assets</p>`;
  const rows = visibleRows(cook);
  const picked = selectedAssets(cook, rows).length;
  const problems = broken(cook);
  const noRoot = cook.report.analysed && !cook.report.rooted && !cook.report.consumers.length;
  const views: [AssetView, string][] = [["all", "All assets"], ["left-out", "Left out"], ["unused", "Unused"], ["in_string", "Only in a string"], ["pinned", "Always keep"]];
  if (cook.built) views.push(["not-built", "Not in last build"]);
  return `
    <div class="mb-3 flex flex-wrap items-center gap-2">
      <button class="btn btn-primary btn-sm" data-action="filter-used" ${cook.analysing || noRoot || isRunning() ? "disabled" : ""} title="${noRoot ? "Nothing shows what this mod's assets are for" : "Leave out everything nothing uses"}">
        ${cook.analysing ? `<span class="spinner size-3.5"></span>Reading references` : `${icon("filter", 14)}Filter only used`}
      </button>
      <button class="btn btn-secondary btn-sm" data-action="keep-all" ${cook.excluded.size ? "" : "disabled"}>Keep everything</button>
      <span class="flex">
      <button class="btn btn-ghost btn-sm btn-icon" data-action="recheck" aria-label="Check references again" title="Check references again" ${cook.analysing || !cook.report.analysed ? "disabled" : ""}>${icon("refresh")}</button>
      <button class="btn btn-ghost btn-sm btn-icon" data-action="cook-undo" aria-label="Undo" title="Undo (Ctrl+Z)" ${cook.undo.length ? "" : "disabled"}>${icon("undo")}</button>
      <button class="btn btn-ghost btn-sm btn-icon" data-action="cook-redo" aria-label="Redo" title="Redo (Ctrl+Y)" ${cook.redo.length ? "" : "disabled"}><span class="inline-block -scale-x-100">${icon("undo")}</span></button>
      <button class="btn btn-ghost btn-sm btn-icon" data-action="expand-all" aria-label="Expand all" title="Expand all">${icon("expand")}</button>
      <button class="btn btn-ghost btn-sm btn-icon" data-action="collapse-all" aria-label="Collapse all" title="Collapse all">${icon("collapse")}</button>
      </span>
      <span class="flex-1"></span>
      <input id="asset-search" class="input h-8 w-28 lg:w-36" autocomplete="off" value="${escape(cook.query)}" placeholder="Search assets" aria-label="Search assets" />
      <select id="asset-view" class="input h-8 w-28 lg:w-36" aria-label="Show">
        ${views.map(([id, label]) => `<option value="${id}" ${cook.view === id ? "selected" : ""}>${label}</option>`).join("")}
      </select>
    </div>
    ${cook.note ? `<div class="mb-3">${noteHtml(cook.note)}</div>` : ""}
    ${noRoot ? `<div class="notice mb-3">This mod's definition doesn't lead to its assets, and no mod in your modkit uses them. Nothing here can tell which matter, so choose by hand.</div>` : ""}
    ${usedByHtml(cook)}
    ${problems.length ? `
      <div class="notice notice-failed mb-3 items-center justify-between">
        <span>${count(problems.length)} kept ${problems.length === 1 ? "asset points" : "assets point"} at something you left out, such as ${escape(problems[0].kept)} using ${escape(problems[0].missing.path)}.</span>
        <button class="btn btn-secondary btn-sm" data-action="keep-missing">Keep what they need</button>
      </div>` : ""}
    <div class="card overflow-hidden">
      <div class="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-line px-3 py-2 text-[13px] text-ink-muted">
        <span class="flex-1 whitespace-nowrap" id="cook-summary" title="Sizes are before cooking">${cookSummary(cook)}</span>
        <span id="cook-picked" class="${picked ? "" : "hidden"} flex flex-wrap items-center gap-2">
          <span class="tabular-nums">${count(picked)} selected</span>
          <button class="btn btn-secondary btn-sm" data-action="include-picked">Include</button>
          <button class="btn btn-secondary btn-sm" data-action="exclude-picked">Leave out</button>
          <button class="btn btn-secondary btn-sm" data-action="pin-picked" title="Keep these and everything they use, even if nothing seems to use them">Always keep</button>
          <button class="btn btn-ghost btn-sm" data-action="unpin-picked">Stop keeping</button>
        </span>
      </div>
      <div id="asset-scroll" class="h-[max(16rem,calc(100vh-34rem))] overflow-y-auto">
        <div class="asset-head"><span class="flex-1 pl-14">Name</span><span class="asset-status">Status</span><span class="asset-built">Built</span><span class="asset-size">Size</span></div>
        <div id="asset-tree" class="outline-none" role="tree" aria-label="Assets" aria-multiselectable="true" tabindex="0">${treeHtml(cook)}</div>
      </div>
      <div id="asset-detail" class="max-h-32 overflow-y-auto border-t border-line px-3 py-2 text-[13px] select-text">${assetDetailHtml(cook, rows)}</div>
    </div>
    <p class="hint mt-2">Ctrl or Shift click to select, Ctrl+A for all, Space to switch, Delete to leave out, P to always keep. This only changes the build. Nothing is removed from your project.</p>`;
}

function leftOutNotice(entry: Entry): string {
  const cook = cookViews.get(entry.id);
  if (!cook?.excluded.size) return "";
  return `
    <div class="notice items-center justify-between">
      <span>This build leaves out ${count(cook.excluded.size)} ${cook.excluded.size === 1 ? "asset" : "assets"}.</span>
      <button class="btn btn-secondary btn-sm" data-tab="cook">Review</button>
    </div>`;
}

/** Update the tree alone, which matters when it holds thousands of rows. */
function renderTree(cook: CookView): void {
  const tree = main.querySelector<HTMLElement>("#asset-tree");
  if (!tree) return render();
  morph(tree, treeHtml(cook));
  syncMixed();
  const rows = visibleRows(cook);
  const detail = main.querySelector<HTMLElement>("#asset-detail");
  if (detail) morph(detail, assetDetailHtml(cook, rows));
  const summary = main.querySelector("#cook-summary");
  if (summary) summary.textContent = cookSummary(cook);
  const picked = selectedAssets(cook, rows).length;
  const bar = main.querySelector("#cook-picked");
  if (bar) {
    bar.classList.toggle("hidden", !picked);
    bar.querySelector("span")!.textContent = `${count(picked)} selected`;
  }
  const index = rows.findIndex((row) => row.key === cook.cursor);
  tree.setAttribute("aria-activedescendant", index >= 0 ? `asset-row-${index}` : "");
}

/** Moving the selection changes highlights only, which must stay instant in a big tree. */
function paintSelection(cook: CookView): void {
  const tree = main.querySelector<HTMLElement>("#asset-tree");
  if (!tree) return;
  tree.querySelectorAll<HTMLElement>("[data-row]").forEach((row) => {
    const key = row.dataset.row!;
    const selected = cook.selected.has(key);
    row.classList.toggle("asset-row-selected", selected);
    row.classList.toggle("asset-row-cursor", key === cook.cursor);
    row.setAttribute("aria-selected", String(selected));
    if (key === cook.cursor) tree.setAttribute("aria-activedescendant", row.id);
  });
  const rows = visibleRows(cook);
  const detail = main.querySelector<HTMLElement>("#asset-detail");
  if (detail) morph(detail, assetDetailHtml(cook, rows));
  const picked = selectedAssets(cook, rows).length;
  const bar = main.querySelector("#cook-picked");
  if (bar) {
    bar.classList.toggle("hidden", !picked);
    bar.querySelector("span")!.textContent = `${count(picked)} selected`;
  }
}

function showCursor(): void {
  main.querySelector("#asset-tree .asset-row-cursor")?.scrollIntoView({ block: "nearest" });
}

const selectionTimers = new Map<number, number>();

function selectionOf(cook: CookView, confirmed: boolean) {
  return { excluded: [...cook.excluded], confirmed, pinned: [...cook.pinned] };
}

/** Everything a pinned asset needs stays too, or keeping it would be pointless. */
function pinnedClosure(cook: CookView): Set<string> {
  const assets = cook.report?.assets ?? [];
  const byPath = new Map(assets.map((asset) => [asset.path, asset]));
  const kept = new Set<string>();
  const stack = [...cook.pinned];
  while (stack.length) {
    const path = stack.pop()!;
    if (kept.has(path)) continue;
    kept.add(path);
    byPath.get(path)?.uses.forEach((index) => stack.push(assets[index].path));
  }
  return kept;
}

function setPinned(id: number, cook: CookView, paths: string[], pinned: boolean): void {
  for (const path of paths) {
    if (pinned) cook.pinned.add(path);
    else cook.pinned.delete(path);
  }
  if (pinned) changeExcluded(id, cook, (excluded) => pinnedClosure(cook).forEach((path) => excluded.delete(path)));
  saveCookSelection(id, cook);
  render();
}

function saveCookSelection(id: number, cook: CookView): void {
  if (cook.share) return saveShare(id, cook);
  window.clearTimeout(selectionTimers.get(id));
  selectionTimers.set(id, window.setTimeout(async () => {
    selectionTimers.delete(id);
    try {
      await invoke("store_selection", { modId: id, selection: selectionOf(cook, false) });
      await refreshPending();
      if (selectedEntry()?.id === id && activeTabIs("publish")) render();
    } catch (error) {
      cook.note = { ok: false, text: String(error) };
      render();
    }
  }, 200));
}

/** Save a waiting change now, before something reads the selection from disk. */
async function flushSelection(id: number, cook: CookView): Promise<void> {
  if (!selectionTimers.has(id)) return;
  window.clearTimeout(selectionTimers.get(id));
  selectionTimers.delete(id);
  await invoke("store_selection", { modId: id, selection: selectionOf(cook, false) });
}

function activeTabIs(tab: Tab): boolean {
  const entry = selectedEntry();
  return Boolean(entry) && state.view === "mod" && activeTab(entry!) === tab;
}

/** Change what's left out. Every change can be undone. */
function changeExcluded(id: number, cook: CookView, update: (excluded: Set<string>) => void): void {
  const before = [...cook.excluded];
  cook.note = null;
  update(cook.excluded);
  cook.excluded.delete("definition");
  pinnedClosure(cook).forEach((path) => cook.excluded.delete(path));
  if (before.length === cook.excluded.size && before.every((path) => cook.excluded.has(path))) return;
  cook.undo.push(before);
  if (cook.undo.length > HISTORY_LIMIT) cook.undo.shift();
  cook.redo = [];
  saveCookSelection(id, cook);
}

function stepHistory(id: number, cook: CookView, from: string[][], to: string[][]): void {
  const previous = from.pop();
  if (!previous) return;
  to.push([...cook.excluded]);
  cook.excluded = new Set(previous);
  cook.note = null;
  pinnedClosure(cook).forEach((path) => cook.excluded.delete(path));
  saveCookSelection(id, cook);
  render();
}

function setIncluded(id: number, cook: CookView, paths: string[], included: boolean): void {
  changeExcluded(id, cook, (excluded) => {
    for (const path of paths) {
      if (included) excluded.delete(path);
      else excluded.add(path);
    }
  });
  render();
}

function selectRow(cook: CookView, key: string, adding: boolean, extending: boolean): void {
  const rows = visibleRows(cook);
  const at = rows.findIndex((row) => row.key === key);
  if (at < 0) return;
  if (extending) {
    const from = Math.max(rows.findIndex((row) => row.key === cook.anchor), 0);
    const range = rows.slice(Math.min(from, at), Math.max(from, at) + 1).map((row) => row.key);
    if (!adding) cook.selected.clear();
    range.forEach((item) => cook.selected.add(item));
  } else if (adding) {
    if (cook.selected.has(key)) cook.selected.delete(key);
    else cook.selected.add(key);
    cook.anchor = key;
  } else {
    cook.selected = new Set([key]);
    cook.anchor = key;
  }
  cook.cursor = key;
  paintSelection(cook);
}

function toggleFolder(cook: CookView, path: string): void {
  if (cook.expanded.has(path)) cook.expanded.delete(path);
  else cook.expanded.add(path);
  renderTree(cook);
}

function treeKey(event: KeyboardEvent, id: number, cook: CookView): void {
  const rows = visibleRows(cook);
  const at = rows.findIndex((row) => row.key === cook.cursor);
  const row = rows[at];
  const ctrl = event.ctrlKey || event.metaKey;
  const key = event.key.toLowerCase();
  const go = (index: number) => {
    const next = rows[Math.min(Math.max(index, 0), rows.length - 1)];
    if (!next) return;
    if (ctrl && !event.shiftKey) {
      cook.cursor = next.key;
      paintSelection(cook);
    } else {
      selectRow(cook, next.key, false, event.shiftKey);
    }
    showCursor();
  };
  const page = Math.max(Math.floor((main.querySelector("#asset-tree")?.clientHeight ?? 300) / 30) - 1, 1);
  const picked = () => {
    const paths = selectedAssets(cook, rows);
    return paths.length ? paths : row ? assetsOf(row) : [];
  };
  let handled = true;
  if (ctrl && key === "a") {
    cook.selected = new Set(rows.map((item) => item.key));
    paintSelection(cook);
  } else if (ctrl && key === "z" && !event.shiftKey) stepHistory(id, cook, cook.undo, cook.redo);
  else if (ctrl && (key === "y" || (key === "z" && event.shiftKey))) stepHistory(id, cook, cook.redo, cook.undo);
  else if (key === "arrowdown") go(at + 1);
  else if (key === "arrowup") go(at < 0 ? 0 : at - 1);
  else if (key === "home") go(0);
  else if (key === "end") go(rows.length - 1);
  else if (key === "pagedown") go(at + page);
  else if (key === "pageup") go(at - page);
  else if (key === "arrowright" && row?.folder) {
    if (!cook.expanded.has(row.folder.path)) toggleFolder(cook, row.folder.path);
    else go(at + 1);
  } else if (key === "arrowleft" && row) {
    if (row.folder && cook.expanded.has(row.folder.path)) toggleFolder(cook, row.folder.path);
    else {
      const parent = rows.slice(0, at).reverse().find((item) => item.depth === row.depth - 1 && item.folder);
      if (parent) selectRow(cook, parent.key, false, false);
    }
  } else if (key === " ") {
    const paths = picked();
    setIncluded(id, cook, paths, paths.every((path) => cook.excluded.has(path)));
    main.querySelector<HTMLElement>("#asset-tree")?.focus();
  } else if (key === "delete") {
    setIncluded(id, cook, picked(), false);
    main.querySelector<HTMLElement>("#asset-tree")?.focus();
  } else if (key === "insert") {
    setIncluded(id, cook, picked(), true);
    main.querySelector<HTMLElement>("#asset-tree")?.focus();
  } else if (key === "p" && !ctrl && !cook.share) {
    const paths = picked();
    setPinned(id, cook, paths, !paths.every((path) => cook.pinned.has(path)));
    main.querySelector<HTMLElement>("#asset-tree")?.focus();
  } else if (key === "enter" && row?.folder) toggleFolder(cook, row.folder.path);
  else if (key === "escape") {
    cook.selected.clear();
    paintSelection(cook);
  } else handled = false;
  if (handled) event.preventDefault();
}

function useReport(cook: CookView, report: AssetReport): void {
  cook.report = report;
  cook.root = buildTree(report.assets);
  cook.excluded = new Set(report.excluded);
  cook.pinned = new Set(report.pinned);
  // Rows can disappear when files do, and nothing should stay selected that isn't there.
  const rows = new Set(visibleRows(cook).map((row) => row.key));
  cook.selected = new Set([...cook.selected].filter((key) => rows.has(key)));
  if (!rows.has(cook.cursor)) cook.cursor = "";
}

async function loadCook(id: number): Promise<void> {
  const cook = cookFor(id);
  if (cook.report || cook.loading) return;
  cook.loading = true;
  try {
    useReport(cook, await invoke<AssetReport>("mod_assets", { modId: id }));
    cook.error = "";
  } catch (error) {
    cook.error = String(error);
  }
  cook.loading = false;
  if (selectedEntry()?.id === id) render();
  void loadBuilt(id);
}

/** Read the mod's assets again, keeping what's open and selected in the tree. */
async function reloadCook(id: number): Promise<void> {
  const cook = cookViews.get(id);
  if (!cook?.report) return;
  try {
    await flushSelection(id, cook);
    const report = await invoke<AssetReport>("mod_assets", { modId: id });
    // A click while this was reading is newer than what's on disk.
    if (selectionTimers.has(id)) return;
    useReport(cook, report);
  } catch (error) {
    cook.note = { ok: false, text: String(error) };
  }
  if (state.selectedId === id) render();
}

async function loadBuilt(id: number): Promise<void> {
  const cook = cookFor(id);
  try {
    cook.built = new Set(await invoke<string[]>("built_assets", { modId: id, target: "windows" }));
  } catch {
    cook.built = null;
  }
  if (state.selectedId === id && activeTabIs("cook")) render();
}

async function readReferences(id: number, cook: CookView): Promise<boolean> {
  cook.analysing = true;
  cook.note = null;
  render();
  const excluded = [...cook.excluded];
  const pinned = [...cook.pinned];
  try {
    useReport(cook, await invoke<AssetReport>("find_used_assets", { modId: id }));
    cook.excluded = new Set(excluded);
    cook.pinned = new Set(pinned);
    cook.analysing = false;
    return true;
  } catch (error) {
    cook.note = { ok: false, text: String(error) };
    cook.analysing = false;
    return false;
  }
}

async function filterOnlyUsed(id: number, cook: CookView): Promise<void> {
  cook.note = null;
  if (!cook.report?.analysed && !(await readReferences(id, cook))) return render();
  const report = cook.report!;
  if (!report.rooted && !report.consumers.length) return render();
  const kept = pinnedClosure(cook);
  const unused = report.assets.filter((asset) => asset.status === "unused" && !kept.has(asset.path));
  const saved = unused.reduce((total, asset) => total + asset.size, 0);
  changeExcluded(id, cook, (excluded) => unused.forEach((asset) => excluded.add(asset.path)));
  const strings = report.assets.filter((asset) => asset.status === "in_string").length;
  const others = report.assets.filter((asset) => asset.status === "other_mods").length;
  const parts = [strings ? `${count(strings)} that only a string points at` : "", others ? `${count(others)} that other mods need` : ""].filter(Boolean);
  const keptText = parts.length ? ` Kept ${parts.join(" and ")}.` : "";
  cook.note = {
    ok: true,
    text: unused.length
      ? `Left out ${count(unused.length)} unused ${unused.length === 1 ? "asset" : "assets"}, ${megabytes(saved)} before cooking.${keptText} Check them before you publish, because a name built while the game runs can't be seen.`
      : `Everything in this mod is used.${keptText}`,
  };
  render();
}

/** Check a partial build before spending time on it. Returns false to stop. */
async function confirmSelection(entry: Entry): Promise<boolean> {
  const cook = cookViews.get(entry.id);
  if (!cook?.excluded.size) return true;
  window.clearTimeout(selectionTimers.get(entry.id));
  selectionTimers.delete(entry.id);
  await invoke("store_selection", { modId: entry.id, selection: selectionOf(cook, false) });
  const missing = await invoke<MissingReference[]>("missing_references", { modId: entry.id });
  if (!missing.length) return true;
  const first = missing[0];
  const who = first.kept.startsWith("UGC") && first.kept.includes(":") ? outsideName(first.kept) : first.kept;
  const yes = await ask(`${missing.length} kept assets point at assets you left out, such as ${who} using ${first.missing}. Players could see missing content.`, {
    title: "Build anyway?", kind: "warning",
  });
  if (!yes) {
    selectTab("cook");
    return false;
  }
  await invoke("store_selection", { modId: entry.id, selection: selectionOf(cook, true) });
  return true;
}

/* Source: exactly what other modders get */

function shortName(path: string): string {
  return path.split("/").pop()!;
}

/** What's on the list, without the descriptor, which always goes. */
function sharedFiles(view: CookView): string[] {
  const descriptor = view.share!.descriptor;
  return (view.report?.assets ?? []).filter((asset) => asset.path !== descriptor && !view.excluded.has(asset.path)).map((asset) => asset.path);
}

function chosenCount(id: number): number {
  const view = shareViews.get(id);
  return view?.report ? sharedFiles(view).length : 0;
}

function shareStatus(view: CookView, asset: Asset): AssetStatus | ShareStatus {
  const share = view.share!;
  if (asset.path === share.descriptor) return "required";
  if (share.needed.has(asset.path)) return "needed";
  return view.excluded.has(asset.path) ? "unknown" : share.changes.get(asset.path) ?? "unknown";
}

function shareSummary(view: CookView): string {
  const assets = view.report?.assets ?? [];
  const chosen = new Set(sharedFiles(view));
  const size = assets.filter((asset) => chosen.has(asset.path)).reduce((total, asset) => total + asset.size, 0);
  const ticked = `${count(chosen.size)} of ${count(assets.length - 1)} files ticked`;
  return chosen.size ? `${ticked}, ${megabytes(size)}` : ticked;
}

function shareDetailHtml(view: CookView, rows: TreeRow[]): string {
  const row = rows.find((item) => item.key === view.cursor);
  if (!row) return `<p class="hint">Select a file to see what it needs.</p>`;
  if (row.folder) {
    const chosen = row.folder.all.filter((path) => !view.excluded.has(path)).length;
    return `<p class="font-medium break-words">${escape(row.folder.path)}</p>
      <p class="hint">${count(row.folder.all.length)} files, ${megabytes(row.folder.size)}, ${count(chosen)} ticked.</p>`;
  }
  const share = view.share!;
  const path = row.asset!.path;
  const links = share.check?.missing ?? [];
  const needs = links.filter((link) => link.from === path).map((link) => link.needs);
  const neededBy = links.filter((link) => link.needs === path).map((link) => link.from);
  const list = (title: string, paths: string[]) => paths.length ? `
    <p class="mt-2 text-[13px] font-medium">${title}</p>
    <ul class="hint">${paths.map((item) => `<li class="truncate" title="${escape(item)}">${escape(item)}</li>`).join("")}</ul>` : "";
  const hint = path === share.descriptor ? "Always shared, because a download can't be installed without it."
    : view.excluded.has(path) ? "This stays on your PC." : "This goes to whoever downloads the source.";
  return `
    <p class="font-medium break-words">${escape(path)}</p>
    <p class="hint">${hint}</p>
    ${list("Needs these, which aren't shared", needs)}
    ${list("Needed by", neededBy)}`;
}

function shareNoticesHtml(view: CookView): string {
  const share = view.share!;
  const check = share.check;
  const parts: string[] = [];
  if (share.gone.length) {
    const one = share.gone.length === 1;
    parts.push(`<div class="notice mb-3">${count(share.gone.length)} ticked ${one ? "file no longer exists" : "files no longer exist"}, such as ${escape(share.gone[0])}, so ${one ? "it" : "they"} won't be shared.</div>`);
  }
  if (check?.add.length) {
    const one = check.add.length === 1;
    const first = check.missing[0];
    const lead = one && first
      ? `${escape(shortName(first.from))} needs ${escape(shortName(first.needs))}, which isn't shared`
      : `${count(check.add.length)} files these need aren't shared${first ? `, such as ${escape(shortName(first.needs))} for ${escape(shortName(first.from))}` : ""}`;
    parts.push(`
      <div class="notice notice-failed mb-3 items-center justify-between gap-3">
        <span>${lead}. Without ${one ? "it" : "them"}, other modders see missing references.</span>
        <span class="flex shrink-0 gap-2">
          <button class="btn btn-ghost btn-sm" data-action="share-show-needed">Show</button>
          <button class="btn btn-secondary btn-sm" data-action="share-add-needed">Add ${one ? "it" : "them"}</button>
        </span>
      </div>`);
  }
  if (check?.plugins.length) {
    const one = check.plugins.length === 1;
    const names = check.plugins.map((need) => escape(modNameOf(Number(need.plugin.replace("UGC", ""))))).join(", ");
    parts.push(`<div class="notice mb-3">These also point at ${one ? "another mod" : "other mods"}: ${names}. Whoever downloads them needs ${one ? "it" : "those"} too.</div>`);
  }
  return parts.join("");
}

/** Where this mod's source stands on mod.io, and what's changed here since. */
function shareStateHtml(entry: Entry, view: CookView): string {
  const share = view.share!;
  const live = entry.info?.platforms.find((platform) => platform.platform === "source");
  const files = dataFor(entry.id).files.value ?? [];
  const version = live && files.find((file) => file.id === live.file_id)?.version;
  const older = files.filter((file) => file.platforms.includes("source") && file.id !== live?.file_id);
  const where = live ? `Shared on mod.io${version ? ` as version ${escape(version)}` : ""}.`
    : entry.info ? "Not shared on mod.io yet." : "";
  const changes = [...share.changes.entries()].filter(([path]) => !view.excluded.has(path));
  const changed = changes.filter(([, change]) => change === "changed").length;
  const added = changes.length - changed;
  const parts = [
    changed ? `${count(changed)} changed` : "", added ? `${count(added)} new` : "",
    share.removed.length ? `${count(share.removed.length)} removed` : "",
  ].filter(Boolean);
  return `
    <p class="mb-3 text-ink-muted">${where}${parts.length ? ` Since your last share: ${parts.join(", ")}.` : ""}</p>
    ${older.length ? `
      <div class="notice mb-3 items-center justify-between gap-3">
        <span>${count(older.length)} older ${older.length === 1 ? "share is" : "shares are"} still on mod.io, and whatever ${older.length === 1 ? "it holds" : "they hold"} can still be downloaded.</span>
        <button class="btn btn-secondary btn-sm shrink-0" data-action="share-older">Review</button>
      </div>` : ""}`;
}

function shareHtml(entry: Entry): string {
  const view = shareFor(entry.id);
  const share = view.share!;
  if (view.error && !view.report) return failedHtml(view.error);
  if (!view.report) return `<p class="flex items-center gap-2.5 py-2 text-ink-subtle"><span class="spinner"></span>Reading the mod's files</p>`;
  const rows = visibleRows(view);
  const picked = selectedAssets(view, rows).length;
  const chosen = sharedFiles(view).length;
  const running = isRunning();
  const updating = state.update.status === "installing";
  const blocked = !chosen || running || state.checking || !state.hasToken || updating;
  const label = updating ? "Updating the app" : running && state.run?.action === "share" ? "Sharing" : state.checking ? "Checking" : "Share on mod.io";
  const views: [AssetView, string][] = [["all", "All files"], ["shared", "Ticked"], ["left-out", "Not ticked"], ["needed", "Needed"], ["changed", "Changed"]];
  return `
    <div id="share-state">${shareStateHtml(entry, view)}</div>
    <div class="mb-3 flex flex-wrap items-center gap-2">
      <button class="btn btn-secondary btn-sm" data-action="share-clear" ${chosen ? "" : "disabled"}>Clear</button>
      <span class="flex">
      <button class="btn btn-ghost btn-sm btn-icon" data-action="cook-undo" aria-label="Undo" title="Undo (Ctrl+Z)" ${view.undo.length ? "" : "disabled"}>${icon("undo")}</button>
      <button class="btn btn-ghost btn-sm btn-icon" data-action="cook-redo" aria-label="Redo" title="Redo (Ctrl+Y)" ${view.redo.length ? "" : "disabled"}><span class="inline-block -scale-x-100">${icon("undo")}</span></button>
      <button class="btn btn-ghost btn-sm btn-icon" data-action="expand-all" aria-label="Expand all" title="Expand all">${icon("expand")}</button>
      <button class="btn btn-ghost btn-sm btn-icon" data-action="collapse-all" aria-label="Collapse all" title="Collapse all">${icon("collapse")}</button>
      </span>
      ${share.checking ? `<span class="flex items-center gap-2 text-[13px] text-ink-subtle"><span class="spinner size-3.5"></span>Checking what these need</span>` : ""}
      <span class="flex-1"></span>
      <input id="asset-search" class="input h-8 w-28 lg:w-36" autocomplete="off" value="${escape(view.query)}" placeholder="Search files" aria-label="Search files" />
      <select id="asset-view" class="input h-8 w-28 lg:w-36" aria-label="Show">
        ${views.map(([id, name]) => `<option value="${id}" ${view.view === id ? "selected" : ""}>${name}</option>`).join("")}
      </select>
    </div>
    <div id="share-notices">
      ${view.note ? `<div class="mb-3">${noteHtml(view.note)}</div>` : ""}
      ${shareNoticesHtml(view)}
    </div>
    <div class="card overflow-hidden">
      <div class="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-line px-3 py-2 text-[13px] text-ink-muted">
        <span class="flex-1 whitespace-nowrap" id="cook-summary">${shareSummary(view)}</span>
        <span id="cook-picked" class="${picked ? "" : "hidden"} flex flex-wrap items-center gap-2">
          <span class="tabular-nums">${count(picked)} selected</span>
          <button class="btn btn-secondary btn-sm" data-action="include-picked">Tick</button>
          <button class="btn btn-secondary btn-sm" data-action="exclude-picked">Untick</button>
        </span>
      </div>
      <div id="asset-scroll" class="h-[max(16rem,calc(100vh-42rem))] overflow-y-auto">
        <div class="asset-head"><span class="flex-1 pl-14">Name</span><span class="asset-status">Status</span><span class="asset-size">Size</span></div>
        <div id="asset-tree" class="outline-none" role="tree" aria-label="Files" aria-multiselectable="true" tabindex="0">${treeHtml(view)}</div>
      </div>
      <div id="asset-detail" class="max-h-32 overflow-y-auto border-t border-line px-3 py-2 text-[13px] select-text">${shareDetailHtml(view, rows)}</div>
    </div>
    <p class="hint mt-2">Only what's ticked leaves your PC, and a new file is never ticked by itself. Ctrl or Shift click to select, Ctrl+A for all, Space to switch.</p>
    <label class="field mt-5"><span class="label">Change note</span>
      <input id="share-note" class="input" autocomplete="off" spellcheck="true" value="${escape(share.changeNote)}" placeholder="What changed in this share" /></label>
    <div id="share-account">${state.hasToken ? "" : `
      <div class="notice mt-4 items-center justify-between">
        <span>Connect your mod.io account to share.</span>
        <button class="btn btn-secondary btn-sm" data-action="settings">Open Settings</button>
      </div>`}</div>
    <div class="sticky bottom-0 z-10 -mx-1 mt-4 flex flex-wrap gap-2 border-t border-line bg-canvas px-1 py-3">
      <button class="btn btn-primary" data-action="share-now" ${blocked ? "disabled" : ""}>${label}</button>
      <button class="btn btn-secondary" data-action="share-export" title="Save exactly what would be shared, to look through first" ${chosen && !running ? "" : "disabled"}>Save as zip</button>
    </div>`;
}

function useShare(view: CookView, report: ShareReport): void {
  const share = view.share!;
  const assets: Asset[] = report.items.map(({ path, size }) => ({ path, size, status: "unknown", used_by: [], uses: [], outside: [] }));
  view.report = { assets, analysed: false, rooted: false, excluded: [], confirmed: false, pinned: [], consumers: [] };
  view.root = buildTree(assets);
  const chosen = new Set(report.files);
  view.excluded = new Set(assets.filter((asset) => asset.path !== report.descriptor && !chosen.has(asset.path)).map((asset) => asset.path));
  view.pinned = new Set([report.descriptor]);
  Object.assign(share, {
    descriptor: report.descriptor, gone: report.gone, removed: report.removed,
    changes: new Map(report.items.filter((item) => item.change).map((item) => [item.path, item.change])),
  });
  const rows = new Set(visibleRows(view).map((row) => row.key));
  view.selected = new Set([...view.selected].filter((key) => rows.has(key)));
  if (!rows.has(view.cursor)) view.cursor = "";
}

async function loadShare(id: number): Promise<void> {
  const view = shareFor(id);
  if (view.report || view.loading) return;
  view.loading = true;
  try {
    useShare(view, await invoke<ShareReport>("share_files", { modId: id }));
    view.error = "";
  } catch (error) {
    view.error = String(error);
  }
  view.loading = false;
  if (selectedEntry()?.id === id) render();
  if (view.report) void checkShare(id, view);
}

/** Read the files again, keeping what's open and selected in the tree. */
async function reloadShare(id: number): Promise<void> {
  const view = shareViews.get(id);
  if (!view?.report) return;
  try {
    await flushShare(id, view);
    const report = await invoke<ShareReport>("share_files", { modId: id });
    // A click while this was reading is newer than what's on disk.
    if (shareTimers.has(id)) return;
    useShare(view, report);
    void checkShare(id, view);
  } catch (error) {
    view.note = { ok: false, text: String(error) };
  }
  if (state.selectedId === id) render();
}

/** A box can be ticked many times a second, and big mods take a moment to read. */
async function checkShare(id: number, view: CookView): Promise<void> {
  const share = view.share!;
  const ticket = ++share.ticket;
  const files = sharedFiles(view);
  const show = () => {
    if (selectedEntry()?.id === id && activeTabIs("source")) render();
  };
  if (!files.length) {
    share.check = null;
    share.needed = new Set();
    share.checking = false;
    return show();
  }
  share.checking = true;
  show();
  try {
    const check = await invoke<ShareCheck>("check_shared", { modId: id, files });
    if (ticket !== share.ticket) return;
    share.check = check;
    share.needed = new Set(check.add);
  } catch (error) {
    if (ticket !== share.ticket) return;
    view.note = { ok: false, text: String(error) };
  }
  share.checking = false;
  show();
}

const shareTimers = new Map<number, number>();

function storeShare(id: number, view: CookView): Promise<void> {
  return invoke("store_share", { modId: id, files: sharedFiles(view) });
}

function saveShare(id: number, view: CookView): void {
  window.clearTimeout(shareTimers.get(id));
  shareTimers.set(id, window.setTimeout(async () => {
    shareTimers.delete(id);
    try {
      await storeShare(id, view);
      void checkShare(id, view);
    } catch (error) {
      view.note = { ok: false, text: String(error) };
      render();
    }
  }, 200));
}

/** Save a waiting change now, before something reads the list from disk. */
async function flushShare(id: number, view: CookView): Promise<void> {
  if (!shareTimers.has(id)) return;
  window.clearTimeout(shareTimers.get(id));
  shareTimers.delete(id);
  await storeShare(id, view);
}

/** Say exactly what's about to go public. Returns false to stop. */
async function confirmShare(entry: Entry, view: CookView): Promise<boolean> {
  await flushShare(entry.id, view);
  const files = sharedFiles(view);
  const check = await invoke<ShareCheck>("check_shared", { modId: entry.id, files });
  view.share!.check = check;
  view.share!.needed = new Set(check.add);
  const chosen = new Set(files);
  const size = view.report!.assets.filter((asset) => chosen.has(asset.path)).reduce((total, asset) => total + asset.size, 0);
  const warning = check.add.length
    ? ` ${count(check.add.length)} ${check.add.length === 1 ? "file these need isn't" : "files these need aren't"} included, so other modders will see missing references.` : "";
  return ask(`${count(files.length)} ${files.length === 1 ? "file" : "files"}, ${megabytes(size)}, plus the plugin's .uplugin. Anyone can download these from mod.io, and everything else stays on your PC.${warning}`, {
    title: "Share with other modders?", kind: check.add.length ? "warning" : "info", okLabel: "Share", cancelLabel: "Cancel",
  });
}

async function shareNow(): Promise<void> {
  const entry = selectedEntry();
  if (!entry || isRunning() || state.checking) return;
  const view = shareFor(entry.id);
  state.checking = true;
  view.note = null;
  render();
  let ready = false;
  try {
    ready = await confirmUnrealWork() && await confirmShare(entry, view);
  } catch (error) {
    view.note = { ok: false, text: String(error) };
  }
  state.checking = false;
  if (ready && selectedEntry()?.id !== entry.id) {
    ready = false;
    view.note = { ok: false, text: `Nothing was shared, because you left ${entry.name} during the check.` };
  }
  render();
  if (!ready) return;
  const run = beginRun("share", "package", "source", ["source"]);
  try {
    await invoke("start_share", { modId: run.modId, changelog: view.share!.changeNote });
  } catch (error) {
    endRun(run, false, String(error));
    render();
  }
}

async function exportShare(): Promise<void> {
  const entry = selectedEntry();
  if (!entry) return;
  const view = shareFor(entry.id);
  try {
    await flushShare(entry.id, view);
    const path = await save({ defaultPath: `UGC${entry.id}-source.zip`, filters: [{ name: "Zip", extensions: ["zip"] }] });
    if (!path) return;
    const made = await invoke<{ files: number; bytes: number }>("export_share", { modId: entry.id, name: entry.name, path });
    view.note = { ok: true, text: `Saved ${count(made.files)} files, ${megabytes(made.bytes)}. This is exactly what would be shared.` };
  } catch (error) {
    view.note = { ok: false, text: String(error) };
  }
  render();
}

function shareOptionHtml(entry: Entry): string {
  const chosen = chosenCount(entry.id);
  const hint = chosen
    ? `Also shares the ${count(chosen)} ${chosen === 1 ? "file" : "files"} ticked in the Source tab.`
    : "Tick what to share in the Source tab first.";
  return optionHtml("share-source", "Share source with other modders", hint, state.shareSource && chosen > 0, !chosen);
}

function optionHtml(id: string, label: string, hint: string, checked: boolean, disabled = false): string {
  return `
    <label class="flex items-start gap-2.5 ${disabled ? "opacity-50" : "cursor-pointer"}">
      <input type="checkbox" id="${id}" class="mt-0.5" ${checked ? "checked" : ""} ${disabled ? "disabled" : ""} />
      <span><span class="block font-medium">${label}</span><span class="hint block">${hint}</span></span>
    </label>`;
}

function publishHtml(entry: Entry): string {
  const group = (id: Group, label: string) =>
    `<button data-group="${id}" aria-pressed="${state.group === id}">${label}</button>`;
  const running = isRunning();
  const updating = state.update.status === "installing";
  const blocked = running || state.checking || !state.hasToken || updating;
  const label = updating ? "Updating the app"
    : state.checking ? "Checking references"
    : running && state.run?.action === "publish" ? "Publishing"
    : state.pending ? `Resume version ${state.pending}` : "Publish";
  return `
    <div class="grid gap-8 lg:grid-cols-[minmax(0,1fr)_288px]">
      <div class="min-w-0 space-y-6">
        ${state.hasToken ? "" : `
          <div class="notice items-center justify-between">
            <span>Connect your mod.io account to publish.</span>
            <button class="btn btn-secondary btn-sm" data-action="settings">Open Settings</button>
          </div>`}
        ${state.pending && !running ? `
          <div class="notice items-center justify-between">
            <span>Version ${state.pending} is already cooked. Resume to finish uploading it, or start over.</span>
            <button class="btn btn-secondary btn-sm" data-action="discard">Start over</button>
          </div>` : ""}
        ${leftOutNotice(entry)}
        <div>
          <span class="label">Platforms</span>
          <div class="segmented" role="group" aria-label="Platforms">
            ${group("pc", "PC")}${group("quest", "Quest")}${group("both", "Both")}
          </div>
          <p class="hint mt-1.5">${GROUP_HINTS[state.group]}</p>
        </div>
        <label class="field"><span class="label">Change note</span>
          <input id="changelog" class="input" autocomplete="off" spellcheck="true" value="${escape(state.changelog)}" placeholder="What changed in this version" /></label>
        <div class="space-y-3">
          ${optionHtml("inactive", "Keep hidden after upload", "Make the files live later from the Files tab.", state.inactive)}
          ${optionHtml("together", "Go live on all platforms together", "Waits until every build has uploaded.", state.together && !state.inactive, state.inactive)}
          ${optionHtml("uncompressed", "Skip compression", "Packages faster, but the upload is larger.", !state.compress)}
          ${entry.kind === "Mod" ? shareOptionHtml(entry) : ""}
          ${optionHtml("verbose", "Show verbose log", "Opens the log with all output from Unreal, such as UnrealPak.", state.verbose)}
        </div>
        <div class="sticky bottom-0 z-10 -mx-1 flex flex-wrap gap-2 border-t border-line bg-canvas px-1 py-3">
          <button class="btn btn-primary" data-action="publish" ${blocked ? "disabled" : ""}>${label}</button>
          <button class="btn btn-secondary" data-action="stage" ${running || state.checking || updating ? "disabled" : ""}>Stage for local testing</button>
        </div>
      </div>
      <div class="min-w-0">${state.hasToken ? liveHtml(entry) : ""}</div>
    </div>`;
}

/* Settings */

function tokenHtml(): string {
  const ready = state.tokenDraft.trim() && !state.tokenBusy;
  return `
        <div class="space-y-4 border-t border-line pt-4">
          <ol class="list-decimal space-y-1.5 pl-5 text-ink-muted">
            <li>Open the Personal access tokens tab on mod.io and log in. Agree to the API Access Terms if it asks.</li>
            <li>Select Generate token and give it any name.</li>
            <li>Under Permissions, tick All my mods and User actions.</li>
            <li>Under Scope, tick Read and Write, then set Expiry to 1 Year.</li>
            <li>Select Generate and copy the token. mod.io only shows it once.</li>
            <li>Paste it here and select Connect.</li>
          </ol>
          <button class="btn btn-secondary" data-action="token-page">Open mod.io</button>
          <label class="field"><span class="label">Personal token</span>
            <div class="flex gap-2">
              <input id="token" class="input" autocomplete="off" type="password" autocomplete="off" value="${escape(state.tokenDraft)}" placeholder="Paste your token" />
              <button class="btn btn-primary" data-action="save-token" ${ready ? "" : "disabled"}>${state.tokenBusy ? "Checking" : "Connect"}</button>
            </div></label>
          <p class="hint">You choose how long a token lasts, and the app tells you when yours runs out. It's encrypted for your Windows account and never leaves your PC, except to reach mod.io. By connecting you agree to the <button class="cursor-pointer text-ink underline" data-action="terms">mod.io terms</button>.</p>
        </div>`;
}

function usageHtml(): string {
  const usage = state.usage;
  if (!usage) return "";
  const share = Math.min((usage.requests / usage.request_limit) * 100, 100);
  return `
        <div class="border-t border-line pt-4">
          <div class="mb-1.5 flex items-baseline justify-between gap-3 text-[13px]">
            <span class="font-medium">mod.io requests this minute</span>
            <span id="usage-count" class="text-ink-muted tabular-nums">${count(usage.requests)} of ${count(usage.request_limit)}</span>
          </div>
          <span id="usage-meter" class="meter meter-plain" role="progressbar" aria-label="mod.io requests this minute" aria-valuemin="0" aria-valuemax="${usage.request_limit}" aria-valuenow="${usage.requests}"><i style="width:${share}%"></i></span>
          <p class="hint mt-1.5">The app slows itself down before it reaches mod.io's limit.</p>
        </div>`;
}

function usernameHtml(username: string): string {
  return `
        <div class="border-t border-line pt-4">
          <label class="field"><span class="label">mod.io username</span>
            <div class="flex gap-2">
              <input id="username" class="input" autocomplete="off" value="${escape(username)}" />
              <button class="btn btn-secondary" data-action="save-username">Save</button>
            </div>
            <span class="hint mt-1 block">The app uses this to hide mods you can't manage.</span></label>
        </div>`;
}

function updateStatusHtml(): string {
  const { status, version, percent, error } = state.update;
  const busy = isRunning() || state.checking;
  if (status === "installing") {
    return `
      <div>
        <p class="mb-1.5 text-[13px]">${percent === null ? `Downloading version ${escape(version)}` : `Downloading version ${escape(version)}, ${percent}%`}</p>
        <span class="meter" role="progressbar" aria-label="Update download" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent ?? 0}"><i style="width:${percent ?? 0}%"></i></span>
      </div>`;
  }
  if (status === "available") {
    return `
      <div class="flex flex-wrap items-center gap-2">
        <button class="btn btn-primary" data-action="install-update" ${busy ? "disabled" : ""}>Install version ${escape(version)}</button>
        <button class="btn btn-ghost" data-action="whats-new">What's new${icon("external", 14)}</button>
      </div>
      <p class="hint">${busy ? "This can install once the current run has finished." : "The app closes, updates and reopens."}</p>`;
  }
  const result = status === "current" ? "You have the latest version."
    : status === "failed" ? escape(error) : "";
  return `
    <div class="flex flex-wrap items-center gap-3">
      <button class="btn btn-secondary" data-action="check-update" ${status === "checking" ? "disabled" : ""}>${status === "checking" ? `<span class="spinner size-3.5"></span>Checking` : "Check for updates"}</button>
      ${result ? `<span class="text-[13px] ${status === "failed" ? "text-danger" : "text-ink-muted"}">${result}</span>` : ""}
    </div>`;
}

function updatesHtml(): string {
  return `
    <section class="card mt-5 space-y-4 p-5">
      <div>
        <h2 class="font-semibold">Updates</h2>
        ${state.version ? `<p class="mt-0.5 text-[13px] text-ink-muted">Pavlov Mod Studio ${escape(state.version)}</p>` : ""}
      </div>
      ${optionHtml("auto-update", "Update automatically", "Checks when the app starts, then installs any new version and reopens. It never interrupts a publish.", state.autoUpdate)}
      ${updateStatusHtml()}
    </section>`;
}

/* Pavlov's EULA */

/** Nothing else is on offer until a modkit is chosen and its EULA is accepted. */
function eulaLocked(): boolean {
  return !state.eula?.accepted;
}

async function readEula(): Promise<void> {
  state.eula = null;
  state.eulaNote = null;
  try {
    state.eula = await invoke<Eula | null>("read_eula");
  } catch (error) {
    state.eulaNote = { ok: false, text: String(error) };
  }
}

/** Everything that waits for the EULA. */
async function unlock(): Promise<void> {
  state.hasToken = await invoke<boolean>("has_token");
  restoreLibrary();
  await loadMods();
}

async function acceptEula(): Promise<void> {
  try {
    await invoke("accept_eula");
    state.eula = { ...state.eula!, accepted: true };
    state.note = null;
    await unlock();
    void loadLibrary();
  } catch (error) {
    state.note = { ok: false, text: String(error) };
  }
  render();
}

function eulaHtml(): string {
  return `
    <div class="mx-auto flex h-full max-w-3xl flex-col px-8 py-7">
      ${pageHeader("Pavlov's End User License Agreement", "This is Vankrupt's agreement for the modkit. Accept it to use the modkit through this app.")}
      ${noteHtml(state.note)}
      <div id="eula-text" class="card min-h-0 flex-1 overflow-y-auto p-4 text-[13px] whitespace-pre-wrap select-text" tabindex="0">${escape(state.eula!.text)}</div>
      <div class="mt-4 flex flex-wrap gap-2">
        <button class="btn btn-primary" data-action="accept-eula">I accept</button>
        <button class="btn btn-secondary" data-action="decline-eula">Close the app</button>
      </div>
    </div>`;
}

function settingsHtml(): string {
  const { project, engine, stage, username } = state.settingsDraft;
  const folder = (id: string, label: string, value: string, action: string) => `
    <label class="field"><span class="label">${label}</span>
      <div class="flex gap-2">
        <input id="${id}" class="input" autocomplete="off" value="${escape(value)}" />
        ${action ? `<button class="btn btn-secondary" data-action="${action}">Browse</button>` : ""}
      </div></label>`;
  const account = state.hasToken
    ? `Connected${state.access.user ? ` as ${escape(state.access.user)}` : ""}.`
    : "Connect your account to publish and manage your mods.";
  return `
    <div class="mx-auto max-w-2xl px-8 py-7">
      ${pageHeader("Settings", state.settings.project ? "" : "Choose your modkit folder to get started.")}
      <section class="card mb-5 space-y-4 p-5">
        <h2 class="font-semibold">Folders</h2>
        ${folder("project", "Modkit folder", project, "browse")}
        ${folder("engine", "Unreal Engine folder", engine, "browse-engine")}
        ${folder("stage", "Staging folder, for local testing", stage, "browse-stage")}
        <button class="btn btn-primary" data-action="save-settings">Save settings</button>
        ${noteHtml(state.eulaNote ?? state.settingsNote)}
      </section>
      ${eulaLocked() ? "" : `<section class="card space-y-4 p-5">
        <div class="flex items-center justify-between gap-3">
          <div>
            <h2 class="font-semibold">mod.io account</h2>
            <p class="mt-0.5 text-[13px] text-ink-muted">${account}</p>
          </div>
          ${state.hasToken ? `<button class="btn btn-secondary btn-sm" data-action="remove-token">Disconnect</button>` : ""}
        </div>
        ${state.hasToken ? (state.access.user && !state.settings.username ? "" : usernameHtml(username)) + usageHtml() : tokenHtml()}
        ${noteHtml(state.accountNote)}
      </section>
      ${updatesHtml()}`}
    </div>`;
}

function createReady(): boolean {
  const { name, summary, logo } = state.draft;
  return Boolean(name.trim().length >= 3 && summary.trim() && logo && !state.creating && state.hasToken);
}

function createHtml(): string {
  const { name, summary, kind, logo } = state.draft;
  const type = (id: string, label: string, hint: string) => `
    <button class="card flex cursor-pointer flex-col justify-start p-3 text-left transition-colors hover:bg-panel aria-pressed:border-ink" data-kind="${id}" aria-pressed="${kind === id}">
      <span class="block font-medium">${label}</span><span class="hint block">${hint}</span>
    </button>`;
  return `
    <div class="mx-auto max-w-2xl px-8 py-7">
      ${pageHeader("New mod", "Creates the mod on mod.io and sets up its plugin here.")}
      <div class="space-y-5">
        ${state.hasToken ? "" : `
          <div class="notice items-center justify-between">
            <span>Connect your mod.io account to create a mod.</span>
            <button class="btn btn-secondary btn-sm" data-action="settings">Open Settings</button>
          </div>`}
        <div class="grid grid-cols-3 gap-2" role="group" aria-label="Mod type">
          ${type("Map", "Map", "Playable level")}
          ${type("Mod", "Mod", "Content for maps and game modes")}
          ${type("GameMode", "Game mode", "Custom game logic")}
        </div>
        <label class="field"><span class="label">Name, 3 to 32 characters</span>
          <input id="draft-name" class="input" autocomplete="off" maxlength="32" value="${escape(name)}" /></label>
        <label class="field"><span class="label">Summary</span>
          <textarea id="draft-summary" class="input" spellcheck="true" rows="3" maxlength="250">${escape(summary)}</textarea></label>
        ${logoField("Logo", logo, "pick-logo", "No image chosen")}
        <p class="hint">By creating a mod you agree to the <button class="cursor-pointer text-ink underline" data-action="terms">mod.io terms</button>.</p>
        <button class="btn btn-primary" data-action="create-mod" ${createReady() ? "" : "disabled"}>
          ${state.creating ? "Creating mod" : "Create mod"}
        </button>
        ${state.createError ? failedHtml(state.createError) : ""}
        ${state.failedSetup ? `<button class="btn btn-secondary" data-action="retry-setup">Finish setting up mod ${state.failedSetup.id}</button>` : ""}
      </div>
    </div>`;
}

function emptyHtml(): string {
  const configured = Boolean(state.settings.project);
  return `
    <div class="flex h-full items-center justify-center p-8">
      <div class="max-w-sm text-center">
        <h1 class="text-lg font-semibold tracking-tight">${configured ? "Select a mod" : "Choose your modkit folder"}</h1>
        <p class="mt-1.5 text-ink-muted">${configured
          ? allEntries().length ? "Your mods are listed on the left." : "This folder has no UGC plugins. Create a mod or choose another folder in Settings."
          : "Open Settings and choose the folder that holds Pavlov.uproject."}</p>
        ${configured ? "" : `<button class="btn btn-primary mt-4" data-action="settings">Open Settings</button>`}
      </div>
    </div>`;
}

/* Modal */

function modalHtml(current: Modal): string {
  const error = state.modalError ? failedHtml(state.modalError) : "";
  const actions = (label: string, action: string, ready: boolean, danger = false) => `
    <div class="flex justify-end gap-2 pt-1">
      <button class="btn btn-secondary" data-action="close-modal" ${locked()}>Cancel</button>
      <button class="btn ${danger ? "btn-danger" : "btn-primary"}" data-action="${action}" ${ready && !state.acting ? "" : "disabled"}>${label}</button>
    </div>`;
  if (current.kind === "edit-file") {
    return `
      <div class="space-y-4 p-5">
        <h2 id="modal-title" class="text-base font-semibold">Edit file</h2>
        <label class="field"><span class="label">Version</span>
          <input id="modal-version" class="input" autocomplete="off" value="${escape(current.version)}" />
          <span class="hint mt-1 block">This only changes the label. The build stays the same.</span></label>
        <label class="field"><span class="label">Changelog</span>
          <textarea id="modal-changelog" class="input" spellcheck="true" rows="4">${escape(current.changelog)}</textarea></label>
        ${error}
        ${actions("Save", "save-file", Boolean(current.version.trim()))}
      </div>`;
  }
  if (current.kind === "edit-comment") {
    return `
      <div class="space-y-4 p-5">
        <h2 id="modal-title" class="text-base font-semibold">Edit comment</h2>
        <textarea id="modal-comment" class="input" spellcheck="true" rows="5" aria-label="Comment">${escape(current.content)}</textarea>
        ${error}
        ${actions("Save", "save-comment", Boolean(current.content.trim()))}
      </div>`;
  }
  const name = selectedEntry()?.name ?? "";
  return `
    <div class="space-y-4 p-5">
      <h2 id="modal-title" class="text-base font-semibold">Close ${escape(name)}?</h2>
      <p class="text-ink-muted">Players will lose access to this mod. Only the mod.io website can restore it.</p>
      <label class="field"><span class="label">Type the mod's name to confirm</span>
        <input id="modal-confirm" class="input" autocomplete="off" value="${escape(current.typed)}" /></label>
      ${error}
      ${actions("Close mod", "confirm-close", current.typed.trim() === name.trim(), true)}
    </div>`;
}

function renderModal(): void {
  if (!state.modal) {
    if (modal.open) modal.close();
    modal.innerHTML = "";
    return;
  }
  morph(modal, modalHtml(state.modal));
  if (!modal.open) {
    modal.showModal();
    modal.querySelector<HTMLElement>("input, textarea")?.focus();
  }
}

function openModal(next: Modal): void {
  state.modal = next;
  state.modalError = "";
  renderModal();
}

function closeModal(): void {
  state.modal = null;
  state.modalError = "";
  renderModal();
}

/** Change only what differs, so images, focus, scroll, open lists and selected text survive. */
function morph(element: HTMLElement, html: string): void {
  morphdom(element, `<div>${html}</div>`, {
    childrenOnly: true,
    getNodeKey: (node) => {
      const item = node as HTMLElement;
      return item.dataset?.row ?? item.dataset?.mod ?? item.id ?? undefined;
    },
    // The log paints itself, and could hold thousands of lines.
    onBeforeElUpdated: (from) => from.id !== "run-log",
  });
}

/** A checkbox can only be half ticked from code. */
function syncMixed(): void {
  main.querySelectorAll<HTMLInputElement>("input[type=checkbox]").forEach((box) => {
    box.indeterminate = box.hasAttribute("data-mixed");
  });
}

function renderRail(): void {
  if (eulaLocked()) rail.replaceChildren();
  else morph(rail, railHtml());
}

let logTimer = 0;

function paintLog(): void {
  const log = runPanel.querySelector<HTMLElement>("#run-log");
  if (!log || !state.run) return;
  const pinned = !log.textContent || log.scrollHeight - log.scrollTop - log.clientHeight < 24;
  const { run } = state;
  const lines = run.verbose ? run.log : run.log.filter((line) => line.level === "step" || line.level === "error");
  log.innerHTML = lines.slice(-LOG_SHOWN).map((line) => {
    const tone = line.level === "error" ? "text-danger" : line.level === "warning" ? "text-ink" : line.level === "step" ? "text-ink" : "";
    return `<span class="${tone}">${escape(line.text)}</span>`;
  }).join("\n");
  if (pinned) log.scrollTop = log.scrollHeight;
  const counts = runPanel.querySelector("#run-counts");
  if (counts) {
    const parts = [
      run.errors ? `${count(run.errors)} ${run.errors === 1 ? "error" : "errors"}` : "",
      run.warnings ? `${count(run.warnings)} ${run.warnings === 1 ? "warning" : "warnings"}` : "",
    ];
    counts.textContent = parts.filter(Boolean).join(", ");
  }
}

function schedulePaint(run: Run): void {
  if (!run.showLog || logTimer) return;
  logTimer = window.setTimeout(() => {
    logTimer = 0;
    paintLog();
  }, 100);
}

function addLines(run: Run, lines: LogLine[]): void {
  run.log.push(...lines);
  if (run.log.length > LOG_LIMIT) run.log.splice(0, run.log.length - LOG_LIMIT);
  for (const line of lines) {
    if (line.level === "error") run.errors += 1;
    if (line.level === "warning") run.warnings += 1;
  }
  schedulePaint(run);
}

function renderRun(): void {
  const log = runPanel.querySelector<HTMLElement>("#run-log");
  const scrolled = log?.scrollTop ?? 0;
  morph(runPanel, state.run ? runHtml(state.run) : "");
  // A row appearing above the log can move it, and moving an element resets its scroll.
  if (log?.isConnected) log.scrollTop = scrolled;
  paintLog();
  const text = state.run ? headline(state.run) : "";
  if (announce.textContent !== text) announce.textContent = text;
}

let shownPage = "";

let timedNote: Note | null = null;
let noteTimer = 0;

/** Good news clears itself. A failure stays until it's dismissed. */
function timeNote(): void {
  if (state.note === timedNote) return;
  timedNote = state.note;
  window.clearTimeout(noteTimer);
  const note = state.note;
  if (!note?.ok) return;
  noteTimer = window.setTimeout(() => {
    if (state.note === note) {
      state.note = null;
      render();
    }
  }, NOTE_SECONDS * 1000);
}

function render(): void {
  const gated = eulaLocked();
  rail.classList.toggle("hidden", gated);
  app.classList.toggle("grid-cols-[248px_minmax(0,1fr)]", !gated);
  app.classList.toggle("grid-cols-1", gated);
  const entry = selectedEntry();
  const page = `${state.view} ${state.selectedId} ${entry ? activeTab(entry) : ""}`;
  const view =
    gated ? (state.eula ? eulaHtml() : settingsHtml())
    : state.view === "settings" ? settingsHtml()
    : state.view === "create" ? createHtml()
    : state.view === "source" ? sourceHtml()
    : entry ? modHtml(entry) : emptyHtml();
  morph(main, view + toastHtml());
  if (page !== shownPage) main.scrollTop = 0;
  shownPage = page;
  syncMixed();
  renderRail();
  renderRun();
  timeNote();
}

/* Loading */

function showSettings(): void {
  if (state.view !== "settings") {
    state.view = "settings";
    state.settingsDraft = { ...state.settings };
    state.settingsNote = null;
    state.accountNote = null;
  }
  render();
  void readUsage().then(render);
}

async function loadMods(): Promise<void> {
  if (!state.settings.project) {
    state.mods = [];
    return;
  }
  try {
    state.mods = await invoke<LocalMod[]>("list_mods", { project: state.settings.project });
  } catch {
    state.mods = [];
  }
}

function saveLibrary(): void {
  try {
    localStorage.setItem(SAVED_LIBRARY, JSON.stringify({
      infos: state.infos, known: state.access.known, owned: [...state.access.owned], user: state.access.user,
    }));
  } catch {
    // The list loads from mod.io next time instead.
  }
}

function restoreLibrary(): void {
  try {
    const saved = JSON.parse(localStorage.getItem(SAVED_LIBRARY) ?? "null");
    if (!saved) return;
    state.infos = saved.infos ?? {};
    state.access = { known: Boolean(saved.known), owned: new Set(saved.owned ?? []), user: saved.user ?? null, note: null };
  } catch {
    localStorage.removeItem(SAVED_LIBRARY);
  }
}

let libraryTicket = 0;

/** One batched read of every mod. This runs at launch and when asked for, not when switching. */
async function loadLibrary(): Promise<void> {
  const ticket = ++libraryTicket;
  if (!state.hasToken) {
    state.access = { known: false, owned: new Set(), user: null, note: null };
    state.loading = false;
    render();
    return;
  }
  state.loading = true;
  state.libraryError = "";
  renderRail();
  try {
    const library = await invoke<Library>("library", {
      ids: state.mods.map((m) => m.id), username: state.settings.username,
    });
    if (ticket !== libraryTicket) return;
    [...drafts.keys()].forEach(dropCleanDraft);
    state.infos = Object.fromEntries(library.mods.map((m) => [m.id, m]));
    state.access = { known: library.access_known, owned: new Set(library.owned), user: library.user, note: library.note };
    saveLibrary();
  } catch (error) {
    if (ticket !== libraryTicket) return;
    state.libraryError = String(error);
  }
  state.loading = false;
  render();
  void openTabData();
}

async function fetchSlot(id: number, key: SlotKey): Promise<void> {
  const data = dataFor(id);
  const slot = data[key] as Slot<unknown>;
  if (slot.busy || slot.value !== undefined) return;
  slot.busy = true;
  slot.error = "";
  try {
    if (key === "files") {
      const page = await invoke<FilePage>("mod_files", { modId: id, offset: 0, live: liveFiles(id) });
      data.files.value = page.files;
      data.filesTotal = page.total;
      data.filesNext = page.next;
    } else if (key === "comments") {
      const page = await invoke<CommentPage>("mod_comments", { modId: id, offset: 0 });
      data.comments.value = page.comments;
      data.commentsNext = page.next;
    } else {
      slot.value = await invoke(COMMANDS[key], { modId: id });
    }
  } catch (error) {
    slot.error = String(error);
  }
  slot.busy = false;
  if (cache.get(id) === data && state.selectedId === id && state.view === "mod") render();
}

function liveFiles(id: number): number[] {
  return state.infos[id]?.platforms.map((p) => p.file_id) ?? [];
}

async function moreFiles(id: number): Promise<void> {
  const data = dataFor(id);
  const offset = data.filesNext;
  if (offset === null || data.files.busy) return;
  data.files.busy = true;
  render();
  try {
    const page = await invoke<FilePage>("mod_files", { modId: id, offset, live: [] });
    const known = new Set(data.files.value?.map((file) => file.id));
    data.files.value = [...(data.files.value ?? []), ...page.files.filter((file) => !known.has(file.id))].sort((a, b) => b.id - a.id);
    data.filesTotal = page.total;
    data.filesNext = page.next;
  } catch (error) {
    state.note = { ok: false, text: String(error) };
  }
  data.files.busy = false;
  render();
}

async function moreComments(id: number): Promise<void> {
  const data = dataFor(id);
  const offset = data.commentsNext;
  if (offset === null || data.comments.busy) return;
  data.comments.busy = true;
  render();
  try {
    const page = await invoke<CommentPage>("mod_comments", { modId: id, offset });
    const known = new Set(data.comments.value?.map((comment) => comment.id));
    data.comments.value = [...(data.comments.value ?? []), ...page.comments.filter((comment) => !known.has(comment.id))];
    data.commentsNext = page.next;
  } catch (error) {
    state.note = { ok: false, text: String(error) };
  }
  data.comments.busy = false;
  render();
}

/** Refresh can't be pressed again straight away. */
function coolDown(which: "all" | "mod"): void {
  state.cooling[which] = true;
  window.setTimeout(() => {
    state.cooling[which] = false;
    render();
  }, REFRESH_SECONDS * 1000);
}

function waiting(): boolean {
  return state.waitUntil > Date.now();
}

function secondsLeft(): number {
  return Math.max(Math.ceil((state.waitUntil - Date.now()) / 1000), 1);
}

let waitClock = 0;

/** Show how long mod.io's limit is holding things up, then pick up what it stopped. */
function startWait(seconds: number): void {
  state.waitUntil = Math.max(state.waitUntil, Date.now() + seconds * 1000);
  window.clearInterval(waitClock);
  waitClock = window.setInterval(() => {
    if (waiting()) {
      const left = rail.querySelector("#wait-seconds");
      if (left) left.textContent = String(secondsLeft());
      return;
    }
    window.clearInterval(waitClock);
    state.waitUntil = 0;
    render();
    if (state.libraryError) void loadLibrary();
    else void openTabData();
  }, 1000);
  render();
}

async function readUsage(): Promise<void> {
  try {
    state.usage = await invoke<Usage | null>("api_usage");
  } catch {
    state.usage = null;
  }
}

/** The numbers are local, so reading them costs mod.io nothing. */
function paintUsage(): void {
  const usage = state.usage;
  const label = main.querySelector("#usage-count");
  const meter = main.querySelector<HTMLElement>("#usage-meter");
  if (!usage || !label || !meter) return;
  label.textContent = `${count(usage.requests)} of ${count(usage.request_limit)}`;
  meter.setAttribute("aria-valuenow", String(usage.requests));
  meter.querySelector("i")!.style.width = `${Math.min((usage.requests / usage.request_limit) * 100, 100)}%`;
}

async function fetchReport(id: number): Promise<void> {
  const target = state.reportTarget;
  const data = dataFor(id);
  const slot = (data.reports[target] ??= {});
  if (slot.busy || slot.value) return;
  slot.busy = true;
  slot.error = "";
  try {
    slot.value = await invoke<PakReport>("analyze_build", { modId: id, target });
  } catch (error) {
    slot.error = String(error).includes("missing cook output") ? NO_BUILD : String(error);
  }
  slot.busy = false;
  if (cache.get(id) === data && state.selectedId === id) render();
}

/** Load what the open tab shows, unless it's already here. */
async function openTabData(): Promise<void> {
  const entry = selectedEntry();
  if (!entry || state.view !== "mod") return;
  const tab = activeTab(entry);
  if (entry.local && (tab === "cook" || tab === "publish")) {
    void (cookViews.get(entry.id)?.report ? reloadCook(entry.id) : loadCook(entry.id));
  }
  if (entry.local && entry.kind === "Mod" && (tab === "source" || tab === "publish")) {
    void (shareViews.get(entry.id)?.report ? reloadShare(entry.id) : loadShare(entry.id));
  }
  if (tab === "size") return fetchReport(entry.id);
  if (!state.hasToken) return;
  await Promise.all((TAB_DATA[tab] ?? []).map((key) => fetchSlot(entry.id, key)));
}

function selectMod(id: number): void {
  if (state.selectedId !== id) {
    if (state.selectedId) changelogs.set(state.selectedId, state.changelog);
    state.changelog = changelogs.get(id) ?? "";
    state.selectedId = id;
    state.note = null;
    state.pickedFiles.clear();
    state.fileFilter = "all";
    state.filesShown = FILES_SHOWN;
    state.replyTo = null;
    state.replyDraft = "";
    state.commentDraft = "";
    state.linkDraft = "";
    state.dependencyDraft = "";
    state.metadataDraft = { key: "", value: "" };
  }
  state.view = "mod";
  render();
  void refreshPending().then(render);
  void openTabData();
}

/** Forget one mod's data and read it again. */
async function refreshMod(id: number): Promise<void> {
  state.refreshing = true;
  state.note = null;
  render();
  cache.delete(id);
  try {
    await reloadInfo(id);
  } catch (error) {
    state.note = { ok: false, text: String(error) };
  }
  state.refreshing = false;
  await refreshPending();
  render();
  await Promise.all([openTabData(), reloadCook(id)]);
}

let pendingTicket = 0;

/** A slow answer about the last mod must not land on this one. */
async function refreshPending(): Promise<void> {
  const ticket = ++pendingTicket;
  const entry = selectedEntry();
  let pending: number | null = null;
  try {
    if (entry?.local) {
      pending = await invoke<number | null>("pending_release", { modId: entry.id, groupName: state.group, compress: state.compress });
    }
  } catch {
    pending = null;
  }
  if (ticket === pendingTicket) state.pending = pending;
}

function selectTab(tab: Tab): void {
  state.tab = tab;
  state.note = null;
  render();
  void openTabData();
}

/* Changes on mod.io */

/** Run one change at a time and report how it went at the top of the tab. */
async function act(done: string, work: () => Promise<void>): Promise<boolean> {
  if (state.acting) return false;
  state.acting = true;
  state.note = null;
  render();
  let ok = true;
  try {
    await work();
    state.note = done ? { ok: true, text: done } : null;
  } catch (error) {
    ok = false;
    state.note = { ok: false, text: String(error) };
  }
  state.acting = false;
  render();
  return ok;
}

/** Re-read a list after changing it. */
async function reloadSlot(id: number, key: SlotKey): Promise<void> {
  dataFor(id)[key] = {};
  await fetchSlot(id, key);
}

async function reloadInfo(id: number): Promise<void> {
  const info = await invoke<ModInfo>("mod_info", { modId: id });
  dropCleanDraft(id);
  state.infos[id] = info;
  saveLibrary();
}

/** Show a change that already succeeded. Failing to re-read it doesn't undo it. */
async function showChange(id: number): Promise<void> {
  try {
    await reloadInfo(id);
  } catch {
    // Refresh shows it later.
  }
}

async function publishFile(fileId: number): Promise<void> {
  const id = state.selectedId!;
  const yes = await ask("Players will get this file on its platforms straight away.", {
    title: "Make this file live?", kind: "warning",
  });
  if (!yes) return;
  await act("That file is live.", async () => {
    await invoke("publish_file", { modId: id, fileId });
    state.pickedFiles.delete(fileId);
    await showChange(id);
    dataFor(id).events = {};
  });
  void openTabData();
}

function dropFiles(id: number, ids: number[]): void {
  const data = dataFor(id);
  const slot = data.files;
  if (slot.value) {
    const before = slot.value.length;
    slot.value = slot.value.filter((file) => !ids.includes(file.id));
    data.filesTotal -= before - slot.value.length;
  }
  ids.forEach((file) => state.pickedFiles.delete(file));
}

async function deleteFiles(ids: number[]): Promise<void> {
  const id = state.selectedId!;
  const title = ids.length === 1 ? "Delete this file?" : `Delete ${ids.length} files?`;
  const yes = await ask("This can't be undone.", { title, kind: "warning" });
  if (!yes) return;
  await act(ids.length === 1 ? "File deleted." : `Deleted ${ids.length} files.`, async () => {
    for (const [index, fileId] of ids.entries()) {
      try {
        await invoke("remove_file", { modId: id, fileId });
      } catch (error) {
        throw index ? `Deleted ${index} of ${ids.length} files. ${error}` : error;
      }
      dropFiles(id, [fileId]);
    }
  });
}

async function saveFile(): Promise<void> {
  const current = state.modal;
  if (current?.kind !== "edit-file" || state.acting) return;
  const id = state.selectedId!;
  const version = current.version.trim();
  const changelog = current.changelog.trim();
  state.acting = true;
  renderModal();
  try {
    await invoke("update_file", {
      modId: id, fileId: current.file.id,
      version: version === current.file.version ? null : version,
      changelog: changelog === current.file.changelog.trim() ? null : changelog,
    });
    Object.assign(current.file, { version, changelog });
    state.acting = false;
    closeModal();
    state.note = { ok: true, text: "File saved." };
  } catch (error) {
    state.acting = false;
    state.modalError = String(error);
    renderModal();
  }
  render();
}

async function savePage(entry: Entry): Promise<void> {
  const info = entry.info!;
  const draft = drafts.get(entry.id)!;
  const changes = pageChanges(info, draft);
  if (changes.visible === false) {
    const yes = await ask("Players won't be able to find or download this mod.", { title: "Hide this mod?", kind: "warning" });
    if (!yes) return;
  }
  const logo = draft.logo;
  await act("Changes saved.", async () => {
    await invoke("update_mod", { modId: entry.id, edit: changes, logo: logo || null });
    drafts.delete(entry.id);
    const saved = Object.fromEntries(Object.entries(changes).map(([key, value]) => [key, typeof value === "string" && key !== "description" ? value.trim() : value]));
    Object.assign(info, saved);
    saveLibrary();
    if (logo) await showChange(entry.id);
  });
}

async function addImages(id: number): Promise<void> {
  const picked = await open({ multiple: true, title: "Choose images", filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg"] }] });
  const paths = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
  if (!paths.length) return;
  await act(paths.length === 1 ? "Image added." : `Added ${paths.length} images.`, async () => {
    await invoke("add_images", { modId: id, paths });
    await showChange(id);
  });
}

async function addLink(id: number): Promise<void> {
  const link = state.linkDraft.trim();
  if (!link) return;
  const sketchfab = link.includes("sketchfab.com/");
  const added = await act("Link added.", async () => {
    await invoke("add_links", { modId: id, youtube: sketchfab ? [] : [link], sketchfab: sketchfab ? [link] : [] });
    await showChange(id);
  });
  if (added) {
    state.linkDraft = "";
    render();
  }
}

async function removeMedia(id: number, media: Partial<Record<"images" | "youtube" | "sketchfab", string[]>>): Promise<void> {
  const yes = await ask("This removes it from the mod's page.", { title: "Remove from gallery?", kind: "warning" });
  if (!yes) return;
  await act("Removed.", async () => {
    await invoke("remove_media", { modId: id, media: { images: [], youtube: [], sketchfab: [], ...media } });
    const info = state.infos[id];
    if (!info) return;
    info.images = info.images.filter((image) => !media.images?.includes(image.filename));
    info.youtube = info.youtube.filter((url) => !media.youtube?.includes(url));
    info.sketchfab = info.sketchfab.filter((url) => !media.sketchfab?.includes(url));
    saveLibrary();
  });
}

async function moveImage(info: ModInfo, index: number, by: number): Promise<void> {
  const images = [...info.images];
  const [moved] = images.splice(index, 1);
  images.splice(index + by, 0, moved);
  await act("", async () => {
    await invoke("order_media", {
      modId: info.id, media: { images: images.map((image) => image.filename), youtube: [], sketchfab: [] },
    });
    info.images = images;
    saveLibrary();
  });
}

async function addDependency(id: number): Promise<void> {
  const text = state.dependencyDraft.trim();
  if (!/^\d+$/.test(text)) {
    state.note = { ok: false, text: "Enter the mod's ID, which is a number." };
    return render();
  }
  const added = await act("Dependency added.", async () => {
    await invoke("add_dependency", { modId: id, dependency: Number(text) });
    await reloadSlot(id, "deps");
  });
  if (added) {
    state.dependencyDraft = "";
    render();
  }
}

async function addMetadata(id: number): Promise<void> {
  const { key, value } = state.metadataDraft;
  const added = await act("Metadata added.", async () => {
    await invoke("add_metadata", { modId: id, key, value });
    const slot = dataFor(id).metadata;
    slot.value = [...(slot.value ?? []), { key, value }];
  });
  if (added) {
    state.metadataDraft = { key: "", value: "" };
    render();
  }
}

async function postComment(id: number, content: string, replyId: number | null): Promise<boolean> {
  if (!content.trim()) return false;
  return act("Comment posted.", async () => {
    await invoke("post_comment", { modId: id, content, replyId });
    await reloadSlot(id, "comments");
  });
}

async function saveComment(): Promise<void> {
  const current = state.modal;
  if (current?.kind !== "edit-comment" || state.acting) return;
  const id = state.selectedId!;
  state.acting = true;
  renderModal();
  try {
    await invoke("update_comment", { modId: id, commentId: current.comment.id, content: current.content });
    current.comment.content = current.content.trim();
    state.acting = false;
    closeModal();
  } catch (error) {
    state.acting = false;
    state.modalError = String(error);
    renderModal();
  }
  render();
}

async function deleteComment(id: number, commentId: number): Promise<void> {
  const yes = await ask("This can't be undone.", { title: "Delete this comment?", kind: "warning" });
  if (!yes) return;
  await act("Comment deleted.", async () => {
    await invoke("remove_comment", { modId: id, commentId });
    await reloadSlot(id, "comments");
  });
}

async function closeMod(): Promise<void> {
  const id = state.selectedId!;
  state.acting = true;
  renderModal();
  try {
    await invoke("close_mod", { modId: id });
    state.acting = false;
    closeModal();
    delete state.infos[id];
    state.access.owned.delete(id);
    cache.delete(id);
    drafts.delete(id);
    state.selectedId = null;
    saveLibrary();
  } catch (error) {
    state.acting = false;
    state.modalError = String(error);
    renderModal();
  }
  render();
}

async function setupLocal(entry: Entry): Promise<void> {
  const done = await act("Plugin set up in your modkit.", async () => {
    await invoke("retry_setup", { modId: entry.id, kind: entry.kind });
    await loadMods();
  });
  if (done) await offerRestart(entry.name);
}

/* Source plugins */

let sourceTicket = 0;
let searchTimer = 0;

async function loadSource(): Promise<void> {
  const source = state.source;
  const ticket = ++sourceTicket;
  source.busy = true;
  render();
  try {
    const page = await invoke<{ mods: SourceMod[]; total: number }>("browse_source", {
      search: source.search, sort: source.sort, offset: source.offset,
    });
    if (ticket !== sourceTicket) return;
    source.mods = page.mods;
    source.total = page.total;
  } catch (error) {
    if (ticket !== sourceTicket) return;
    source.mods = [];
    source.total = 0;
    source.note = { ok: false, text: String(error) };
  }
  source.busy = false;
  render();
}

async function downloadSource(id: number): Promise<void> {
  const mod = state.source.mods.find((m) => m.id === id);
  const yes = await ask(
    `This replaces the plugin UGC${id} if you already have it. The old copy moves to Saved/Downloads.`,
    { title: `Download ${mod?.name ?? id}?`, kind: "warning" },
  );
  if (!yes) return;
  state.source.busy = true;
  state.source.downloading = id;
  state.source.note = null;
  render();
  try {
    await invoke("download_source", { modId: id });
    state.source.note = { ok: true, text: `Downloaded ${mod?.name ?? id}. Restart Unreal to see it.` };
    await loadMods();
  } catch (error) {
    state.source.note = { ok: false, text: String(error) };
  }
  state.source.busy = false;
  state.source.downloading = 0;
  render();
}

/* Runs */

let clock = 0;

function beginRun(action: Run["action"], stage: string, target: string, targets: string[]): Run {
  const entry = selectedEntry()!;
  const run: Run = {
    modId: entry.id, modName: entry.name, action,
    inactive: state.inactive, together: state.together, targets, builds: {}, stage, target,
    log: [], errors: 0, warnings: 0, verbose: state.verbose,
    result: null, startedAt: Date.now(), endedAt: null, stopping: false, showLog: state.verbose, copyLabel: "Copy",
  };
  state.run = run;
  window.clearInterval(clock);
  clock = window.setInterval(() => {
    const elapsed = runPanel.querySelector("#run-elapsed");
    if (elapsed) elapsed.textContent = duration(Date.now() - run.startedAt);
  }, 1000);
  render();
  return run;
}

function endRun(run: Run, ok: boolean, message: string, failure: Failure | null = null): void {
  window.clearInterval(clock);
  run.result = { ok, message, failure };
  run.endedAt = Date.now();
  run.stopping = false;
  // A failure needs the whole story, whatever the log was showing.
  if (!ok && !wasStopped(run)) {
    run.showLog = run.log.length > 0;
    run.verbose = true;
  }
}

/** Not being able to tell must never be what stops a build. */
async function editorStatus(): Promise<EditorState | null> {
  try {
    return await invoke<EditorState>("editor_status");
  } catch {
    return null;
  }
}

/** A build reads files, so work that's only in an open editor wouldn't be in it. */
async function confirmUnrealWork(): Promise<boolean> {
  const editor = await editorStatus();
  if (!editor?.unsaved) return true;
  const yes = await ask(`Unreal has ${editor.unsaved} unsaved ${editor.unsaved === 1 ? "asset" : "assets"}. They won't be in this build unless they're saved.`, {
    title: "Save your work first?", kind: "warning", okLabel: "Save and continue", cancelLabel: "Cancel",
  });
  if (!yes) return false;
  await invoke("save_unreal_work");
  return true;
}

/** An open editor can't load a new plugin, so offer the one thing that works. */
async function offerRestart(name: string): Promise<void> {
  const editor = await editorStatus();
  if (!editor?.open) return;
  if (!editor.connected) {
    state.note = { ok: true, text: `Restart Unreal to see ${name}.` };
    return render();
  }
  const yes = await ask(`Unreal only loads new mods when it starts. It will ask about any unsaved work first.`, {
    title: `Restart Unreal to see ${name}?`, kind: "info", okLabel: "Restart Unreal", cancelLabel: "Later",
  });
  if (!yes) return;
  state.note = { ok: true, text: "Restarting Unreal." };
  render();
  try {
    const restarted = await invoke<boolean>("restart_unreal");
    state.note = { ok: true, text: restarted ? "Unreal is opening again." : "Unreal stayed open. Restart it to see the new mod." };
  } catch (error) {
    state.note = { ok: false, text: String(error) };
  }
  render();
}

/** Check the selection, which can take seconds on a big mod, then start. */
async function startRun(start: () => Promise<void>): Promise<void> {
  const entry = selectedEntry();
  if (!entry || isRunning() || state.checking) return;
  state.checking = true;
  state.note = null;
  render();
  let ready = false;
  try {
    // Saving first, so the check reads the same files the build will.
    ready = await confirmUnrealWork() && await confirmSelection(entry);
  } catch (error) {
    state.note = { ok: false, text: String(error) };
  }
  state.checking = false;
  // Everything above was about this mod, so it can't start for another one.
  if (ready && selectedEntry()?.id !== entry.id) {
    ready = false;
    state.note = { ok: false, text: `Nothing was started, because you left ${entry.name} during the check.` };
  }
  render();
  if (ready) await start();
}

async function stage(): Promise<void> {
  const run = beginRun("stage", "cook", "windows", []);
  try {
    await invoke("stage_mod", { modId: run.modId });
  } catch (error) {
    endRun(run, false, String(error));
    render();
  }
}

async function publish(): Promise<void> {
  const run = beginRun("publish", "check", "", GROUP_TARGETS[state.group]);
  try {
    const shared = shareViews.get(run.modId);
    if (shared) await flushShare(run.modId, shared);
    await invoke("start_publish", {
      request: {
        mod_id: run.modId, group: state.group, changelog: state.changelog,
        inactive: state.inactive, compress: state.compress,
        together: state.together, share_source: state.shareSource && selectedEntry()?.kind === "Mod" && chosenCount(run.modId) > 0,
      },
    });
  } catch (error) {
    endRun(run, false, String(error));
    render();
  }
}

/* Settings actions */

async function afterAccountChange(): Promise<void> {
  cache.clear();
  await loadLibrary();
}

async function saveSettings(): Promise<void> {
  const { project, engine, stage, username } = state.settingsDraft;
  const settings = { project: project.trim(), engine: engine.trim(), stage: stage.trim(), username: username.trim() };
  const before = state.settings;
  try {
    const check = await invoke<FolderCheck | null>("check_folders", { project: settings.project, engine: settings.engine });
    settings.engine = check?.engine ?? settings.engine;
    await invoke("save_settings", { settings });
    if (settings.project !== before.project) {
      cookViews.clear();
      shareViews.clear();
      state.selectedId = null;
    }
    state.settings = settings;
    state.settingsDraft = { ...settings };
    const wasLocked = eulaLocked();
    await readEula();
    if (eulaLocked()) return render();
    if (wasLocked) await unlock();
    else await loadMods();
    state.settingsNote = check?.problems.length
      ? { ok: false, text: `Settings saved, but check them. ${check.problems.join(" ")}` }
      : { ok: true, text: "Settings saved." };
    render();
    if (settings.project !== before.project || settings.username !== before.username) await loadLibrary();
    return;
  } catch (error) {
    state.settingsNote = { ok: false, text: String(error) };
  }
  render();
}

async function saveToken(): Promise<void> {
  const token = state.tokenDraft.trim();
  if (!token || state.tokenBusy) return;
  state.tokenBusy = true;
  state.accountNote = null;
  render();
  try {
    await invoke("check_mod_token", { token });
    await invoke("save_token", { token });
    state.hasToken = await invoke<boolean>("has_token");
    state.tokenDraft = "";
    state.tokenBusy = false;
    state.accountNote = { ok: true, text: "Connected to mod.io." };
    render();
    await afterAccountChange();
    return;
  } catch (error) {
    state.accountNote = { ok: false, text: String(error) };
  }
  state.tokenBusy = false;
  render();
}

let pendingUpdate: Update | null = null;

/** Quiet when it runs by itself: no connection at launch isn't worth telling anyone. */
async function checkForUpdate(quiet: boolean): Promise<void> {
  if (["checking", "installing"].includes(state.update.status)) return;
  state.update = { status: "checking", version: "", percent: null, error: "" };
  render();
  try {
    pendingUpdate = await check();
    state.update.status = pendingUpdate ? "available" : "current";
    state.update.version = pendingUpdate?.version ?? "";
  } catch (error) {
    state.update.status = quiet ? "idle" : "failed";
    state.update.error = `Couldn't check for updates. ${error}`;
  }
  render();
  if (quiet && state.autoUpdate) await installUpdate();
}

/** Windows ends the app the moment the installer starts, so it can't happen during a run. */
async function installUpdate(): Promise<void> {
  if (!pendingUpdate || state.update.status !== "available" || isRunning() || state.checking) return;
  state.update.status = "installing";
  state.update.percent = null;
  render();
  let total = 0;
  let received = 0;
  try {
    await pendingUpdate.downloadAndInstall((event) => {
      if (event.event === "Started") total = event.data.contentLength ?? 0;
      if (event.event === "Progress") {
        received += event.data.chunkLength;
        if (total) state.update.percent = Math.min(100, Math.round((received / total) * 100));
        render();
      }
    });
  } catch (error) {
    state.update.status = "failed";
    state.update.error = `Couldn't install the update. ${error}`;
    render();
  }
}

async function saveUsername(): Promise<void> {
  const settings = { ...state.settings, username: state.settingsDraft.username.trim() };
  const changed = settings.username !== state.settings.username;
  try {
    await invoke("save_settings", { settings });
    state.settings = settings;
    state.settingsDraft.username = settings.username;
    state.accountNote = { ok: true, text: "Username saved." };
    render();
    if (changed) await loadLibrary();
    return;
  } catch (error) {
    state.accountNote = { ok: false, text: String(error) };
  }
  render();
}

async function createMod(): Promise<void> {
  state.creating = true;
  state.createError = "";
  render();
  try {
    const id = await invoke<number>("create_new_mod", { request: state.draft });
    const name = state.draft.name.trim();
    state.draft = { name: "", summary: "", kind: "Map", logo: "" };
    await loadMods();
    await loadLibrary();
    selectMod(id);
    await offerRestart(name);
  } catch (error) {
    state.createError = String(error);
    const failed = /mod (\d+) exists on mod\.io/.exec(state.createError);
    if (failed) state.failedSetup = { id: Number(failed[1]), kind: state.draft.kind };
  }
  state.creating = false;
  render();
}

async function pickLogo(): Promise<string | null> {
  const file = await open({ title: "Choose a logo image", filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg"] }] });
  return typeof file === "string" ? file : null;
}

async function copyLog(run: Run): Promise<void> {
  try {
    await navigator.clipboard.writeText(run.log.map((line) => line.text).join("\n"));
    run.copyLabel = "Copied";
  } catch {
    run.copyLabel = "Copy failed";
  }
  renderRun();
  window.setTimeout(() => {
    run.copyLabel = "Copy";
    renderRun();
  }, 1500);
}

/* Events */

function cachedFile(id: string | undefined): ModFile | undefined {
  return dataFor(state.selectedId!).files.value?.find((file) => file.id === Number(id));
}

async function handleClick(event: MouseEvent): Promise<void> {
  const target = (event.target as HTMLElement).closest<HTMLElement>("button");
  if (!target) return;
  const data = target.dataset;
  const entry = selectedEntry();
  const id = state.selectedId!;
  if (data.mod) return selectMod(Number(data.mod));
  if (data.tab) return selectTab(data.tab as Tab);
  if (data.report) {
    state.reportTarget = data.report;
    render();
    return void openTabData();
  }
  if (data.publishFile) return void publishFile(Number(data.publishFile));
  if (data.deleteFile) return void deleteFiles([Number(data.deleteFile)]);
  if (data.editFile) {
    const file = cachedFile(data.editFile);
    if (file) openModal({ kind: "edit-file", file, version: file.version, changelog: file.changelog });
    return;
  }
  if (data.downloadFile) {
    const url = cachedFile(data.downloadFile)?.download;
    if (url) void openUrl(url);
    return;
  }
  if (data.download) return void downloadSource(Number(data.download));
  if (data.page) {
    state.source.offset = Math.max(0, state.source.offset + Number(data.page));
    state.source.note = null;
    main.scrollTop = 0;
    return void loadSource();
  }
  if (data.kind) {
    state.draft.kind = data.kind;
    return render();
  }
  if (data.group) {
    state.group = data.group as Group;
    return void refreshPending().then(render);
  }
  if (data.visible && entry) {
    const draft = draftFor(entry);
    if (draft) draft.visible = data.visible === "1";
    return render();
  }
  if (data.moveImage && entry?.info) return void moveImage(entry.info, Number(data.moveImage), Number(data.by));
  if (data.removeImage) return void removeMedia(id, { images: [data.removeImage] });
  if (data.removeLink) return void removeMedia(id, { [data.linkKind!]: [data.removeLink] });
  if (data.removeDependency) {
    const dependency = Number(data.removeDependency);
    return void act("Dependency removed.", async () => {
      await invoke("remove_dependency", { modId: id, dependency });
      const slot = dataFor(id).deps;
      slot.value = slot.value?.filter((mod) => mod.id !== dependency);
    });
  }
  if (data.removeMetadata) {
    const pair = dataFor(id).metadata.value?.[Number(data.removeMetadata)];
    if (pair) {
      void act("Metadata removed.", async () => {
        await invoke("remove_metadata", { modId: id, key: pair.key, value: pair.value });
        const slot = dataFor(id).metadata;
        slot.value = slot.value?.filter((item) => item !== pair);
      });
    }
    return;
  }
  if (data.reply) {
    state.replyTo = Number(data.reply);
    state.replyDraft = "";
    render();
    return app.querySelector<HTMLElement>("#reply-draft")?.focus();
  }
  if (data.editComment) {
    const comment = dataFor(id).comments.value?.find((c) => c.id === Number(data.editComment));
    if (comment) openModal({ kind: "edit-comment", comment, content: comment.content });
    return;
  }
  if (data.deleteComment) return void deleteComment(id, Number(data.deleteComment));
  switch (data.action) {
    case "filter-used": return void filterOnlyUsed(id, cookFor(id));
    case "recheck": return void readReferences(id, cookFor(id)).then(render);
    case "pin-picked":
    case "unpin-picked": {
      const cook = cookFor(id);
      return setPinned(id, cook, selectedAssets(cook, visibleRows(cook)), data.action === "pin-picked");
    }
    case "keep-all": {
      const cook = cookFor(id);
      cook.note = null;
      changeExcluded(id, cook, (excluded) => excluded.clear());
      return render();
    }
    case "keep-missing": {
      const cook = cookFor(id);
      // Keeping one asset can reveal what it needs in turn, and it's still one change to undo.
      changeExcluded(id, cook, (excluded) => {
        for (let problems = broken(cook); problems.length; problems = broken(cook)) {
          problems.forEach(({ missing }) => excluded.delete(missing.path));
        }
      });
      return render();
    }
    case "accept-eula": return void acceptEula();
    case "decline-eula": return void invoke("quit_app");
    case "dismiss-note": state.note = null; return render();
    case "hide-access-hint":
      state.accessHintHidden = true;
      saveFlag(ACCESS_HINT_SETTING);
      return renderRail();
    case "cook-undo": return stepHistory(id, treeFor(id), treeFor(id).undo, treeFor(id).redo);
    case "cook-redo": return stepHistory(id, treeFor(id), treeFor(id).redo, treeFor(id).undo);
    case "share-clear": {
      const view = shareFor(id);
      return setIncluded(id, view, sharedFiles(view), false);
    }
    case "share-add-needed": {
      const view = shareFor(id);
      return setIncluded(id, view, view.share!.check?.add ?? [], true);
    }
    case "share-show-needed":
      shareFor(id).view = "needed";
      return render();
    case "share-older":
      state.fileFilter = "source";
      return selectTab("files");
    case "share-now": return void shareNow();
    case "share-export": return void exportShare();
    case "expand-all": {
      const cook = treeFor(id);
      const open = (folder: TreeFolder) => {
        cook.expanded.add(folder.path);
        folder.folders.forEach(open);
      };
      if (cook.root) open(cook.root);
      return renderTree(cook);
    }
    case "collapse-all": {
      const cook = treeFor(id);
      cook.expanded = new Set([""]);
      return renderTree(cook);
    }
    case "include-picked":
    case "exclude-picked": {
      const cook = treeFor(id);
      return setIncluded(id, cook, selectedAssets(cook, visibleRows(cook)), data.action === "include-picked");
    }
    case "settings": return showSettings();
    case "new-mod": state.view = "create"; state.createError = ""; return render();
    case "toggle-others": state.showOthers = !state.showOthers; return renderRail();
    case "refresh-all":
      if (state.cooling.all || waiting()) return;
      coolDown("all");
      cache.clear();
      for (const [modId, cook] of cookViews) {
        if (modId !== id && !selectionTimers.has(modId)) cook.report = null;
      }
      if (id) void reloadCook(id);
      return void loadLibrary();
    case "refresh-mod":
      if (state.cooling.mod || waiting()) return;
      coolDown("mod");
      return void refreshMod(id);
    case "source":
      state.view = "source";
      state.source.note = null;
      return state.source.mods.length ? render() : void loadSource();
    case "stage": return void startRun(stage);
    case "publish": return void startRun(publish);
    case "more-files":
      state.filesShown += FILES_SHOWN;
      if (dataFor(id).filesNext !== null && matchingFiles(dataFor(id).files.value ?? []).length < state.filesShown) return void moreFiles(id);
      return render();
    case "more-comments": return void moreComments(id);
    case "delete-picked": return void deleteFiles([...state.pickedFiles]);
    case "save-page": return entry ? void savePage(entry) : undefined;
    case "reset-page": drafts.delete(id); return render();
    case "pick-page-logo": {
      const file = await pickLogo();
      const draft = entry && draftFor(entry);
      if (file && draft) {
        draft.logo = file;
        render();
      }
      return;
    }
    case "add-images": return void addImages(id);
    case "add-link": return void addLink(id);
    case "add-dependency": return void addDependency(id);
    case "add-metadata": return void addMetadata(id);
    case "post-comment": {
      const posted = await postComment(id, state.commentDraft, null);
      if (posted) {
        state.commentDraft = "";
        render();
      }
      return;
    }
    case "post-reply": {
      const posted = await postComment(id, state.replyDraft, state.replyTo);
      if (posted) {
        state.replyTo = null;
        state.replyDraft = "";
        render();
      }
      return;
    }
    case "cancel-reply":
      state.replyTo = null;
      return render();
    case "close-mod": return openModal({ kind: "close-mod", typed: "" });
    case "close-modal": return closeModal();
    case "save-file": return void saveFile();
    case "save-comment": return void saveComment();
    case "confirm-close": return void closeMod();
    case "setup-local": return entry ? void setupLocal(entry) : undefined;
    case "cancel":
      if (!state.run) return;
      state.run.stopping = true;
      renderRun();
      await invoke("cancel_run");
      return;
    case "toggle-log":
      if (!state.run) return;
      state.run.showLog = !state.run.showLog;
      return renderRun();
    case "copy-log":
      if (state.run) void copyLog(state.run);
      return;
    case "reveal-log":
      if (state.run?.result?.failure?.log_path) void revealItemInDir(state.run.result.failure.log_path);
      return;
    case "dismiss-run":
      state.run = null;
      return render();
    case "discard": {
      const yes = await ask("The cooked builds won't be used, and the next publish cooks everything again.", {
        title: `Start over from version ${state.pending}?`, kind: "warning",
      });
      if (!yes) return;
      await invoke("discard_release", { modId: id });
      state.pending = null;
      return render();
    }
    case "open-profile":
      if (entry?.info?.profile_url) void openUrl(entry.info.profile_url);
      return;
    case "pick-logo": {
      const file = await pickLogo();
      if (file) {
        state.draft.logo = file;
        render();
      }
      return;
    }
    case "browse": {
      const folder = await open({ directory: true, title: "Choose your modkit folder" });
      if (typeof folder === "string") {
        state.settingsDraft.project = folder;
        render();
      }
      return;
    }
    case "browse-engine": {
      const folder = await open({ directory: true, title: "Choose the Engine folder inside UE_5.1" });
      if (typeof folder === "string") {
        state.settingsDraft.engine = folder;
        render();
      }
      return;
    }
    case "browse-stage": {
      const folder = await open({ directory: true, title: "Choose a staging folder" });
      if (typeof folder === "string") {
        state.settingsDraft.stage = folder;
        render();
      }
      return;
    }
    case "retry-setup": {
      const failed = state.failedSetup!;
      state.createError = "";
      try {
        await invoke("retry_setup", { modId: failed.id, kind: failed.kind });
        state.failedSetup = null;
        await loadMods();
        await loadLibrary();
        selectMod(failed.id);
        return await offerRestart(modNameOf(failed.id));
      } catch (error) {
        state.createError = String(error);
        return render();
      }
    }
    case "create-mod": return void createMod();
    case "terms": return void openUrl("https://mod.io/terms");
    case "save-settings": return void saveSettings();
    case "save-token": return void saveToken();
    case "save-username": return void saveUsername();
    case "check-update": return void checkForUpdate(false);
    case "install-update": return void installUpdate();
    case "whats-new": return void openUrl(RELEASES_PAGE);
    case "token-page": return void openUrl(TOKEN_PAGE);
    case "remove-token":
      await invoke("remove_token");
      state.hasToken = false;
      state.accountNote = null;
      cache.clear();
      return void loadLibrary();
  }
}

/** Nothing should fail without saying so. */
function showFailure(error: unknown): void {
  state.note = { ok: false, text: String(error) };
  render();
}

app.addEventListener("click", (event) => {
  handleClick(event).catch(showFailure);
});
window.addEventListener("unhandledrejection", (event) => showFailure(event.reason));

app.addEventListener("change", (event) => {
  const input = event.target as HTMLInputElement;
  const entry = selectedEntry();
  if (input.id === "inactive") {
    state.inactive = input.checked;
    render();
  }
  if (input.id === "uncompressed") {
    state.compress = !input.checked;
    void refreshPending().then(render);
  }
  if (input.id === "together") state.together = input.checked;
  if (input.id === "verbose") saveVerbose(input.checked);
  if (input.id === "auto-update") {
    state.autoUpdate = input.checked;
    saveAutoUpdate(input.checked);
  }
  if (input.id === "run-verbose" && state.run) {
    state.run.verbose = input.checked;
    saveVerbose(input.checked);
    paintLog();
    const box = app.querySelector<HTMLInputElement>("#verbose");
    if (box) box.checked = input.checked;
  }
  if (input.id === "share-source") state.shareSource = input.checked;
  if (input.id === "source-sort") {
    Object.assign(state.source, { sort: input.value, offset: 0, note: null });
    void loadSource();
  }
  if (input.id === "asset-view" && state.selectedId) {
    treeFor(state.selectedId).view = input.value as AssetView;
    renderTree(treeFor(state.selectedId));
  }
  if (input.id === "file-filter") {
    state.fileFilter = input.value;
    state.filesShown = FILES_SHOWN;
    // Only what can be seen should be deleted.
    state.pickedFiles.clear();
    render();
  }
  if (input.id === "pick-all" && state.selectedId) {
    const entry2 = selectedEntry();
    const live = new Set(entry2?.info?.platforms.map((p) => p.file_id));
    const files = matchingFiles(dataFor(state.selectedId).files.value ?? [])
      .slice(0, state.filesShown)
      .filter((f) => !live.has(f.id));
    files.forEach((f) => (input.checked ? state.pickedFiles.add(f.id) : state.pickedFiles.delete(f.id)));
    render();
  }
  if (input.dataset.pick) {
    const id = Number(input.dataset.pick);
    if (input.checked) state.pickedFiles.add(id);
    else state.pickedFiles.delete(id);
    render();
  }
  const draft = entry && draftFor(entry);
  if (draft && input.dataset.flag) {
    const key = input.dataset.flag as "maturity_option" | "community_options" | "credit_options";
    const bit = Number(input.dataset.bit);
    draft[key] = input.checked ? draft[key] | bit : draft[key] & ~bit;
    render();
  }
  if (draft && input.id === "credit-patch") {
    draft.credit_options = (draft.credit_options & ~PATCH_MASK) | Number(input.value);
    render();
  }
  if (draft && input.id === "credit-repack") {
    draft.credit_options = (draft.credit_options & ~REPACK_MASK) | Number(input.value);
    render();
  }
});

app.addEventListener("keydown", (event) => {
  const input = event.target as HTMLElement;
  if (input.id === "source-search" && event.key === "Enter") {
    window.clearTimeout(searchTimer);
    Object.assign(state.source, { offset: 0, note: null });
    void loadSource();
  }
  if (event.key === "Enter" && !event.shiftKey) {
    const id = state.selectedId;
    const commentKey = event.ctrlKey || event.metaKey;
    if (id && input.id === "link-draft") void addLink(id);
    if (id && input.id === "dependency-draft" && state.dependencyDraft.trim()) void addDependency(id);
    if (id && (input.id === "metadata-key" || input.id === "metadata-value") && state.metadataDraft.key.trim() && state.metadataDraft.value) void addMetadata(id);
    if (input.id === "token") void saveToken();
    if (input.id === "modal-version" && modalReady()) void saveFile();
    if (input.id === "modal-confirm" && modalReady()) void closeMod();
    if (commentKey && input.id === "modal-comment" && modalReady()) void saveComment();
    if (commentKey && input.id === "comment-draft") app.querySelector<HTMLElement>('[data-action="post-comment"]')?.click();
    if (commentKey && input.id === "reply-draft") app.querySelector<HTMLElement>('[data-action="post-reply"]')?.click();
  }
  const entry = selectedEntry();
  if (entry && input.getAttribute("role") === "tab" && ["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
    const tabs = tabsFor(entry);
    const at = tabs.indexOf(activeTab(entry));
    const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : (at + (event.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length;
    event.preventDefault();
    selectTab(tabs[next]);
    app.querySelector<HTMLElement>(`#tab-${state.tab}`)?.focus();
  }
});

/* The asset tree works like Windows Explorer, so it has its own handlers. */
app.addEventListener("click", (event) => {
  const element = event.target as HTMLElement;
  const row = element.closest<HTMLElement>("[data-row]");
  const id = state.selectedId;
  if (!row || !id) return;
  const cook = treeFor(id);
  const key = row.dataset.row!;
  const item = visibleRows(cook).find((candidate) => candidate.key === key);
  if (!item) return;
  const tree = main.querySelector<HTMLElement>("#asset-tree");
  if (element.closest("[data-twist]")) {
    if (item.folder) toggleFolder(cook, item.folder.path);
  } else if (element.matches("[data-cook]")) {
    const included = (element as HTMLInputElement).checked;
    const paths = cook.selected.has(key) ? selectedAssets(cook, visibleRows(cook)) : assetsOf(item);
    setIncluded(id, cook, paths, included);
  } else {
    selectRow(cook, key, event.ctrlKey || event.metaKey, event.shiftKey);
  }
  // Keep the keyboard on the tree, whichever part of a row was clicked.
  tree?.focus({ preventScroll: true });
});

app.addEventListener("dblclick", (event) => {
  const element = event.target as HTMLElement;
  const key = element.closest<HTMLElement>("[data-row]")?.dataset.row;
  if (element.closest("[data-twist]") || element.matches("[data-cook]")) return;
  if (key?.startsWith("d:") && state.selectedId) toggleFolder(treeFor(state.selectedId), key.slice(2));
});

app.addEventListener("keydown", (event) => {
  if ((event.target as HTMLElement).id === "asset-tree" && state.selectedId) treeKey(event, state.selectedId, treeFor(state.selectedId));
});

modal.addEventListener("cancel", (event) => {
  event.preventDefault();
  if (!state.acting) closeModal();
});

function modalReady(): boolean {
  const current = state.modal;
  if (current?.kind === "edit-file") return Boolean(current.version.trim());
  if (current?.kind === "edit-comment") return Boolean(current.content.trim());
  if (current?.kind === "close-mod") return current.typed.trim() === (selectedEntry()?.name ?? "").trim();
  return false;
}

function refreshSaveButtons(entry: Entry): void {
  const draft = drafts.get(entry.id);
  if (!draft || !entry.info) return;
  const dirty = Object.keys(pageChanges(entry.info, draft)).length > 0 || Boolean(draft.logo);
  app.querySelectorAll<HTMLButtonElement>('[data-action="save-page"], [data-action="reset-page"]')
    .forEach((button) => { button.disabled = !dirty || state.acting; });
}

app.addEventListener("input", (event) => {
  const input = event.target as HTMLInputElement;
  const entry = selectedEntry();
  const bound: Record<string, (value: string) => void> = {
    changelog: (v) => { state.changelog = v; },
    "draft-name": (v) => { state.draft.name = v; },
    "draft-summary": (v) => { state.draft.summary = v; },
    project: (v) => { state.settingsDraft.project = v; },
    engine: (v) => { state.settingsDraft.engine = v; },
    stage: (v) => { state.settingsDraft.stage = v; },
    username: (v) => { state.settingsDraft.username = v; },
    token: (v) => { state.tokenDraft = v; },
    "comment-draft": (v) => { state.commentDraft = v; },
    "reply-draft": (v) => { state.replyDraft = v; },
    "link-draft": (v) => { state.linkDraft = v; },
    "dependency-draft": (v) => { state.dependencyDraft = v; },
    "metadata-key": (v) => { state.metadataDraft.key = v; },
    "metadata-value": (v) => { state.metadataDraft.value = v; },
  };
  bound[input.id]?.(input.value);
  const pageFields: Record<string, keyof PageDraft> = {
    "page-name": "name", "page-name-id": "name_id", "page-summary": "summary",
    "page-description": "description", "page-homepage": "homepage_url",
  };
  const field = pageFields[input.id];
  if (field && entry) {
    const draft = draftFor(entry);
    if (draft) (draft[field] as string) = input.value;
    refreshSaveButtons(entry);
  }
  const current = state.modal;
  if (current?.kind === "edit-file" && input.id === "modal-version") current.version = input.value;
  if (current?.kind === "edit-file" && input.id === "modal-changelog") current.changelog = input.value;
  if (current?.kind === "edit-comment" && input.id === "modal-comment") current.content = input.value;
  if (current?.kind === "close-mod" && input.id === "modal-confirm") current.typed = input.value;
  if (input.id.startsWith("modal-")) {
    const button = modal.querySelector<HTMLButtonElement>('[data-action="save-file"], [data-action="save-comment"], [data-action="confirm-close"]');
    if (button) button.disabled = !modalReady() || state.acting;
  }
  const needs: Record<string, [string, boolean]> = {
    "comment-draft": ["post-comment", Boolean(state.commentDraft.trim())],
    "reply-draft": ["post-reply", Boolean(state.replyDraft.trim())],
    "link-draft": ["add-link", Boolean(state.linkDraft.trim())],
    "dependency-draft": ["add-dependency", Boolean(state.dependencyDraft.trim())],
    "metadata-key": ["add-metadata", Boolean(state.metadataDraft.key.trim() && state.metadataDraft.value)],
    "metadata-value": ["add-metadata", Boolean(state.metadataDraft.key.trim() && state.metadataDraft.value)],
    token: ["save-token", Boolean(state.tokenDraft.trim())],
  };
  const need = needs[input.id];
  if (need) {
    const button = app.querySelector<HTMLButtonElement>(`[data-action="${need[0]}"]`);
    if (button) button.disabled = !need[1] || state.acting;
  }
  if (input.id === "mod-filter") {
    state.filter = input.value;
    renderRail();
  }
  if (input.id === "asset-search" && state.selectedId) {
    treeFor(state.selectedId).query = input.value;
    renderTree(treeFor(state.selectedId));
  }
  if (input.id === "share-note" && state.selectedId) shareFor(state.selectedId).share!.changeNote = input.value;
  if (input.id === "source-search") {
    state.source.search = input.value;
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => {
      Object.assign(state.source, { offset: 0, note: null });
      void loadSource();
    }, SEARCH_DELAY);
  }
  if (input.id.startsWith("draft-")) {
    const button = app.querySelector<HTMLButtonElement>('[data-action="create-mod"]');
    if (button) button.disabled = !createReady();
  }
});

await listen<Progress>("progress", ({ payload }) => {
  if (payload.kind === "waiting") {
    if (payload.seconds >= WAIT_SHOWN_FROM) startWait(payload.seconds);
    return;
  }
  const run = state.run;
  if (!run) return;
  switch (payload.kind) {
    case "log":
      return addLines(run, [{ text: payload.line, level: "step" }]);
    case "output":
      return addLines(run, payload.lines.map(({ text, level }) => ({ text, level })));
    case "stage":
      run.stage = payload.stage;
      run.target = payload.target ?? "";
      return renderRun();
    case "parts":
      run.builds[payload.target] = { done: payload.done, total: payload.total };
      if (!run.targets.includes(payload.target)) run.targets.push(payload.target);
      return renderRun();
    case "finished":
      endRun(run, payload.ok, payload.message, payload.failure);
      if (payload.ok && run.action === "publish") {
        changelogs.delete(run.modId);
        if (state.selectedId === run.modId) state.changelog = "";
      }
      if (payload.ok && run.action === "share") shareFor(run.modId).share!.changeNote = "";
      // Any run can change what's in a build, whether or not it finished.
      dataFor(run.modId).reports = {};
      render();
      void loadBuilt(run.modId);
      if (payload.ok) void refreshAfterRun(run);
      else void refreshPending().then(render);
  }
});

async function refreshAfterRun(run: Run): Promise<void> {
  if (run.action === "stage") return void refreshPending().then(render);
  cache.delete(run.modId);
  await showChange(run.modId);
  await refreshPending();
  await reloadShare(run.modId);
  render();
  await openTabData();
}

/** Coming back from Unreal or Explorer means the files may have changed. */
let checkingFiles = false;
window.addEventListener("focus", async () => {
  if (checkingFiles || eulaLocked()) return;
  checkingFiles = true;
  await loadMods();
  const entry = selectedEntry();
  if (entry?.local && state.view === "mod" && ["cook", "publish"].includes(activeTab(entry))) await reloadCook(entry.id);
  if (entry?.local && state.view === "mod" && ["source", "publish"].includes(activeTab(entry))) await reloadShare(entry.id);
  render();
  checkingFiles = false;
});

/* Browser behaviour that doesn't belong in a desktop app. */
document.addEventListener("contextmenu", (event) => {
  const element = event.target as HTMLElement;
  if (!element.closest("input, textarea, pre, .select-text")) event.preventDefault();
});
document.addEventListener("keydown", (event) => {
  const key = event.key.toLowerCase();
  const ctrl = event.ctrlKey || event.metaKey;
  const browser = key === "f5" || key === "f3" || key === "f7" || (ctrl && ["r", "p", "f", "g", "u", "j", "s", "o"].includes(key)) || (event.altKey && (key === "arrowleft" || key === "arrowright"));
  if (browser) event.preventDefault();
}, true);

try {
  state.settings = await invoke<Settings>("load_settings");
  await readEula();
  if (!eulaLocked()) await unlock();
} catch (error) {
  state.note = { ok: false, text: String(error) };
}
if (state.eula) render();
else showSettings();
window.setInterval(() => {
  if (state.view === "settings") void readUsage().then(paintUsage);
}, USAGE_SECONDS * 1000);
if (!eulaLocked()) void loadLibrary();
if (state.autoUpdate) void checkForUpdate(true);
getVersion().then((version) => {
  state.version = version ?? "";
  if (state.view === "settings") render();
}).catch(() => undefined);
