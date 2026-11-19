import { invoke, Channel } from "@tauri-apps/api/core";

export type Loader = "forge" | "neoforge" | "fabric" | "quilt" | "vanilla";

export interface ModEntry {
  file_name: string;
  name_hint: string | null;
  slug_hint: string | null;
  size: number | null;
  origin: unknown | null;
  download_url: string | null;
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
export interface ScanResult { pack: ImportedPack; resolved: ResolvedMod[] }
export interface ExportReport {
  output: string; matched_exact: number; matched_fuzzy: number; bundled: string[];
  skipped: { file_name: string; reason: string }[]; overrides: number;
}
export type ProgressEvent =
  | { kind: "stage"; name: string }
  | { kind: "progress"; done: number; total: number }
  | { kind: "log"; message: string };

function channel(onEvent: (e: ProgressEvent) => void) {
  const c = new Channel<ProgressEvent>();
  c.onmessage = onEvent;
  return c;
}

export const api = {
  scan: (path: string, o: { name?: string; mcVersion?: string; loader?: string }, onEvent: (e: ProgressEvent) => void) =>
    invoke<ScanResult>("scan_pack", {
      path, name: o.name || null, mcVersion: o.mcVersion || null, loader: o.loader || null, onEvent: channel(onEvent),
    }),
  exportPack: (
    scan: ScanResult, output: string,
    o: { bundleUnmatched: boolean; downloadRemote: boolean; loaderVersion?: string },
    onEvent: (e: ProgressEvent) => void,
  ) =>
    invoke<ExportReport>("export_pack", {
      pack: scan.pack, resolved: scan.resolved, output,
      bundleUnmatched: o.bundleUnmatched, downloadRemote: o.downloadRemote,
      loaderVersion: o.loaderVersion || null, onEvent: channel(onEvent),
    }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  secretStatus: () => invoke<{ modrinth: boolean; curseforge: boolean }>("secret_status"),
  saveSecret: (kind: "modrinth" | "curseforge", value: string) => invoke<void>("save_secret", { kind, value }),
  clearSecret: (kind: "modrinth" | "curseforge") => invoke<void>("clear_secret", { kind }),
  publish: (req: PublishRequest, onEvent: (e: ProgressEvent) => void) =>
    invoke<PublishReport>("publish_project", { req, onEvent: channel(onEvent) }),
};

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
