// Fake backend for the browser dev preview only (never bundled into the desktop app).
// It exists so every screen and state can be looked at without the Rust side.
import type { ManualBatch, ManualResult, ManualTask, ResolvedMod, ScanResult, Setting } from "./api";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

const settings: Setting[] = [
  { key: "download", title: "Download mods that aren't on Modrinth", description: "Mods without an exact match on Modrinth are downloaded from their original source (CurseForge) and bundled in the pack. If one can't be downloaded you are asked to supply it.", value: true },
  { key: "bundle", title: "Bundle files that aren't on Modrinth", description: "Put those files (and any you supply by hand) inside the pack. Turn off to list them in the report instead.", value: true },
  { key: "match-by-name", title: "Match by name when there is no exact match", description: "Search Modrinth for a similar mod and use its newest compatible version. This substitutes different files, so it is off by default: a modpack is assumed not to exist on Modrinth.", value: false },
  { key: "keyless", title: "Keyless CurseForge lookup (experimental)", description: "Without a working CurseForge API key, look files up through the community services cursemaven.com and api.cfwidget.com. Your pack's CurseForge project and file IDs are sent to them.", value: false },
];

const entry = (o: Partial<ResolvedMod["entry"]> & { file_name: string }): ResolvedMod["entry"] => ({
  name_hint: null, slug_hint: null, size: null, origin: null, download_url: null, cf: null,
  expected_file_name: null, cf_url: null, lookup_sources: [], ...o,
});
const cand = (title: string, slug: string) => ({
  project_id: slug, slug, title, version_id: "v", version_number: "1.0.0",
  file: { url: "", filename: `${slug}.jar`, sha1: "", sha512: "", size: 1 },
});

function fixture(): ResolvedMod[] {
  return [
    { entry: entry({ file_name: "jei-1.20.1-fabric-15.75.0.236.jar", name_hint: "Just Enough Items (JEI)", cf: { project_id: 238222, file_id: 9701664 }, lookup_sources: ["core_api"] }),
      resolution: { status: "matched", confidence: "exact", candidate: cand("Just Enough Items", "jei") } },
    { entry: entry({ file_name: "fabric-api-0.92.2+1.20.1.jar", name_hint: "Fabric API", cf: { project_id: 306612, file_id: 5000001 }, lookup_sources: ["core_api"] }),
      resolution: { status: "matched", confidence: "exact", candidate: cand("Fabric API", "fabric-api") } },
    { entry: entry({ file_name: "create-fabric-0.5.1.jar", name_hint: "Create", origin: { type: "file", path: "/tmp/dl/create.jar" }, cf: { project_id: 328085, file_id: 5000002 }, lookup_sources: ["core_api"] }),
      resolution: { status: "unmatched", reason: "not on Modrinth" } },
    { entry: entry({ file_name: "my-local-tweaks.jar", origin: { type: "file", path: "/mods/my-local-tweaks.jar" } }),
      resolution: { status: "unmatched", reason: "not on Modrinth" } },
    { entry: entry({ file_name: "restricted-mod-3.1.jar", name_hint: "Restricted Mod", cf: { project_id: 111, file_id: 1110 }, expected_file_name: "restricted-mod-3.1.jar", cf_url: "https://www.curseforge.com/minecraft/mc-mods/restricted-mod", lookup_sources: ["core_api"] }),
      resolution: { status: "unmatched", reason: "download failed: the author disabled third-party downloads" } },
    { entry: entry({ file_name: "five-2.3.jar", name_hint: "Five Mod", cf: { project_id: 5, file_id: 55 }, expected_file_name: "five-2.3.jar", cf_url: "https://www.curseforge.com/minecraft/mc-mods/five-mod", lookup_sources: ["cf_widget"] }),
      resolution: { status: "unmatched", reason: "not on Modrinth and no source to download it from" } },
    { entry: entry({ file_name: "cf-99999999-1.jar", cf: { project_id: 99999999, file_id: 1 } }),
      resolution: { status: "unmatched", reason: "not on Modrinth and no source to download it from" } },
  ];
}

