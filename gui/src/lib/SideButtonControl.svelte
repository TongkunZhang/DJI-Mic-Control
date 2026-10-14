<script>
  // One receiver's side-button remapping (macOS): turn its side button,
  // normally Volume Up, into a keyboard shortcut. Rendered as a settings
  // group alongside the device's other settings. `info` is the snapshot's
  // `side_button` for this receiver; `onchange` receives a new binding.
  import { requestSideButtonPermissions, openSideButtonSettings } from "./api.js";

  let { info, keys = [], compact = false, pending = false, onchange } = $props();

  let recording = $state(false);
  let recordError = $state(null);

  const binding = $derived(info.binding);
  const shortcut = $derived(binding.shortcut);
  const disabled = $derived(pending || !info.available);
  const missingPermissions = $derived(!info.input_monitoring || !info.accessibility);

  const MODS = [
    { id: "ctrl", sym: "⌃", label: "Control" },
    { id: "option", sym: "⌥", label: "Option" },
    { id: "shift", sym: "⇧", label: "Shift" },
    { id: "command", sym: "⌘", label: "Command" },
  ];

  const combo = $derived(
    MODS.filter((m) => shortcut[m.id]).map((m) => m.sym).join("") +
      (keys.find((k) => k.code === shortcut.key)?.label ?? shortcut.key),
  );

  const setEnabled = (enabled) => onchange?.({ ...binding, enabled });
  const setShortcut = (patch) => onchange?.({ ...binding, shortcut: { ...shortcut, ...patch } });

  // Capture the next non-modifier key press (with whatever modifiers are held)
  // as the new shortcut. A bare Escape cancels.
  function onKeydown(e) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (/^(Control|Alt|Shift|Meta|OS)/.test(e.code)) return;
    recording = false;
    if (e.code === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) return;
    if (!keys.some((k) => k.code === e.code)) {
      recordError = `${e.code || e.key} can't be used — pick a key from the list.`;
      return;
    }
    recordError = null;
    setShortcut({
      key: e.code,
      ctrl: e.ctrlKey,
      option: e.altKey,
      shift: e.shiftKey,
      command: e.metaKey,
    });
  }

  $effect(() => {
    if (!recording) return;
    window.addEventListener("keydown", onKeydown, true);
    return () => window.removeEventListener("keydown", onKeydown, true);
  });
</script>

