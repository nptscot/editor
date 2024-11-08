<script lang="ts">
  import { loadingScreen, backend, stats } from "./stores";
  import { onMount } from "svelte";

  async function recalc() {
    await loadingScreen("Recalculating stats");
    $stats = await $backend!.recalculateStats();
    await loadingScreen("");
  }

  onMount(async () => {
    if ($stats == null) {
      await recalc();
    }
  });
</script>

<button on:click={recalc}>Recalculate</button>

{#if $stats}
  <p>
    Average weighted directness: {$stats.average_weighted_directness.toFixed(
      1,
    )}x
  </p>

  <details>
    <summary>Percent of demand by infrastructure type</summary>
    <ul>
      {#each Object.entries($stats.od_percents_infra_type).toSorted((a, b) => b[1] - a[1]) as [key, percent]}
        <li>{key}: {(100 * percent).toFixed(1)}%</li>
      {/each}
    </ul>
  </details>

  <details>
    <summary>Percent of demand by level of service:</summary>
    <ul>
      {#each Object.entries($stats.od_percents_los).toSorted((a, b) => b[1] - a[1]) as [key, percent]}
        <li>{key}: {(100 * percent).toFixed(1)}%</li>
      {/each}
    </ul>
  </details>
{/if}
