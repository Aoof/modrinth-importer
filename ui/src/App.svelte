<script lang="ts">
  import Convert from "./lib/Convert.svelte";
  import Publish from "./lib/Publish.svelte";
  import Settings from "./lib/Settings.svelte";
  import { onMount } from "svelte";
  import { api, type UpdateInfo } from "./lib/api";

  // On launch, offer a newer release. Optional: "Later" keeps working as normal.
  let update = $state<UpdateInfo | null>(null);
  let stage = $state("");
  let updating = $state(false);
  let updateError = $state("");
  onMount(() => { api.checkUpdate().then((u) => (update = u)).catch(() => {}); });
  async function install() {
    updating = true; updateError = ""; stage = "Starting…";
    try {
      await api.installUpdate((e) => { if (e.kind === "stage") stage = e.name; });
      stage = "Restarting…";
    } catch (e) { updateError = String(e); updating = false; }
  }

  type View = "convert" | "publish" | "settings";
  let view = $state<View>("convert");
  const items: [View, string][] = [
    ["convert", "Import pack"],
    ["publish", "Publish project"],
    ["settings", "Settings"],
  ];
</script>

{#if update}
  <div class="notice ok" role="alertdialog" aria-label="Update available" style="position:fixed;right:16px;bottom:16px;z-index:50;max-width:440px;display:flex;gap:12px;align-items:center;flex-wrap:wrap;box-shadow:0 6px 24px rgba(0,0,0,.35)">
    <span style="flex:1 1 100%">
      <b>Version {update.version} is available</b> (you have {update.current}).
      {#if updating}{stage}{:else}Restart now to update? Your settings and credentials are kept.{/if}
      {#if updateError}<span style="color:var(--bad, #c0392b)"> {updateError}</span>{/if}
    </span>
    <button class="btn primary" onclick={install} disabled={updating}>{updating ? "Updating…" : "Update and restart"}</button>
    <button class="btn" onclick={() => (update = null)} disabled={updating}>Later</button>
  </div>
{/if}
<div class="shell">
  <nav class="side">
    <div class="brand">Modrinth Importer<small>Unofficial community tool</small></div>
    {#each items as [id, label]}
      <button class="nav" aria-current={view === id ? "page" : undefined} onclick={() => (view = id)}>{label}</button>
    {/each}
  </nav>
  <main class="main">
    <!-- Views stay mounted so an in-progress import survives tab switches. -->
    <div class="page" hidden={view !== "convert"}><Convert /></div>
    <div class="page" hidden={view !== "publish"}><Publish /></div>
    <div class="page" hidden={view !== "settings"}><Settings /></div>
  </main>
</div>
