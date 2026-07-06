<script>
  import DevicePicto from "./DevicePicto.svelte";
  import SettingControl from "./SettingControl.svelte";

  let {
    device,
    status,
    settings = [],
    values = {},
    pending = {},
    view = "compact",
    onchange,
    onview,
  } = $props();

  // Audio levels sit roughly in the 0x14–0x4f range; scale to a full bar.
  const levelPct = (v) => Math.max(0, Math.min(100, Math.round((v / 80) * 100)));

  // Cluster settings by their declared group, preserving first-seen order.
  const groups = $derived.by(() => {
    const map = new Map();
    for (const s of settings) {
      if (!map.has(s.group)) map.set(s.group, []);
      map.get(s.group).push(s);
    }
    return [...map.entries()];
  });

  // JS masonry: distribute group cards into balanced columns ourselves rather
  // than using CSS multi-column, which paints card shadows across column
  // boundaries. Each card lands in the currently-shortest column.
  let groupsWidth = $state(0);
  const MIN_COL = 260;
  const COL_GAP = 18;
  const colCount = $derived(
    Math.max(1, Math.min(groups.length, Math.floor((groupsWidth + COL_GAP) / (MIN_COL + COL_GAP)))),
  );
  const columns = $derived.by(() => {
    const cols = Array.from({ length: colCount }, () => []);
    const heights = new Array(colCount).fill(0);
    for (const [name, items] of groups) {
      let min = 0;
      for (let i = 1; i < colCount; i++) if (heights[i] < heights[min]) min = i;
      cols[min].push([name, items]);
      heights[min] += items.length + 1.5; // ~header + rows, as a height estimate
    }
    return cols;
  });

  const labelById = $derived(Object.fromEntries(settings.map((s) => [s.id, s.label])));

  // Why a setting is locked, or null if it is freely settable.
  function lockReason(s) {
    // NC mode is meaningless unless NC is enabled (physical button on the TX).
    if (s.id === "noise-cancel" && status.nc_enabled === false) {
      return "Turn on Noise Cancel using the button on the transmitter";
    }
    // Mutually exclusive settings lock each other while active.
    for (const other of s.exclusive_with ?? []) {
      if (values[other] === "on") return `Turn off ${labelById[other] ?? other} first`;
    }
    return null;
  }
</script>

