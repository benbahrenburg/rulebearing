<script context="module" lang="ts">
  import { shared } from "./shared";
</script>

<script lang="ts">
  import type { Props } from "./props";
  import helper from "./helper.js";
  export let name: Props["name"] = shared;
</script>

<h1>{helper(name)}</h1>
<style lang="scss">h1 { color: red }</style>