function tasks(resolved: ResolvedMod[]): ManualTask[] {
  const out: ManualTask[] = [];
  resolved.forEach((r, index) => {
    const e = r.entry;
    if (r.resolution.status === "matched" || !e.cf || e.origin || e.download_url) return;
    const site = e.cf_url ?? null;
    out.push({
      index, project_id: e.cf.project_id, file_id: e.cf.file_id, project_name: e.name_hint,
      project_url: site ?? `https://www.curseforge.com/projects/${e.cf.project_id}`,
      file_url: site ? `${site}/files/${e.cf.file_id}` : null,
      expected_file_name: e.expected_file_name, expected_sha1: e.file_name.startsWith("restricted") ? "ab12cd34ef56" : null,
      exactness: e.file_name.startsWith("restricted") ? "hash" : e.expected_file_name ? "file_name" : "unverifiable",
    });
  });
  return out;
}

function applyManual(resolved: ResolvedMod[], files: { path: string; index: number | null }[], acceptUnverified: boolean, force: boolean): ManualBatch {
  const next: ResolvedMod[] = JSON.parse(JSON.stringify(resolved));
  const results: ManualResult[] = [];
  const waiting = tasks(next);
  for (const f of files) {
    const name = f.path.split("/").pop() ?? f.path;
    const idx = f.index ?? waiting.find((t) => t.expected_file_name && name.replace(/ \(\d+\)/, "") === t.expected_file_name)?.index ?? null;
    if (idx === null) { results.push({ path: f.path, file_name: name, index: null, mod_name: null, verdict: null, applied: false, on_modrinth: false, message: "this file doesn't match any mod that still needs a file" }); continue; }
    const e = next[idx].entry;
    const expected = e.expected_file_name;
    const clean = name.replace(/ \(\d+\)/, "");
    let verdict: ManualResult["verdict"]; let msg: string; let ok = false;
    if (expected && clean === expected) { verdict = { kind: "exact_file_name" }; msg = "exact file (file name matches CurseForge)"; ok = true; }
    else if (expected) { verdict = { kind: "mismatch", reason: `expected the file '${expected}' but this is '${clean}'` }; ok = force; msg = (ok ? "accepted anyway: " : "rejected: ") + (verdict as { reason: string }).reason; }
    else { verdict = { kind: "unverified", note: "The exact mod and version can't be confirmed without a CurseForge API key." }; ok = acceptUnverified; msg = (ok ? "accepted unverified. " : "not accepted yet. ") + verdict.note; }
    if (ok) { e.origin = { type: "file", path: f.path }; e.file_name = expected ?? clean; }
    results.push({ path: f.path, file_name: name, index: idx, mod_name: e.name_hint ?? `CurseForge project ${e.cf?.project_id}`, verdict, applied: ok, on_modrinth: false, message: msg });
  }
  return { resolved: next, results, tasks: tasks(next) };
}

const packs = [
  { project_id: 925200, name: "All the Mods 10", summary: "Over 400 mods across tech, magic, exploration and more.", downloads: 18400000, thumbnail: null, url: "https://www.curseforge.com/minecraft/modpacks/all-the-mods-10" },
  { project_id: 2, name: "All the Mods Lite", summary: "A lighter take on All the Mods for smaller machines.", downloads: 920000, thumbnail: null, url: "https://www.curseforge.com/minecraft/modpacks/atm-lite" },
  { project_id: 777, name: "Locked Pack", summary: "The author only allows downloads on CurseForge.", downloads: 1200, thumbnail: null, url: "https://www.curseforge.com/minecraft/modpacks/locked-pack" },
];
const files = (locked: boolean) => [
  { id: 6000123, name: "All the Mods 10-1.2.0", game_versions: ["1.21.1", "NeoForge"], release_type: 1, date: "2026-10-30T10:00:00Z", downloadable: !locked },
  { id: 6000100, name: "All the Mods 10-1.1.0", game_versions: ["1.21.1", "NeoForge"], release_type: 1, date: "2026-09-12T10:00:00Z", downloadable: !locked },
  { id: 6000090, name: "All the Mods 10-1.2.0-beta1", game_versions: ["1.21.1", "NeoForge"], release_type: 2, date: "2026-10-20T10:00:00Z", downloadable: !locked },
];