<div class="panel">
  <header class="head">
    <span class="picto"><DevicePicto pictogram={`${device.pictogram_key}-rx`} size={54} /></span>
    <div class="titles">
      <h1>{device.model_name}</h1>
      <span class="sub">{status.rx?.serial ?? device.id}</span>
    </div>
    <span class="pill" class:on={status.connected}>
      {status.connected ? "Connected" : "No signal"}
    </span>
  </header>

  <section class="cards">
    <div class="card">
      <div class="card-head">
        <span class="card-title">Receiver</span>
        <DevicePicto pictogram={`${device.pictogram_key}-rx`} size={40} />
      </div>
      {#if status.rx}
        <div class="kv"><span>Serial</span><b>{status.rx.serial}</b></div>
        <div class="kv"><span>Firmware</span><b>{status.rx.firmware}</b></div>
      {:else}
        <div class="muted">No data</div>
      {/if}
      <div class="kv">
        <span>Noise Cancelling</span>
        <b class={status.nc_enabled ? "good" : "muted"}>{status.nc_enabled ? "On" : "Off"}</b>
      </div>
    </div>

    {#each status.tx as tx, i}
      <div class="card">
        <div class="card-head">
          <span class="card-title">Transmitter {i + 1}</span>
          <DevicePicto pictogram={`${device.pictogram_key}-tx`} size={38} />
        </div>
        {#if tx}
          <div class="kv"><span>Serial</span><b>{tx.serial}</b></div>
          <div class="kv"><span>Firmware</span><b>{tx.firmware}</b></div>
          <div class="level">
            <span class="lvl-label">Level</span>
            <div class="meter">
              <div class="unfilled" style="left:{levelPct(tx.level)}%"></div>
            </div>
          </div>
        {:else}
          <div class="muted">Disconnected</div>
        {/if}
      </div>
    {/each}
  </section>

  <section class="settings-head">
    <span class="settings-title">Settings</span>
    <div class="view-switch" role="group" aria-label="Settings layout">
      <button class:active={view === "compact"} onclick={() => onview?.("compact")}>Compact</button>
      <button class:active={view === "list"} onclick={() => onview?.("list")}>List</button>
    </div>
  </section>

  {#if view === "list"}
    <section class="settings">
      {#each settings as s (s.id)}
        <SettingControl
          setting={s}
          value={values[s.id] ?? null}
          pending={!!pending[s.id]}
          locked={!!lockReason(s)}
          lockReason={lockReason(s) ?? ""}
          {onchange}
        />
      {/each}
    </section>
  {:else}
    <section class="groups" bind:clientWidth={groupsWidth}>
      {#each columns as col, ci (ci)}
        <div class="col">
          {#each col as [name, items] (name)}
            <div class="group">
              <div class="group-title">{name}</div>
              <div class="group-grid">
                {#each items as s (s.id)}
                  <SettingControl
                    setting={s}
                    value={values[s.id] ?? null}
                    pending={!!pending[s.id]}
                    locked={!!lockReason(s)}
                    lockReason={lockReason(s) ?? ""}
                    compact
                    {onchange}
                  />
                {/each}
              </div>
            </div>
          {/each}
        </div>
      {/each}
    </section>
  {/if}
</div>

<style>
  .panel {
    flex: 1 1 auto;
    height: 100%;
    overflow-y: auto;
    padding: 22px 26px 40px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 22px;
  }
  .picto {
    width: 52px;
    height: 52px;
    display: grid;
    place-items: center;
    color: var(--accent);
  }
  .titles {
    flex: 1 1 auto;
    min-width: 0;
  }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 650;
  }
  .sub {
    color: var(--text-dim);
    font-size: 13px;
  }
  .pill {
    font-size: 12px;
    font-weight: 600;
    padding: 5px 11px;
    border-radius: 999px;
    background: var(--bg-elev);
    color: var(--text-dim);
    border: 1px solid var(--border);
  }
  .pill.on {
    color: var(--good);
    border-color: color-mix(in srgb, var(--good) 40%, transparent);
    background: color-mix(in srgb, var(--good) 12%, transparent);
  }

  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 14px;
    margin-bottom: 22px;
  }
  .card {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 15px 16px;
    box-shadow: var(--shadow);
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 10px;
    min-height: 40px;
  }
  .card-title {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .kv {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 3px 0;
    font-size: 13px;
  }
  .kv span {
    color: var(--text-dim);
  }
  .kv b {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--text-faint);
  }
  .good {
    color: var(--good);
  }
  .level {
    margin-top: 8px;
  }
  .lvl-label {
    font-size: 12px;
    color: var(--text-dim);
  }
  /* VU meter: the track holds a fixed green→yellow→red gradient across its full
     width; the .unfilled overlay hides everything past the current level, so low
     levels show green and high levels reveal yellow then red. */
  .meter {
    position: relative;
    margin-top: 5px;
    height: 8px;
    border-radius: 999px;
    overflow: hidden;
    border: 1px solid var(--border);
    background: linear-gradient(
      90deg,
      #2ea043 0%,
      #57b846 35%,
      #e0b400 68%,
      #e8863a 85%,
      #e5484d 100%
    );
  }
  .unfilled {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    left: 0;
    background: var(--bg);
    transition: left 0.09s linear;
  }

  .settings-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
  }
  .settings-title {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .view-switch {
    display: inline-flex;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px;
  }
  .view-switch button {
    border: none;
    background: transparent;
    color: var(--text-dim);
    padding: 4px 12px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
  }
  .view-switch button.active {
    background: var(--accent);
    color: var(--accent-contrast);
  }

  .settings {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px 18px 8px;
    box-shadow: var(--shadow);
  }

  /* Masonry via balanced flex columns (see the script). Normal block flow means
     shadows render cleanly, unlike CSS multi-column. */
  .groups {
    display: flex;
    align-items: flex-start;
    gap: 18px;
  }
  .col {
    flex: 1 1 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .group {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px 14px 14px;
    box-shadow: var(--shadow-card);
  }
  .group-title {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
    margin-bottom: 10px;
  }
  .group-grid {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
</style>