<div class="side" class:compact class:busy={pending}>
  <div class="row">
    <div class="label">
      <span class="title">Remap side button</span>
      <span class="sub">
        {#if !info.available}
          Waiting for the receiver…
        {:else if binding.enabled}
          Sends {combo} instead of Volume Up
        {:else}
          Acts as Volume Up
        {/if}
      </span>
    </div>
    <button
      class="switch"
      class:on={binding.enabled}
      role="switch"
      aria-checked={binding.enabled}
      aria-label="Remap side button"
      {disabled}
      onclick={() => setEnabled(!binding.enabled)}
    >
      <span class="knob"></span>
    </button>
  </div>

  {#if binding.enabled}
    <div class="row column">
      <div class="row-head">
        <span class="title">Shortcut</span>
        <kbd class="combo">{combo}</kbd>
      </div>
      <div class="editor">
        <div class="mods" role="group" aria-label="Modifiers">
          {#each MODS as m (m.id)}
            <button
              class="mod"
              class:active={shortcut[m.id]}
              aria-pressed={shortcut[m.id]}
              title={m.label}
              {disabled}
              onclick={() => setShortcut({ [m.id]: !shortcut[m.id] })}
            >
              {m.sym}
            </button>
          {/each}
        </div>
        <select
          value={shortcut.key}
          aria-label="Key"
          {disabled}
          onchange={(e) => setShortcut({ key: e.currentTarget.value })}
        >
          {#each keys as k (k.code)}
            <option value={k.code}>{k.label}</option>
          {/each}
        </select>
        <button class="btn" class:recording {disabled} onclick={() => (recording = !recording)}>
          {recording ? "Press keys…" : "Record"}
        </button>
      </div>
      {#if recording}
        <span class="hint">Press the combination to use. Esc cancels.</span>
      {/if}
    </div>

    {#if missingPermissions}
      <div class="row column warn">
        <span class="title">Permissions needed</span>
        <span class="hint">
          {#if !info.input_monitoring}Input Monitoring (to read the button){/if}
          {#if !info.input_monitoring && !info.accessibility} and {/if}
          {#if !info.accessibility}Accessibility (to send the shortcut){/if}.
        </span>
        <div class="actions">
          <button class="btn" onclick={() => requestSideButtonPermissions()}>Request</button>
          <button
            class="link"
            onclick={() =>
              openSideButtonSettings(info.input_monitoring ? "accessibility" : "input-monitoring")}
          >
            Open System Settings
          </button>
        </div>
      </div>
    {:else}
      <div class="state">
        <span class="dot" class:ok={info.attached}></span>
        {#if info.attached}
          Active · {info.presses} press{info.presses === 1 ? "" : "es"}, {info.sent} sent
        {:else}
          Not active yet
        {/if}
      </div>
    {/if}
  {/if}

  {#if recordError || info.error}
    <div class="err" role="alert">{recordError ?? info.error}</div>
  {/if}
</div>

<style>
  .side {
    display: flex;
    flex-direction: column;
  }
  .side.compact {
    gap: 8px;
  }
  .side.busy {
    cursor: progress;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding: 13px 4px;
    border-bottom: 1px solid var(--border);
  }
  .compact .row {
    padding: 8px 10px;
    border-bottom: none;
    background: var(--bg-elev);
    border-radius: var(--radius-sm);
    gap: 8px;
  }
  .row.column {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
  }
  .row.warn {
    background: color-mix(in srgb, var(--warn) 12%, var(--bg-elev));
  }
  .side:not(.compact) .row.warn {
    margin-top: 10px;
    padding: 10px 12px;
    border-bottom: none;
    border-radius: var(--radius-sm);
  }
  .row-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .title {
    font-weight: 500;
  }
  .compact .title {
    font-size: 13px;
  }
  .sub,
  .hint {
    font-size: 12px;
    color: var(--text-dim);
  }
  .combo {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    padding: 2px 7px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg);
  }
  .editor {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }
  .mods {
    display: inline-flex;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px;
  }
  .mod {
    border: none;
    background: transparent;
    color: var(--text-dim);
    width: 30px;
    padding: 3px 0;
    border-radius: 999px;
    font-size: 14px;
    transition: background 0.14s, color 0.14s;
  }
  .mod.active {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  select {
    border: 1px solid var(--border-strong);
    background: var(--bg-panel);
    color: var(--text);
    border-radius: 6px;
    padding: 4px 6px;
    font: inherit;
    font-size: 12px;
  }
  .btn {
    border: 1px solid var(--border-strong);
    background: var(--bg-panel);
    color: var(--text);
    border-radius: 6px;
    padding: 4px 10px;
    font-size: 12px;
  }
  .btn.recording {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .link {
    border: none;
    background: transparent;
    color: var(--accent);
    padding: 0;
    font-size: 12px;
  }
  .state {
    display: flex;
    align-items: center;
    padding: 4px 10px 0;
    font-size: 12px;
    color: var(--text-dim);
  }
  .side:not(.compact) .state {
    padding: 12px 4px;
  }
  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    margin-right: 6px;
    background: var(--text-faint);
  }
  .dot.ok {
    background: var(--good);
  }
  .err {
    padding: 6px 10px;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--danger) 14%, var(--bg-panel));
    font-size: 12px;
  }
  button:disabled,
  select:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* Switch (matches SettingControl) */
  .switch {
    width: 42px;
    height: 25px;
    border-radius: 999px;
    border: none;
    background: var(--border-strong);
    padding: 0;
    position: relative;
    transition: background 0.16s;
    flex: 0 0 auto;
  }
  .compact .switch {
    width: 36px;
    height: 21px;
  }
  .switch.on {
    background: var(--accent);
  }
  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 19px;
    height: 19px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
    transition: transform 0.16s;
  }
  .compact .knob {
    width: 15px;
    height: 15px;
  }
  .switch.on .knob {
    transform: translateX(17px);
  }
  .compact .switch.on .knob {
    transform: translateX(15px);
  }
</style>
