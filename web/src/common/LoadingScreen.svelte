<script lang="ts">
  // Block the rest of the page while 'loading' is non-empty
  import { loading } from "../stores";
</script>

<div bind:this={$loading} style:visibility="none" id="loading" />

<style>
  div {
    position: fixed;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    justify-content: center;
    align-items: center;
    z-index: 999;

    color: white;
    font-size: 32px;
  }
</style>
