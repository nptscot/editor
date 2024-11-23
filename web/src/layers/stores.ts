import { writable, type Writable } from "svelte/store";

export let allControls: Writable<Map<string, HTMLDivElement>> = writable(
  new Map(),
);

// Only layers that need to be controlled remotely need to be here
export let currentNetwork = writable(true);
export let schools = writable(false);
export let gpHospitals = writable(false);
export let townCentres = writable(false);
export let imdZones = writable(false);
export let highRouteCoverage = writable(false);
