<script lang="ts">
  import type { Snippet } from "svelte";
  import "$lib/styles/fonts.css";
  import "$lib/styles/tokens.css";
  import "$lib/styles/global.css";

  let { children }: { children: Snippet } = $props();
</script>

{@render children()}
