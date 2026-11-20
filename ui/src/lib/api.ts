import { invoke as tauriInvoke, Channel } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";

export type Loader = "forge" | "neoforge" | "fabric" | "quilt" | "vanilla";

export interface ModEntry {
  file_name: string;
  name_hint: string | null;
  slug_hint: string | null;
  size: number | null;
  origin: unknown | null;
  download_url: string | null;
  cf: { project_id: number; file_id: number } | null;
  expected_file_name: string | null;
  cf_url: string | null;
  lookup_sources: ("core_api" | "curse_maven" | "cf_widget")[];
}
export interface FileRef { url: string; filename: string; sha1: string; sha512: string; size: number }
export interface Candidate {
  project_id: string; slug: string; title: string; version_id: string; version_number: string; file: FileRef;
}
export type Resolution =
  | { status: "matched"; candidate: Candidate; confidence: "exact" | "fuzzy" }
  | { status: "ambiguous"; candidates: Candidate[] }
  | { status: "unmatched"; reason: string };
export interface ResolvedMod { entry: ModEntry; resolution: Resolution }
export interface ImportedPack {
  source: string; name: string; version: string; mc_version: string; loader: Loader;
  loader_version: string | null; mods: ModEntry[]; overrides: unknown[];
  [k: string]: unknown;
}

/** A mod that needs a file the user downloads by hand. */
export interface ManualTask {
  index: number;
  project_id: number;
  file_id: number;
  project_name: string | null;
  project_url: string;
  file_url: string | null;
  expected_file_name: string | null;
  expected_sha1: string | null;
  exactness: "hash" | "file_name" | "unverifiable";
}
export interface ScanResult {
  pack: ImportedPack;
  resolved: ResolvedMod[];
  tasks: ManualTask[];
  notices: string[];
}
export type Verdict =
  | { kind: "exact_hash" }
  | { kind: "exact_file_name" }
  | { kind: "unverified"; note: string }
  | { kind: "mismatch"; reason: string };
export interface ManualResult {
  path: string; file_name: string; index: number | null; mod_name: string | null;
  verdict: Verdict | null; applied: boolean; on_modrinth: boolean; message: string;
}
export interface ManualBatch { resolved: ResolvedMod[]; results: ManualResult[]; tasks: ManualTask[] }
export interface ExportReport {
  output: string; matched_exact: number; matched_fuzzy: number; bundled: string[];
  skipped: { file_name: string; reason: string; link: string | null }[]; overrides: number;
}
export interface ModpackHit {
  project_id: number; name: string; summary: string; downloads: number; thumbnail: string | null; url: string;
}
export interface FileOption {
  id: number; name: string; game_versions: string[]; release_type: number; date: string | null; downloadable: boolean;
}
export interface ModpackDetail extends ModpackHit { files: FileOption[]; selected_file_id: number }
export interface FetchOutcome { path: string | null; manual_url: string | null; message: string }
export interface Setting { key: string; title: string; description: string; value: boolean }
export type ProgressEvent =
  | { kind: "stage"; name: string }
  | { kind: "progress"; done: number; total: number }
  | { kind: "log"; message: string };

/** True inside the desktop app; false in a plain browser (dev preview), where a fake backend is used. */
export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return tauriInvoke<T>(cmd, args);
  if (import.meta.env.DEV) {
    const { mockInvoke } = await import("./mock");
    return mockInvoke(cmd, args ?? {}) as Promise<T>;
  }
  throw new Error("Not running inside the desktop app");
}

function channel(onEvent: (e: ProgressEvent) => void) {
  if (!isTauri) return undefined;
  const c = new Channel<ProgressEvent>();
  c.onmessage = onEvent;
  return c;
}

