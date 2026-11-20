<script lang="ts">
  import { onMount } from "svelte";
  import AdvancedOptions from "./AdvancedOptions.svelte";
  import { api, pickSavePath } from "./api";

  let report = $state("");
  let checking = $state(false);
  async function runCheck() {
    checking = true;
    report = await api.runDiagnostics().catch((e) => String(e));
    checking = false;
  }
  async function saveLog() {
    const out = await pickSavePath("modrinth-importer-debug.log", "log", "Log");
    if (!out) return;
    try { await api.saveDebugLog(out); msg = "Debug log saved. Attach it to your bug report."; } catch (e) { msg = String(e); }
  }
  async function copyLog() {
    try { await navigator.clipboard.writeText(await api.debugLogText()); msg = "Debug log copied to the clipboard."; }
    catch (e) { msg = String(e); }
  }

  let status = $state({ modrinth: false, curseforge: false });
  let modrinth = $state("");
  let curseforge = $state("");
  let msg = $state("");
  let keyWarning = $state("");

  const refresh = () => api.secretStatus().then((s) => (status = s));
  onMount(refresh);

  async function save(kind: "modrinth" | "curseforge") {
    const v = kind === "modrinth" ? modrinth : curseforge;
    if (!v.trim()) return;
    try {
      const warning = await api.saveSecret(kind, v.trim());
      if (kind === "modrinth") modrinth = ""; else curseforge = "";
      msg = "Saved to your system keychain.";
      keyWarning = warning ?? "";
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
{#if keyWarning}<div class="notice bad" role="alert"><b>Check this key.</b> {keyWarning}</div>{/if}

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
  <p class="muted">Needed for exact, 1:1 imports of CurseForge modpack zips and for migrating CurseForge projects. It must be a <b>Core API key</b> from console.curseforge.com (about 60 characters, starting with $2a$), not the author upload token from authors.curseforge.com. Without a key you can use the experimental keyless lookup below.</p>
  <div class="row">
    <input type="password" bind:value={curseforge} placeholder="API key" style="flex:1" />
    <button class="btn primary" onclick={() => save("curseforge")} disabled={!curseforge}>Save</button>
    {#if status.curseforge}<button class="btn" onclick={() => clear("curseforge")}>Remove</button>{/if}
  </div>
</div>

<AdvancedOptions />

<div class="card">
  <b>Diagnostics</b>
  <p class="muted">Having trouble (for example a CurseForge 403)? Run the check, then save the debug log and attach it to a <a href="https://github.com/Aoof/modrinth-importer/issues/new/choose" target="_blank" rel="noreferrer">bug report</a>. The log lists API calls, statuses and errors. It never contains your keys, and your home folder is shown as <span class="mono">~</span>.</p>
  <div class="row">
    <button class="btn primary" onclick={runCheck} disabled={checking}>{checking ? "Checking…" : "Run check"}</button>
    <button class="btn" onclick={saveLog}>Save debug log…</button>
    <button class="btn" onclick={copyLog}>Copy log</button>
  </div>
  {#if report}<pre class="mono" style="margin:12px 0 0;white-space:pre-wrap;user-select:text">{report}</pre>{/if}
</div>

<p class="muted">Modrinth Importer is an unofficial community tool and is not affiliated with Modrinth or CurseForge.</p>
