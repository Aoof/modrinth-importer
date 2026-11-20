<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";
  import AdvancedOptions from "./AdvancedOptions.svelte";
  import CurseForgeFinder from "./CurseForgeFinder.svelte";
  import {
    api, isTauri, pickFolder, pickJars, pickPack, pickSavePath,
    type ExportReport, type ManualResult, type ManualTask, type ProgressEvent, type ScanResult, type Setting,
  } from "./api";

  type Phase = "idle" | "scanning" | "review" | "exporting" | "done";
  let phase = $state<Phase>("idle");
  let over = $state(false);
  let error = $state("");
  let stage = $state("");
  let done = $state(0);
  let total = $state(0);

  let path = $state("");
  let name = $state("");
  let mcVersion = $state("");
  let loader = $state("");
  let needsVersion = $state(false);

  let scan = $state<ScanResult | null>(null);
  let filter = $state<"all" | "attention">("all");
  let report = $state<ExportReport | null>(null);
  let advanced = $state<Setting[]>([]);
  let manualResults = $state<ManualResult[]>([]);
  let downloads = $state<string | null>(null);
  let busyManual = $state(false);

  const onEvent = (e: ProgressEvent) => {
    if (e.kind === "stage") { stage = e.name; done = 0; total = 0; }
    else if (e.kind === "progress") { done = e.done; total = e.total; }
  };

  onMount(() => {
    api.defaultDownloadsDir().then((d) => (downloads = d)).catch(() => {});
    if (!isTauri) return;
    let un: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((ev) => {
        const p = ev.payload;
        if (p.type === "over" || p.type === "enter") over = true;
        else if (p.type === "leave") over = false;
        else if (p.type === "drop") {
          over = false;
          if (phase === "idle" || phase === "done") start(p.paths[0]);
          else if (phase === "review") addFiles(p.paths.map((x) => ({ path: x })));
        }
      })
      .then((f) => (un = f));
    return () => un?.();
  });

  async function pick(directory: boolean) {
    const r = await pickPack(directory);
    if (r) start(r);
  }

  async function start(p: string | undefined) {
    if (!p) return;
    path = p;
    error = "";
    manualResults = [];
    phase = "scanning";
    try {
      advanced = await api.getSettings();
      scan = await api.scan(p, { name, mcVersion, loader }, onEvent);
      needsVersion = false;
      phase = "review";
    } catch (e) {
      error = String(e);
      needsVersion = /Minecraft version/.test(error);
      phase = "idle";
    }
  }

  const label = (m: ScanResult["resolved"][number]) => m.entry.name_hint ?? m.entry.expected_file_name ?? (m.entry.cf && m.entry.file_name.startsWith("cf-") ? `CurseForge project ${m.entry.cf.project_id}` : m.entry.file_name);
  const open = (url: string) => api.openLink(url).catch((e) => (error = String(e)));
  let taskAt = $derived(new Map((scan?.tasks ?? []).map((t) => [t.index, t])));

  function choose(i: number, ci: string) {
    const r = scan?.resolved[i];
    if (!r || r.resolution.status !== "ambiguous" || ci === "") return;
    scan!.resolved[i].resolution = { status: "matched", candidate: r.resolution.candidates[+ci], confidence: "fuzzy" };
  }

  let counts = $derived.by(() => {
    const c = { exact: 0, fuzzy: 0, bundled: 0, manual: scan?.tasks.length ?? 0, ambiguous: 0 };
    for (const [i, r] of (scan?.resolved ?? []).entries()) {
      if (r.resolution.status === "matched") c[r.resolution.confidence]++;
      else if (r.resolution.status === "ambiguous") c.ambiguous++;
      else if (r.entry.origin) c.bundled++;
      else if (!taskAt.has(i)) { /* skipped for another reason */ }
    }
    return c;
  });

  // What needs the person's attention comes first.
  const rank = (i: number, r: ScanResult["resolved"][number]) =>
    taskAt.has(i) ? 0 : r.resolution.status === "ambiguous" || (r.resolution.status === "matched" && r.resolution.confidence === "fuzzy") ? 1 : 2;
  let rows = $derived(
    (scan?.resolved ?? [])
      .map((r, i) => ({ r, i, k: rank(i, r) }))
      .filter(({ k }) => filter === "all" || k < 2)
      .sort((a, b) => a.k - b.k || a.i - b.i),
  );

  // ---- manual files ------------------------------------------------------
  async function addFiles(files: { path: string; index?: number }[], o: { acceptUnverified?: boolean; force?: boolean } = {}) {
    if (!scan || !files.length) return;
    busyManual = true;
    error = "";
    try {
      const b = await api.applyManual(scan.resolved, files, o);
      scan.resolved = b.resolved;
      scan.tasks = b.tasks;
      // keep earlier results that are still relevant, newest first
      const fresh = new Set(b.results.map((r) => r.path + ":" + r.index));
      manualResults = [...b.results, ...manualResults.filter((r) => !fresh.has(r.path + ":" + r.index) && !r.applied)].slice(0, 30);
    } catch (e) { error = String(e); }
    busyManual = false;
  }
  async function chooseFor(t: ManualTask) {
    const [f] = await pickJars(false);
    if (f) addFiles([{ path: f, index: t.index }]);
  }
  async function chooseFiles() { const fs = await pickJars(true); addFiles(fs.map((p) => ({ path: p }))); }
  async function chooseFolder() { const d = await pickFolder(); if (d) addFiles([{ path: d }]); }

  const exactnessText = (t: ManualTask) =>
    t.exactness === "hash" ? "exact SHA-1 check"
    : t.exactness === "file_name" ? "exact file-name check"
    : "can't be verified without a CurseForge API key";

  // ---- export --------------------------------------------------------------
  async function doExport() {
    if (!scan) return;
    const out = await pickSavePath(`${scan.pack.name.replace(/[^\w\- ]+/g, "_")}.mrpack`, "mrpack", "Modrinth modpack");
    if (!out) return;
    phase = "exporting";
    error = "";
    try {
      report = await api.exportPack(scan, out, {}, onEvent);
      phase = "done";
    } catch (e) {
      error = String(e);
      phase = "review";
    }
  }

  function reset() { api.discardScan().catch(() => {}); phase = "idle"; scan = null; report = null; error = ""; manualResults = []; }
  function backToOptions() { api.discardScan().catch(() => {}); phase = "idle"; scan = null; manualResults = []; }
  const shortName: Record<string, string> = { download: "download", bundle: "bundle", "match-by-name": "match by name", keyless: "keyless lookup" };
  const summary = $derived(advanced.map((s) => `${shortName[s.key] ?? s.key} ${s.value ? "on" : "off"}`).join(" · "));
