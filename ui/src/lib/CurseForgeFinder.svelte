<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ModpackDetail, type ModpackHit, type ProgressEvent } from "./api";

  let { onimport }: { onimport: (path: string) => void } = $props();

  let hasKey = $state(true);
  let query = $state("");
  let hits = $state<ModpackHit[]>([]);
  let detail = $state<ModpackDetail | null>(null);
  let fileId = $state(0);
  let busy = $state(false);
  let fetching = $state(false);
  let pct = $state(0);
  let error = $state("");
  let manual = $state<{ url: string; message: string } | null>(null);

  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0; // ignore answers to searches that were already replaced

  onMount(() => { api.secretStatus().then((s) => (hasKey = s.curseforge)).catch(() => {}); });

  const isLink = (q: string) => /^(https?:\/\/|curseforge:\/\/|cf:)/i.test(q.trim()) || /^\d+$/.test(q.trim());
  const open = (url: string) => api.openLink(url).catch((e) => (error = String(e)));
  const downloads = (n: number) => n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}K` : String(n);
  const release = (t: number) => (t === 1 ? "Release" : t === 2 ? "Beta" : "Alpha");

  function changed() {
    error = ""; manual = null; detail = null;
    clearTimeout(timer);
    const q = query.trim();
    if (q.length < 2 || isLink(q)) { hits = []; return; }
    timer = setTimeout(() => search(q), 350);
  }

  async function search(q: string) {
    const me = ++seq;
    busy = true;
    try {
      const r = await api.cfSearch(q);
      if (me === seq) hits = r;
    } catch (e) { if (me === seq) { hits = []; error = String(e); } }
    if (me === seq) busy = false;
  }

  async function lookup(input: string) {
    const me = ++seq;
    busy = true; error = ""; manual = null;
    try {
      const d = await api.cfResolve(input);
      if (me !== seq) return;
      detail = d; fileId = d.selected_file_id; hits = [];
    } catch (e) { if (me === seq) error = String(e); }
    if (me === seq) busy = false;
  }

  function submit(e: Event) {
    e.preventDefault();
    clearTimeout(timer);
    const q = query.trim();
    if (!q) return;
    if (isLink(q)) lookup(q); else search(q);
  }

  const onEvent = (e: ProgressEvent) => { if (e.kind === "progress" && e.total) pct = Math.min(100, (e.done / e.total) * 100); };

  async function importIt() {
    if (!detail) return;
    fetching = true; pct = 0; error = ""; manual = null;
    try {
      const r = await api.cfFetch(detail.project_id, fileId, onEvent);
      if (r.path) { onimport(r.path); }
      else if (r.manual_url) manual = { url: r.manual_url, message: r.message };
    } catch (e) { error = String(e); }
    fetching = false;
  }
</script>

<section class="card finder" aria-labelledby="cf-title">
  <h3 id="cf-title">Find a modpack on CurseForge</h3>
  {#if !hasKey}
    <p class="hint">Paste a modpack link or search by name to import without leaving the app. This needs a CurseForge Core API key: add one in <b>Settings</b>. Or download the pack's .zip from CurseForge and drop it above.</p>
  {:else}
    <p class="hint">Search by name, or paste a link such as <span class="mono">curseforge.com/minecraft/modpacks/…</span>, a project ID, or a <span class="mono">curseforge://</span> link.</p>
    <form class="row" onsubmit={submit}>
      <input type="text" bind:value={query} oninput={changed} onpaste={() => setTimeout(() => isLink(query) && lookup(query), 0)}
        placeholder="e.g. all the mods, or https://www.curseforge.com/minecraft/modpacks/…" style="flex:1" aria-label="Search CurseForge modpacks or paste a link" />
      <button class="btn" type="submit" disabled={busy || !query.trim()}>{busy ? "Looking…" : isLink(query) ? "Look up" : "Search"}</button>
    </form>

    {#if error}<div class="notice bad" style="margin:10px 0 0;user-select:text">{error}</div>{/if}

    {#if hits.length && !detail}
      <div class="hits">
        {#each hits as h (h.project_id)}
          <button class="hit" onclick={() => lookup(String(h.project_id))}>
            {#if h.thumbnail}<img class="thumb" src={h.thumbnail} alt="" />{:else}<span class="thumb"></span>{/if}
            <span><span class="n">{h.name}</span><br /><span class="s">{h.summary}</span></span>
            <span class="dl">{downloads(h.downloads)} downloads</span>
          </button>
        {/each}
      </div>
    {:else if !busy && !detail && query.trim().length >= 2 && !isLink(query) && !error && hits.length === 0}
      <p class="hint" style="margin-top:10px">No modpacks found. Press Enter to search again, or paste a link.</p>
    {/if}

    {#if detail}
      <div class="pick">
        <div class="row" style="justify-content:space-between;align-items:flex-start">
          <div><b>{detail.name}</b> <span class="muted">· {downloads(detail.downloads)} downloads</span>
            <div class="muted" style="font-size:12px">{detail.summary}</div></div>
          <button class="btn link" onclick={() => open(detail!.url)}>Open on CurseForge ↗</button>
        </div>
        <label class="f">Version
          <select bind:value={fileId} disabled={fetching}>
            {#each detail.files as f (f.id)}
              <option value={f.id}>{f.name} · {f.game_versions.join(", ")} · {release(f.release_type)}{f.date ? " · " + f.date.slice(0, 10) : ""}{f.downloadable ? "" : " · no direct download"}</option>
            {/each}
          </select>
        </label>
        <div class="row">
          <button class="btn primary" onclick={importIt} disabled={fetching}>{fetching ? "Downloading…" : "Import this version"}</button>
          <button class="btn" onclick={() => { detail = null; manual = null; }} disabled={fetching}>Back</button>
          {#if fetching}<div class="bar" style="flex:1;margin:0"><i style="width:{pct || 8}%"></i></div>{/if}
        </div>
        {#if manual}
          <div class="callout" role="alert" style="margin:0">
            <p style="margin:0 0 8px">{manual.message}</p>
            <button class="btn primary" onclick={() => open(manual!.url)}>Open the download page ↗</button>
          </div>
        {/if}
      </div>
    {/if}
  {/if}
</section>
