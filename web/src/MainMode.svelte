<script lang="ts">
  import {
    GeoJSON,
    hoverStateFilter,
    LineLayer,
    type LayerClickInfo,
  } from "svelte-maplibre";
  import { Popup } from "svelte-utils/map";
  import { SplitComponent } from "svelte-utils/two_column_layout";
  import { backend, mode } from "./stores";
  import type { FeatureCollection } from "geojson";
  import { onMount } from "svelte";
  import Link from "./common/Link.svelte";
  import { colorByInraType } from "./common";

  let gj: FeatureCollection | null = null;
  onMount(async () => {
    gj = await $backend!.renderRoutes();
  });

  function onKeyDown(e: KeyboardEvent) {
    if (e.key == "r") {
      e.preventDefault();
      $mode = { kind: "edit-route", id: null };
    }
  }

  function editRouteMap(e: CustomEvent<LayerClickInfo>) {
    $mode = { kind: "edit-route", id: e.detail.features[0].id as number };
  }

  function editRouteSidebar(id: string | number | undefined) {
    $mode = { kind: "edit-route", id: id as number };
  }
</script>

<svelte:window on:keydown={onKeyDown} />

<SplitComponent>
  <div slot="sidebar">
    <h2>Network Planning Workspace</h2>

    <h4>Build network:</h4>

    <button on:click={() => ($mode = { kind: "import-route" })}>
      Import from coherent network
    </button>

    <p style:text-align="center"><b>-or-</b></p>

    <button on:click={() => ($mode = { kind: "edit-route", id: null })}>
      Draw new <u>r</u>
      oute line
    </button>

    <h4>Evaluate:</h4>

    <button on:click={() => ($mode = { kind: "evaluate-route" })}>
      Evaluate a route
    </button>
    <button on:click={() => ($mode = { kind: "evaluate-od" })}>
      Evaluate OD
    </button>

    <button
      class="secondary"
      on:click={() => ($mode = { kind: "debug-network" })}
    >
      Debug network
    </button>
    <button class="secondary" on:click={() => ($mode = { kind: "debug-od" })}>
      Debug OD
    </button>
    <button
      class="secondary"
      on:click={() => ($mode = { kind: "debug-mesh-density" })}
    >
      Debug mesh density
    </button>

    <h4>Current network:</h4>

    {#if gj}
      <ol>
        {#each gj.features as f}
          <li>
            <Link on:click={() => editRouteSidebar(f.id)}>
              {f.properties?.name || `Untitled route ${f.id}`} ({f.properties
                ?.infra_type})
            </Link>
          </li>
        {/each}
      </ol>
    {/if}
  </div>

  <div slot="map">
    {#if gj}
      <GeoJSON data={gj}>
        <LineLayer
          id="routes"
          paint={{
            "line-width": hoverStateFilter(5, 7),
            "line-color": colorByInraType,
          }}
          manageHoverState
          hoverCursor="pointer"
          on:click={editRouteMap}
        >
          <Popup openOn="hover" let:props>
            {props.name || "Untitled"} ({props.infra_type})
          </Popup>
        </LineLayer>
      </GeoJSON>
    {/if}
  </div>
</SplitComponent>
