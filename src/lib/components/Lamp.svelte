<script lang="ts">
  import type { Readiness } from "../types";

  interface Props {
    state: Readiness;
    size?: "sm" | "lg";
  }

  let { state, size = "sm" }: Props = $props();
</script>

<span class="lamp {size} {state}" aria-hidden="true"></span>

<style>
  .lamp {
    display: inline-block;
    flex: none;
    border-radius: 50%;
    background: var(--lamp-off);
    animation: lamp-on 200ms ease both;
    animation-delay: var(--lamp-delay, 0ms);
  }

  .lamp.sm {
    width: 8px;
    height: 8px;
  }

  .lamp.lg {
    width: 12px;
    height: 12px;
  }

  .lamp.ready {
    background: var(--lamp-green);
    box-shadow: 0 0 8px 2px color-mix(in srgb, var(--lamp-green) 55%, transparent);
  }

  .lamp.lg.ready {
    box-shadow: 0 0 12px 3px color-mix(in srgb, var(--lamp-green) 55%, transparent);
  }

  .lamp.failed {
    background: var(--lamp-red);
    box-shadow: 0 0 8px 2px color-mix(in srgb, var(--lamp-red) 55%, transparent);
  }

  .lamp.lg.failed {
    box-shadow: 0 0 12px 3px color-mix(in srgb, var(--lamp-red) 55%, transparent);
  }

  .lamp.none,
  .lamp.untrusted {
    background: var(--lamp-off);
    box-shadow: none;
  }

  @keyframes lamp-on {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
</style>