export async function mockInvoke(cmd: string, a: Record<string, unknown>): Promise<unknown> {
  switch (cmd) {
    case "scan_pack": {
      await sleep(500);
      const resolved = fixture();
      const s: ScanResult = {
        pack: { source: "curse_forge", name: "All the Mods (demo)", version: "1.0", mc_version: "1.20.1", loader: "fabric", loader_version: "0.15.11", mods: resolved.map((r) => r.entry), overrides: [{}, {}, {}] },
        resolved, tasks: tasks(resolved),
        notices: ["Downloaded 1 mod(s) that aren't on Modrinth (0 turned out to be on Modrinth, 1 could not be downloaded)."],
      };
      return s;
    }
    case "apply_manual": await sleep(200); return applyManual(a.resolved as ResolvedMod[], a.files as { path: string; index: number | null }[], !!a.acceptUnverified, !!a.force);
    case "export_pack": {
      await sleep(400);
      const r = a.resolved as ResolvedMod[];
      return { output: String(a.output), matched_exact: 2, matched_fuzzy: 0, bundled: ["create-fabric-0.5.1.jar", "my-local-tweaks.jar"], overrides: 3,
        skipped: tasks(r).map((t) => ({ file_name: t.expected_file_name ?? t.project_name ?? `CurseForge project ${t.project_id} (file ${t.file_id})`, reason: "needs a manual download", link: t.file_url ?? t.project_url })) };
    }
    case "cf_search": {
      await sleep(250);
      const q = String(a.query).toLowerCase();
      return packs.filter((p) => p.name.toLowerCase().includes(q.split(" ")[0]) || q.includes("pack"));
    }
    case "cf_resolve": {
      await sleep(250);
      const input = String(a.input);
      if (/mc-mods/.test(input)) throw new Error("that link is a 'mc-mods' page, not a modpack. Paste a link that contains /minecraft/modpacks/");
      const p = /locked|777/.test(input) ? packs[2] : /lite|^2$/.test(input) ? packs[1] : packs[0];
      return { ...p, files: files(p.project_id === 777), selected_file_id: 6000123 };
    }
    case "cf_fetch": {
      await sleep(600);
      if (a.projectId === 777) return { path: null, manual_url: "https://www.curseforge.com/minecraft/modpacks/locked-pack/files/6000123", message: "The author of 'Locked Pack' only allows downloads on CurseForge. Download it in your browser, then drop the .zip here." };
      return { path: "/Users/demo/cache/modpacks/All-the-Mods-10-1.2.0.zip", manual_url: null, message: "Downloaded All the Mods 10-1.2.0" };
    }
    case "get_settings": return JSON.parse(JSON.stringify(settings));
    case "set_setting": { const s = settings.find((x) => x.key === a.key); if (s) s.value = !!a.value; return null; }
    case "default_downloads_dir": return "/Users/demo/Downloads";
    case "secret_status": return { modrinth: false, curseforge: true };
    case "save_secret": return /^[0-9a-f]{8}-([0-9a-f]{4}-){3}[0-9a-f]{12}$/i.test(String(a.value).trim()) && a.kind === "curseforge"
      ? "This looks like a CurseForge author upload token (the 8-4-4-4-12 form from authors.curseforge.com), which the Core API rejects. Importing needs a Core API key from console.curseforge.com (about 60 characters, starting with $2a$)." : null;
    case "run_diagnostics": await sleep(300); return "Modrinth Importer 0.3.0 on macos aarch64\nModrinth API: reachable\nModrinth token: not set (only needed for publishing)\nCurseForge key: set (OS keychain); length 36, WRONG KIND OF TOKEN? it looks like an author upload token (X-Api-Token), not a Core API key; create one at console.curseforge.com\nCurseForge API: FAILED. CurseForge API returned 403\nKeyless lookup: off (experimental opt-in that works without a CurseForge key)";
    case "debug_log_text": return "# demo log";
    default: return null;
  }
}