export const api = {
  scan: (path: string, o: { name?: string; mcVersion?: string; loader?: string }, onEvent: (e: ProgressEvent) => void) =>
    invoke<ScanResult>("scan_pack", {
      path, name: o.name || null, mcVersion: o.mcVersion || null, loader: o.loader || null, onEvent: channel(onEvent),
    }),
  applyManual: (resolved: ResolvedMod[], files: { path: string; index?: number }[], o: { acceptUnverified?: boolean; force?: boolean } = {}) =>
    invoke<ManualBatch>("apply_manual", {
      resolved, files: files.map((f) => ({ path: f.path, index: f.index ?? null })),
      acceptUnverified: !!o.acceptUnverified, force: !!o.force,
    }),
  exportPack: (scan: ScanResult, output: string, o: { loaderVersion?: string }, onEvent: (e: ProgressEvent) => void) =>
    invoke<ExportReport>("export_pack", {
      pack: scan.pack, resolved: scan.resolved, output, loaderVersion: o.loaderVersion || null, onEvent: channel(onEvent),
    }),
  cfSearch: (query: string) => invoke<ModpackHit[]>("cf_search", { query }),
  cfResolve: (input: string) => invoke<ModpackDetail>("cf_resolve", { input }),
  cfFetch: (projectId: number, fileId: number, onEvent: (e: ProgressEvent) => void) =>
    invoke<FetchOutcome>("cf_fetch", { projectId, fileId, onEvent: channel(onEvent) }),
  discardScan: () => invoke<void>("discard_scan"),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  openLink: (url: string) => invoke<void>("open_link", { url }),
  defaultDownloadsDir: () => invoke<string | null>("default_downloads_dir"),
  getSettings: () => invoke<Setting[]>("get_settings"),
  setSetting: (key: string, value: boolean) => invoke<void>("set_setting", { key, value }),
  secretStatus: () => invoke<{ modrinth: boolean; curseforge: boolean }>("secret_status"),
  /** Resolves to a warning when the saved key looks like the wrong kind, otherwise null. */
  saveSecret: (kind: "modrinth" | "curseforge", value: string) => invoke<string | null>("save_secret", { kind, value }),
  clearSecret: (kind: "modrinth" | "curseforge") => invoke<void>("clear_secret", { kind }),
  runDiagnostics: () => invoke<string>("run_diagnostics"),
  saveDebugLog: (path: string) => invoke<void>("save_debug_log", { path }),
  debugLogText: () => invoke<string>("debug_log_text"),
  publish: (req: PublishRequest, onEvent: (e: ProgressEvent) => void) =>
    invoke<PublishReport>("publish_project", { req, onEvent: channel(onEvent) }),
};

// ---- file dialogs (a plain browser gets canned answers so the UI can be exercised) ----

export async function pickPack(directory: boolean): Promise<string | null> {
  if (!isTauri) return import.meta.env.DEV ? "/Users/demo/Downloads/All-the-Mods-demo.zip" : null;
  const r = await openDialog({
    directory, multiple: false,
    filters: directory ? undefined : [{ name: "Modpack", extensions: ["zip", "mrpack"] }],
  });
  return typeof r === "string" ? r : null;
}
export async function pickJars(multiple: boolean): Promise<string[]> {
  if (!isTauri) return import.meta.env.DEV ? ["/Users/demo/Downloads/five-2.3 (1).jar"] : [];
  const r = await openDialog({ multiple, filters: [{ name: "Mod jar", extensions: ["jar"] }] });
  return Array.isArray(r) ? r : typeof r === "string" ? [r] : [];
}
export async function pickFolder(): Promise<string | null> {
  if (!isTauri) return import.meta.env.DEV ? "/Users/demo/Downloads" : null;
  const r = await openDialog({ directory: true, multiple: false });
  return typeof r === "string" ? r : null;
}
export async function pickSavePath(defaultPath: string, ext: string, label: string): Promise<string | null> {
  if (!isTauri) return import.meta.env.DEV ? `/Users/demo/${defaultPath}` : null;
  return saveDialog({ defaultPath, filters: [{ name: label, extensions: [ext] }] });
}

export interface PublishRequest {
  source: "jars" | "github" | "curseforge";
  value: string; // newline-separated paths, owner/repo, or project id
  versionNumber: string;
  slug: string; title: string; summary: string; body: string; license: string;
  categories: string; clientSide: string; serverSide: string;
  gameVersions: string; loaders: string;
  existing: boolean; dryRun: boolean;
}
export interface PublishReport {
  project_id: string | null; created_project: boolean; uploaded: string[]; skipped: string[]; dry_run: boolean;
}
