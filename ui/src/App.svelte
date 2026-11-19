<script lang="ts">
  import Convert from "./lib/Convert.svelte";
  import Publish from "./lib/Publish.svelte";
  import Settings from "./lib/Settings.svelte";

  type View = "convert" | "publish" | "settings";
  let view = $state<View>("convert");
  const items: [View, string][] = [
    ["convert", "Import pack"],
    ["publish", "Publish project"],
    ["settings", "Settings"],
  ];
</script>

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
