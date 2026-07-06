<script>
  import DevicePicto from "./DevicePicto.svelte";

  let { devices = [], selected = null, onselect } = $props();
</script>

<aside class="sidebar">
  <div class="sidebar-head">Devices</div>

  {#if devices.length === 0}
    <div class="empty">No devices</div>
  {:else}
    <ul class="device-list">
      {#each devices as d (d.id)}
        <li>
          <button
            class="device"
            class:active={d.id === selected}
            onclick={() => onselect?.(d.id)}
          >
            <span class="picto-wrap">
              <DevicePicto pictogram={`${d.pictogram_key}-rx`} size={40} />
            </span>
            <span class="labels">
              <span class="name">{d.model_name}</span>
              <span class="serial">{d.rx_serial ?? d.id}</span>
            </span>
            <span
              class="dot"
              class:connected={d.connected}
              title={d.connected ? "Connected" : "Connecting…"}
            ></span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</aside>

<style>
  .sidebar {
    width: 240px;
    height: 100%;
    background: var(--bg-panel);
    display: flex;
    flex-direction: column;
    overflow-y: auto;
  }
  .sidebar-head {
    padding: 16px 16px 8px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .empty {
    padding: 12px 16px;
    color: var(--text-faint);
  }
  .device-list {
    list-style: none;
    margin: 0;
    padding: 4px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .device {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    text-align: left;
    transition: background 0.12s, border-color 0.12s;
  }
  .device:hover {
    background: var(--bg-elev);
  }
  .device.active {
    background: var(--accent-soft);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .picto-wrap {
    flex: 0 0 40px;
    height: 40px;
    display: grid;
    place-items: center;
  }
  .labels {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }
  .name {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .serial {
    font-size: 12px;
    color: var(--text-dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dot {
    flex: 0 0 auto;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--text-faint);
  }
  .dot.connected {
    background: var(--good);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--good) 22%, transparent);
  }
</style>
