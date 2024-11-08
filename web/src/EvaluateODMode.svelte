<script lang="ts">
  import { GeoJSON, LineLayer } from "svelte-maplibre";
  import { SplitComponent } from "svelte-utils/three_column_layout";
  import { Popup } from "svelte-utils/map";
  import { backend, mode, type EvaluateODOut } from "./stores";
  import { onMount } from "svelte";
  import { colorByInraType } from "./common";
  import type {
    ExpressionSpecification,
    DataDrivenPropertyValueSpecification,
  } from "maplibre-gl";
  import { Loading } from "svelte-utils";

  let gj: EvaluateODOut | null = null;
  let loading = "";

  onMount(async () => {
    loading = "Calculating routes for all OD data";
    gj = await $backend!.evaluateOD();
    loading = "";
  });

  function lineWidth(
    maxCount: number,
  ): DataDrivenPropertyValueSpecification<number> {
    let min = 0;

    // Linearly interpolate between thin and thick, based on the percent each count is between min and max
    let thin = 2;
    let thick = 10;

    let range_input = maxCount - min;
    let range_output = thick - thin;
    // min(1, (value - min) / range_input)
    let calculatePercent: ExpressionSpecification = [
      "min",
      1.0,
      ["/", ["-", ["get", "count"], min], range_input],
    ];
    // thin + range_output * percent
    return ["+", thin, ["*", range_output, calculatePercent]];
  }
</script>

<Loading {loading} />

<SplitComponent>
  <div slot="left">
    <h2>Evaluate OD mode</h2>
    <button on:click={() => ($mode = { kind: "main" })}>Back</button>

    {#if gj}
      <p>
        {gj.succeeded.toLocaleString()} routes succeeded, {gj.failed.toLocaleString()}
        failed
      </p>
      <p>Highest count on any one road is {gj.max_count.toLocaleString()}</p>

      <p>Percent of demand by infrastructure type:</p>
      <ul>
        <li>Off the network: {(100 * gj.percent_off_network).toFixed(1)}%</li>
        {#each Object.entries(gj.percent_on_network) as [key, percent]}
          <li>{key}: {(100 * percent).toFixed(1)}%</li>
        {/each}
      </ul>
    {/if}
  </div>

  <div slot="map">
    {#if gj}
      <GeoJSON data={gj} generateId>
        <LineLayer
          paint={{
            "line-width": lineWidth(gj.max_count),
            "line-color": colorByInraType,
          }}
          manageHoverState
          eventsIfTopMost
        >
          <Popup openOn="hover" let:props>
            {props.count} on {props.infra_type}
          </Popup>
        </LineLayer>
      </GeoJSON>
    {/if}
  </div>
</SplitComponent>