</script>

<h1>Import a pack to Modrinth</h1>
<p class="lede">Convert a CurseForge, Prism/MultiMC, ATLauncher, FTB, Technic or packwiz pack, or a mods folder, into a Modrinth <span class="mono">.mrpack</span>. Mods that match a file on Modrinth exactly use it; the rest are downloaded and bundled.</p>

{#if error}
  <div class="notice bad" style="user-select:text">
    <div style="white-space:pre-wrap">{error}</div>
    {#if /CurseForge|Modrinth API|network/.test(error)}
      <div style="margin-top:6px">Open <b>Settings → Diagnostics</b> to check your keys and save a debug log for a bug report.</div>
    {/if}
  </div>
{/if}

{#if phase === "idle"}
  <div class="drop" class:over>
    <strong>Drop a pack file or instance folder here</strong>
    <div class="row" style="justify-content:center;margin-top:12px">
      <button class="btn primary" onclick={() => pick(false)}>Choose .zip…</button>
      <button class="btn" onclick={() => pick(true)}>Choose folder…</button>
    </div>
  </div>
  <div style="margin-top:14px"><CurseForgeFinder onimport={(p) => start(p)} /></div>
  <div class="card" style="margin-top:14px">
    <div class="grid2">
      <label class="f">Pack name (optional)<input type="text" bind:value={name} placeholder="Detected from the pack" /></label>
      <label class="f">Loader (optional)
        <select bind:value={loader}>
          <option value="">Detect</option><option>fabric</option><option>forge</option><option>neoforge</option><option>quilt</option>
        </select>
      </label>
      <label class="f">Minecraft version {needsVersion ? "(required for this folder)" : "(optional)"}
        <input type="text" bind:value={mcVersion} placeholder="e.g. 1.20.1" />
      </label>
    </div>
    {#if needsVersion && path}
      <div class="row" style="margin-top:12px"><button class="btn primary" onclick={() => start(path)} disabled={!mcVersion}>Retry</button></div>
    {/if}
  </div>
  <AdvancedOptions onchange={(s) => (advanced = s)} />
{:else if phase === "scanning" || phase === "exporting"}
  <div class="card">
    <div>{stage || "Working…"}</div>
    <div class="bar"><i style="width:{total ? (done / total) * 100 : 8}%"></i></div>
    {#if total}<div class="muted" style="margin-top:6px">{done} / {total}</div>{/if}
  </div>
{:else if phase === "review" && scan}
  {#each scan.notices as n}<div class="notice ok">{n}</div>{/each}

  <div class="card">
    <div style="font-weight:600;font-size:15px">{scan.pack.name} <span class="muted">{scan.pack.version}</span></div>
    <div class="muted">{scan.pack.source} · Minecraft {scan.pack.mc_version} · {scan.pack.loader}{scan.pack.loader_version ? " " + scan.pack.loader_version : ""} · {scan.pack.overrides.length} config files</div>
    <div class="stats" style="margin-top:12px">
      <div class="stat"><b>{counts.exact}</b><span>exact on Modrinth</span></div>
      <div class="stat"><b>{counts.bundled}</b><span>bundled in the pack</span></div>
      <div class="stat"><b>{counts.manual}</b><span>need your file</span></div>
      {#if counts.fuzzy || counts.ambiguous}<div class="stat"><b>{counts.fuzzy + counts.ambiguous}</b><span>matched by name</span></div>{/if}
    </div>
    <div class="row">
      <span class="muted" style="font-size:12px">Advanced: {summary || "defaults"}</span>
      <button class="btn link" onclick={backToOptions}>Change &amp; re-scan</button>
      <span style="flex:1"></span>
      <select bind:value={filter} style="width:auto"><option value="all">All mods</option><option value="attention">Needs attention</option></select>
    </div>
  </div>

  {#if scan.tasks.length}
    <div class="callout" role="alert">
      <h3>{scan.tasks.length} mod{scan.tasks.length === 1 ? "" : "s"} need a file from you</h3>
      <p>These aren't an exact match on Modrinth and couldn't be downloaded automatically. Download the <b>exact file</b> from CurseForge using the links below, then drop it on this window (or choose it). The app checks it is the file the pack asks for before using it; files not on Modrinth are bundled in the pack.</p>
      <div class="row">
        {#if downloads}<button class="btn primary" disabled={busyManual} onclick={() => addFiles([{ path: downloads! }])}>Scan Downloads folder</button>{/if}
        <button class="btn" disabled={busyManual} onclick={chooseFiles}>Choose files…</button>
        <button class="btn" disabled={busyManual} onclick={chooseFolder}>Choose folder…</button>
        {#if busyManual}<span class="muted">Checking…</span>{/if}
      </div>
    </div>
  {/if}

  {#if manualResults.length}
    <div class="card">
      <b>Files you added</b>
      {#each manualResults as r (r.path + ":" + r.index)}
        <div class="result">
          <span class="tag" class:ok={r.applied} class:warn={!r.applied && r.verdict?.kind === "unverified"} class:bad={!r.applied && r.verdict?.kind !== "unverified"}>{r.applied ? "Added" : r.verdict?.kind === "unverified" ? "Unverified" : r.verdict ? "Rejected" : "No match"}</span>
          <span class="mono">{r.file_name}</span>
          {#if r.mod_name}<span class="muted">for {r.mod_name}</span>{/if}
          <span class="muted" style="flex-basis:100%">{r.message}{r.applied && r.on_modrinth ? " Found on Modrinth: the pack will download it from there." : ""}</span>
          {#if !r.applied && r.index !== null && r.verdict?.kind === "unverified"}
            <button class="btn small" onclick={() => addFiles([{ path: r.path, index: r.index! }], { acceptUnverified: true })}>Use anyway</button>
          {:else if !r.applied && r.index !== null && r.verdict?.kind === "mismatch"}
            <button class="btn small" onclick={() => addFiles([{ path: r.path, index: r.index! }], { force: true })}>Use anyway (not the exact file)</button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <div class="tablewrap">
    <table>
      <thead><tr><th>Mod</th><th>Result</th><th>Details</th></tr></thead>
      <tbody>
        {#each rows as { r, i } (i)}
          {@const t = taskAt.get(i)}
          <tr>
            <td><div>{label(r)}</div><div class="muted mono">{r.entry.expected_file_name ?? r.entry.file_name}</div></td>
            <td>
              {#if r.resolution.status === "matched"}
                <span class="chip" class:ok={r.resolution.confidence === "exact"} class:warn={r.resolution.confidence === "fuzzy"}>{r.resolution.confidence === "exact" ? "Exact" : "By name"}</span>
              {:else if r.resolution.status === "ambiguous"}
                <span class="chip warn">Choose</span>
              {:else if t}
                <span class="chip warn strong">Needs your file</span>
              {:else if r.entry.origin}
                <span class="chip dim">Bundled</span>
              {:else}
                <span class="chip bad">Skipped</span>
              {/if}
            </td>
            <td>
              {#if r.resolution.status === "matched"}
                {r.resolution.candidate.title} <span class="muted">{r.resolution.candidate.version_number}</span>
                <div class="muted">{r.resolution.confidence === "exact" ? "Identical file on Modrinth" : "A similar version, not the same file"}</div>
              {:else if r.resolution.status === "ambiguous"}
                <select onchange={(e) => choose(i, e.currentTarget.value)}>
                  <option value="">Pick a match…</option>
                  {#each r.resolution.candidates as c, ci}<option value={ci}>{c.title} {c.version_number}</option>{/each}
                </select>
              {:else if t}
                <span class="muted">{r.resolution.reason}</span>
                <div class="need">
                  Expects <b class="mono">{t.expected_file_name ?? "an unknown file"}</b> · {exactnessText(t)}
                  <div class="links">
                    {#if t.file_url}<button class="btn link" onclick={() => open(t.file_url!)}>Open file page ↗</button>{/if}
                    <button class="btn link" onclick={() => open(t.project_url)}>Open project page ↗</button>
                    <button class="btn small" disabled={busyManual} onclick={() => chooseFor(t)}>Choose file…</button>
                  </div>
                </div>
              {:else if r.entry.origin}
                <span class="muted">{r.entry.cf ? "Downloaded from CurseForge or supplied by you, bundled in the pack" : "Not on Modrinth; bundled in the pack"}</span>
              {:else}
                <span class="muted">{r.resolution.reason}</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  <div class="row" style="margin-top:14px">
    <button class="btn primary" onclick={doExport}>{scan.tasks.length ? `Export without ${scan.tasks.length} mod${scan.tasks.length === 1 ? "" : "s"}…` : "Export .mrpack…"}</button>
    <button class="btn" onclick={reset}>Start over</button>
    {#if scan.tasks.length}<span class="muted">Left-out mods are listed in the report with download links.</span>{/if}
  </div>
{:else if phase === "done" && report}
  <div class="notice ok">
    Saved <span class="mono">{report.output}</span><br />
    {report.matched_exact} exact on Modrinth, {report.matched_fuzzy} by name, {report.bundled.length} bundled, {report.skipped.length} left out, {report.overrides} config files.
  </div>
  {#if report.skipped.length}
    <div class="card"><b>Left out — add these by hand</b>
      {#each report.skipped as s}
        <div class="result"><span class="mono">{s.file_name}</span><span class="muted">{s.reason}</span>
          {#if s.link}<button class="btn link" onclick={() => open(s.link!)}>Open download page ↗</button>{/if}</div>
      {/each}
    </div>
  {/if}
  <div class="row">
    <button class="btn primary" onclick={() => api.openPath(report!.output).catch((e) => (error = String(e)))}>Open in Modrinth App</button>
    <button class="btn" onclick={reset}>Import another</button>
  </div>
{/if}
