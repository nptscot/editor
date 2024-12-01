<script lang="ts">
  import { VectorTileSource, LineLayer, GeoJSON } from "svelte-maplibre";
  import { Popup } from "svelte-utils/map";
  import {
    assetUrl,
    backend,
    mainModeRoutesChanged,
    autosave,
  } from "../stores";
  import LayerControls from "./LayerControls.svelte";

  let show = false;
  let firstLoad = false;

  $: if (show) {
    firstLoad = true;
  }

  let showTruth = true;
  let showMatched = true;

  async function importExisting() {
    if ($backend) {
      let numChanges = $backend.importCoreNetwork();
      let noun = numChanges == 1 ? "route segment" : "route segments";
      await autosave();
      $mainModeRoutesChanged += 1;
      window.alert(`Imported ${numChanges} ${noun}`);
    }
  }
</script>

<LayerControls name="core network">
  <label>
    <input type="checkbox" bind:checked={show} />
    Core network
  </label>

  {#if show}
    <button class="outline" on:click={importExisting}>
      Import core network
    </button>

    <label>
      <input type="checkbox" bind:checked={showTruth} />
      Show actual core network
    </label>

    <label style:color="red">
      <input type="checkbox" bind:checked={showMatched} />
      Show map-matched core network
    </label>
  {/if}
</LayerControls>

<!-- TODO Continue showing this for debugging the map matching -->
<VectorTileSource url={`pmtiles://${assetUrl("core_network.pmtiles")}`}>
  <LineLayer
    sourceLayer="coherent_networks"
    paint={{
      "line-color": "black",
      "line-width": 2,
    }}
    layout={{
      visibility: show && showTruth ? "visible" : "none",
    }}
  >
    <Popup openOn="hover" let:props>
      Go Dutch value {props.all_fastest_bicycle_go_dutch}
    </Popup>
  </LineLayer>
</VectorTileSource>

{#if $backend && firstLoad}
  {#await $backend.renderCoreNetwork() then data}
    <GeoJSON {data} generateId>
      <LineLayer
        paint={{
          "line-color": "red",
          "line-width": 10,
          "line-opacity": 0.5,
        }}
        layout={{
          visibility: show && showMatched ? "visible" : "none",
        }}
        filter={["get", "matched"]}
      >
      <Popup openOn="hover" let:props>
              <p>Hausdorff: {props.hausdorff}</p>
              <p>Frechet: {props.frechet}</p>
              <p>endpt1: {props.endpt1_diff}</p>
              <p>endpt2: {props.endpt2_diff}</p>
      </Popup>
      </LineLayer>
    </GeoJSON>
  {/await}
{/if}
