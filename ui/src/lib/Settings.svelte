<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./api";

  let status = $state({ modrinth: false, curseforge: false });
  let modrinth = $state("");
  let curseforge = $state("");
  let msg = $state("");

  const refresh = () => api.secretStatus().then((s) => (status = s));
  onMount(refresh);

  async function save(kind: "modrinth" | "curseforge") {
    const v = kind === "modrinth" ? modrinth : curseforge;
    if (!v.trim()) return;
    try {
      await api.saveSecret(kind, v.trim());
      if (kind === "modrinth") modrinth = ""; else curseforge = "";
      msg = "Saved to your system keychain.";
    } catch (e) { msg = String(e); }
    refresh();
  }
  async function clear(kind: "modrinth" | "curseforge") {
    await api.clearSecret(kind).catch(() => {});
    refresh();
  }
</script>

<h1>Settings</h1>
<p class="lede">Credentials are stored in your operating system's keychain, never in plain files.</p>
{#if msg}<div class="notice ok">{msg}</div>{/if}

<div class="card">
  <div class="row"><b>Modrinth token</b><span class="chip" class:ok={status.modrinth} class:dim={!status.modrinth}>{status.modrinth ? "Set" : "Not set"}</span></div>
  <p class="muted">Needed to publish. Create a personal access token at modrinth.com/settings/pats with the create-project and create-version scopes.</p>
  <div class="row">
    <input type="password" bind:value={modrinth} placeholder="mrp_…" style="flex:1" />
    <button class="btn primary" onclick={() => save("modrinth")} disabled={!modrinth}>Save</button>
    {#if status.modrinth}<button class="btn" onclick={() => clear("modrinth")}>Remove</button>{/if}
  </div>
</div>

<div class="card">
  <div class="row"><b>CurseForge API key</b><span class="chip" class:ok={status.curseforge} class:dim={!status.curseforge}>{status.curseforge ? "Set" : "Not set"}</span></div>
  <p class="muted">Needed to read CurseForge modpack zips and to migrate CurseForge projects. Get a free key at console.curseforge.com.</p>
  <div class="row">
    <input type="password" bind:value={curseforge} placeholder="API key" style="flex:1" />
    <button class="btn primary" onclick={() => save("curseforge")} disabled={!curseforge}>Save</button>
    {#if status.curseforge}<button class="btn" onclick={() => clear("curseforge")}>Remove</button>{/if}
  </div>
</div>

<p class="muted">Modrinth Importer is an unofficial community tool and is not affiliated with Modrinth or CurseForge.</p>
