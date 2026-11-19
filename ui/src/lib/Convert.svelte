<script lang="ts">
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";
  import { api, type ProgressEvent, type ScanResult, type ExportReport, type Candidate } from "./api";

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
  let bundleUnmatched = $state(true);
  let downloadRemote = $state(false);
  let report = $state<ExportReport | null>(null);

  const onEvent = (e: ProgressEvent) => {
    if (e.kind === "stage") { stage = e.name; done = 0; total = 0; }
    else if (e.kind === "progress") { done = e.done; total = e.total; }
  };

  onMount(() => {
    let un: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((ev) => {
        const p = ev.payload;
        if (p.type === "over" || p.type === "enter") over = true;
        else if (p.type === "leave") over = false;
        else if (p.type === "drop") {
          over = false;
          if (phase === "idle" || phase === "done") start(p.paths[0]);
        }
      })
      .then((f) => (un = f));
    return () => un?.();
  });

  async function pick(directory: boolean) {
    const r = await open({
      directory,
      multiple: false,
      filters: directory ? undefined : [{ name: "Modpack", extensions: ["zip", "mrpack"] }],
    });
    if (typeof r === "string") start(r);
  }

  async function start(p: string | undefined) {
    if (!p) return;
    path = p;
    error = "";
    phase = "scanning";
    try {
      scan = await api.scan(p, { name, mcVersion, loader }, onEvent);
      needsVersion = false;
      phase = "review";
    } catch (e) {
      error = String(e);
      needsVersion = /Minecraft version/.test(error);
      phase = "idle";
    }
  }

  const label = (m: ScanResult["resolved"][number]) => m.entry.name_hint ?? m.entry.file_name;

  function choose(i: number, c: Candidate | "") {
    if (!scan || c === "") return;
    scan.resolved[i].resolution = { status: "matched", candidate: c, confidence: "fuzzy" };
  }

  let counts = $derived.by(() => {
    const c = { exact: 0, fuzzy: 0, ambiguous: 0, unmatched: 0 };
    for (const r of scan?.resolved ?? []) {
      if (r.resolution.status === "matched") c[r.resolution.confidence]++;
      else c[r.resolution.status]++;
    }
    return c;
  });
  let rows = $derived(
    (scan?.resolved ?? []).map((r, i) => ({ r, i })).filter(({ r }) => filter === "all" || r.resolution.status !== "matched" || r.resolution.confidence === "fuzzy"),
  );

  async function doExport() {
    if (!scan) return;
    const out = await save({
      defaultPath: `${scan.pack.name.replace(/[^\w\- ]+/g, "_")}.mrpack`,
      filters: [{ name: "Modrinth modpack", extensions: ["mrpack"] }],
    });
    if (!out) return;
    phase = "exporting";
    error = "";
    try {
      report = await api.exportPack(scan, out, { bundleUnmatched, downloadRemote }, onEvent);
      phase = "done";
    } catch (e) {
      error = String(e);
      phase = "review";
    }
  }

  function reset() { phase = "idle"; scan = null; report = null; error = ""; path = ""; }
</script>

<h1>Import a pack to Modrinth</h1>
<p class="lede">Convert a CurseForge, Prism/MultiMC, ATLauncher, FTB, Technic or packwiz pack, or a mods folder, into a Modrinth <span class="mono">.mrpack</span>.</p>

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
{:else if phase === "scanning" || phase === "exporting"}
  <div class="card">
    <div>{stage || "Working…"}</div>
    <div class="bar"><i style="width:{total ? (done / total) * 100 : 8}%"></i></div>
    {#if total}<div class="muted" style="margin-top:6px">{done} / {total}</div>{/if}
  </div>
{:else if phase === "review" && scan}
  <div class="card">
    <div style="font-weight:600;font-size:15px">{scan.pack.name} <span class="muted">{scan.pack.version}</span></div>
    <div class="muted">{scan.pack.source} · Minecraft {scan.pack.mc_version} · {scan.pack.loader}{scan.pack.loader_version ? " " + scan.pack.loader_version : ""} · {scan.pack.overrides.length} config files</div>
    <div class="stats" style="margin-top:12px">
      <div class="stat"><b>{counts.exact}</b><span>matched by file</span></div>
      <div class="stat"><b>{counts.fuzzy}</b><span>matched by name</span></div>
      <div class="stat"><b>{counts.ambiguous}</b><span>need a choice</span></div>
      <div class="stat"><b>{counts.unmatched}</b><span>not on Modrinth</span></div>
    </div>
    <div class="row">
      <label class="check"><input type="checkbox" bind:checked={bundleUnmatched} /> Bundle mods Modrinth doesn't host</label>
      <label class="check" title="Downloads the file from its original source. Check the mod's license allows redistribution."><input type="checkbox" bind:checked={downloadRemote} /> Also download remote-only mods</label>
      <span style="flex:1"></span>
      <select bind:value={filter} style="width:auto"><option value="all">All mods</option><option value="attention">Needs attention</option></select>
    </div>
  </div>

  <div class="tablewrap">
    <table>
      <thead><tr><th>Mod</th><th>Result</th><th>Modrinth version</th></tr></thead>
      <tbody>
        {#each rows as { r, i } (i)}
          <tr>
            <td><div>{label(r)}</div><div class="muted mono">{r.entry.file_name}</div></td>
            <td>
              {#if r.resolution.status === "matched"}
                <span class="chip" class:ok={r.resolution.confidence === "exact"} class:warn={r.resolution.confidence === "fuzzy"}>{r.resolution.confidence === "exact" ? "Exact" : "By name"}</span>
              {:else if r.resolution.status === "ambiguous"}
                <span class="chip warn">Choose</span>
              {:else if r.entry.origin && bundleUnmatched}
                <span class="chip dim">Bundled</span>
              {:else}
                <span class="chip bad">Skipped</span>
              {/if}
            </td>
            <td>
              {#if r.resolution.status === "matched"}
                {r.resolution.candidate.title} <span class="muted">{r.resolution.candidate.version_number}</span>
              {:else if r.resolution.status === "ambiguous"}
                <select onchange={(e) => choose(i, r.resolution.status === "ambiguous" ? (r.resolution.candidates[+e.currentTarget.value] ?? "") : "")}>
                  <option value="">Pick a match…</option>
                  {#each r.resolution.candidates as c, ci}<option value={ci}>{c.title} {c.version_number}</option>{/each}
                </select>
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
    <button class="btn primary" onclick={doExport}>Export .mrpack…</button>
    <button class="btn" onclick={reset}>Start over</button>
  </div>
{:else if phase === "done" && report}
  <div class="notice ok">
    Saved <span class="mono">{report.output}</span><br />
    {report.matched_exact} matched by file, {report.matched_fuzzy} by name, {report.bundled.length} bundled, {report.skipped.length} skipped, {report.overrides} config files.
  </div>
  {#if report.skipped.length}
    <div class="card"><b>Skipped</b>
      {#each report.skipped as s}<div class="muted" style="margin-top:4px"><span class="mono">{s.file_name}</span> — {s.reason}</div>{/each}
    </div>
  {/if}
  <div class="row">
    <button class="btn primary" onclick={() => api.openPath(report!.output).catch((e) => (error = String(e)))}>Open in Modrinth App</button>
    <button class="btn" onclick={reset}>Import another</button>
  </div>
{/if}
