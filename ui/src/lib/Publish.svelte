<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, type ProgressEvent, type PublishReport, type PublishRequest } from "./api";

  let source = $state<PublishRequest["source"]>("jars");
  let value = $state("");
  let f = $state({
    versionNumber: "1.0.0", slug: "", title: "", summary: "", body: "", license: "LicenseRef-All-Rights-Reserved",
    categories: "", clientSide: "optional", serverSide: "optional", gameVersions: "", loaders: "", existing: false,
  });
  let busy = $state(false);
  let error = $state("");
  let stage = $state("");
  let report = $state<PublishReport | null>(null);

  const hints = {
    jars: "One jar path per line",
    github: "owner/repo",
    curseforge: "Numeric project id",
  } as const;

  async function pickJars() {
    const r = await open({ multiple: true, filters: [{ name: "Mod jar", extensions: ["jar"] }] });
    if (Array.isArray(r)) value = r.join("\n");
  }

  async function run(dryRun: boolean) {
    busy = true; error = ""; report = null; stage = "";
    const onEvent = (e: ProgressEvent) => { if (e.kind === "stage") stage = e.name; };
    try {
      report = await api.publish({ source, value, ...f, dryRun }, onEvent);
    } catch (e) { error = String(e); }
    busy = false;
  }
</script>

<h1>Publish a project</h1>
<p class="lede">Create a Modrinth project and upload its versions. New projects are created as drafts so you can review them on the site before submitting.</p>

{#if error}<div class="notice bad">{error}</div>{/if}

<div class="tabs" role="tablist">
  {#each ["jars", "github", "curseforge"] as s}
    <button class="tab" role="tab" aria-selected={source === s} onclick={() => { source = s as typeof source; value = ""; }}>
      {s === "jars" ? "Local jars" : s === "github" ? "GitHub releases" : "CurseForge project"}
    </button>
  {/each}
</div>

<div class="card">
  <label class="f">Source
    {#if source === "jars"}
      <div class="row"><textarea bind:value placeholder={hints[source]} style="flex:1"></textarea><button class="btn" onclick={pickJars}>Browse…</button></div>
    {:else}
      <input type="text" bind:value placeholder={hints[source]} />
    {/if}
  </label>
  {#if source === "jars"}
    <label class="f" style="margin-top:10px">Version number<input type="text" bind:value={f.versionNumber} /></label>
  {/if}
</div>

<div class="card">
  <div class="grid2">
    <label class="f">Title<input type="text" bind:value={f.title} placeholder={source === "curseforge" ? "Taken from CurseForge" : ""} /></label>
    <label class="f">Slug<input type="text" bind:value={f.slug} placeholder="my-mod" /></label>
    <label class="f" style="grid-column:1/-1">Summary (max 256 chars)<input type="text" bind:value={f.summary} maxlength="256" /></label>
    <label class="f" style="grid-column:1/-1">Description (Markdown)<textarea bind:value={f.body}></textarea></label>
    <label class="f">Client<select bind:value={f.clientSide}><option>required</option><option>optional</option><option>unsupported</option></select></label>
    <label class="f">Server<select bind:value={f.serverSide}><option>required</option><option>optional</option><option>unsupported</option></select></label>
    <label class="f">License (SPDX id)<input type="text" bind:value={f.license} /></label>
    <label class="f">Categories (comma separated)<input type="text" bind:value={f.categories} placeholder="optimization, utility" /></label>
    <label class="f">Minecraft versions<input type="text" bind:value={f.gameVersions} placeholder="1.20.1, 1.20.4" /></label>
    <label class="f">Loaders<input type="text" bind:value={f.loaders} placeholder="fabric, forge" /></label>
  </div>
  <label class="check" style="margin-top:12px"><input type="checkbox" bind:checked={f.existing} /> Project already exists on Modrinth — only upload new versions</label>
</div>

<div class="row">
  <button class="btn" disabled={busy || !value} onclick={() => run(true)}>Dry run</button>
  <button class="btn primary" disabled={busy || !value} onclick={() => run(false)}>Publish</button>
  {#if busy}<span class="muted">{stage || "Working…"}</span>{/if}
</div>

{#if report}
  <div class="notice ok" style="margin-top:14px">
    {report.dry_run ? "Dry run: would upload" : "Uploaded"} {report.uploaded.length} version(s){report.skipped.length ? `, skipped ${report.skipped.length}` : ""}.
    {#if report.project_id && !report.dry_run}<br />Project id <span class="mono">{report.project_id}</span> — open it on modrinth.com to review and submit.{/if}
    {#each report.skipped as s}<div class="muted">{s}</div>{/each}
  </div>
{/if}
