<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Setting } from "./api";

  let { onchange }: { onchange?: (s: Setting[]) => void } = $props();
  let settings = $state<Setting[]>([]);
  let error = $state("");

  onMount(async () => {
    try { settings = await api.getSettings(); } catch (e) { error = String(e); }
  });

  async function toggle(s: Setting, value: boolean) {
    s.value = value;
    try { await api.setSetting(s.key, value); onchange?.(settings); }
    catch (e) { s.value = !value; error = String(e); }
  }
</script>

<section class="card adv" aria-labelledby="adv-title">
  <h3 id="adv-title">Advanced options</h3>
  <p class="hint">Saved for both the app and the command line (<span class="mono">mrimport config show</span>). The defaults suit a modpack that isn't on Modrinth.</p>
  {#if error}<div class="notice bad">{error}</div>{/if}
  {#each settings as s (s.key)}
    <label class="opt">
      <input type="checkbox" checked={s.value} onchange={(e) => toggle(s, e.currentTarget.checked)} />
      <span class="t">{s.title}</span>
      <span class="d">{s.description}</span>
    </label>
  {/each}
</section>
