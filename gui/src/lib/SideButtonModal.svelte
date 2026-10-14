<script>
  // Side-button remapping (macOS): turn the receiver's side button, normally
  // Volume Up, into a keyboard shortcut of the user's choosing.
  import {
    sideButtonState,
    setSideButton,
    requestSideButtonPermissions,
    openSideButtonSettings,
  } from "./api.js";

  let { onclose } = $props();

  let st = $state(null);
  let error = $state(null);
  let recording = $state(false);
  let saving = $state(false);

  const config = $derived(st?.config);
  const status = $derived(st?.status);
  const shortcut = $derived(config?.shortcut);
  const permissionsOk = $derived(status?.input_monitoring && status?.accessibility);

  const MODS = [
    { id: "ctrl", sym: "⌃", label: "Control" },
    { id: "option", sym: "⌥", label: "Option" },
    { id: "shift", sym: "⇧", label: "Shift" },
    { id: "command", sym: "⌘", label: "Command" },
  ];

  function describe(s) {
    if (!s || !st) return "";
    const key = st.keys.find((k) => k.code === s.key)?.label ?? s.key;
    return MODS.filter((m) => s[m.id]).map((m) => m.sym).join("") + key;
  }

  async function refresh() {
    if (saving) return; // don't clobber the optimistic value mid-save
    try {
      st = await sideButtonState();
    } catch (e) {
      error = String(e);
    }
  }

  async function save(next) {
    saving = true;
    error = null;
    st = { ...st, config: next }; // reflect instantly
    try {
      await setSideButton(next);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
      await refresh();
    }
  }

  const setEnabled = (enabled) => save({ ...config, enabled });
  const setShortcut = (patch) => save({ ...config, shortcut: { ...shortcut, ...patch } });

  // Capture the next non-modifier key press (with whatever modifiers are held)
  // as the new shortcut. Escape cancels.
  function onKeydown(e) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (/^(Control|Alt|Shift|Meta|OS)/.test(e.code)) return;
    recording = false;
    if (e.code === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) return;
    if (!st.keys.some((k) => k.code === e.code)) {
      error = `That key (${e.code || e.key}) can't be used — pick one from the list.`;
      return;
    }
    setShortcut({
      key: e.code,
      ctrl: e.ctrlKey,
      option: e.altKey,
      shift: e.shiftKey,
      command: e.metaKey,
    });
  }

  $effect(() => {
    refresh();
    const timer = setInterval(refresh, 500);
    return () => clearInterval(timer);
  });

  $effect(() => {
    window.addEventListener("keydown", onKeydown, true);
    return () => window.removeEventListener("keydown", onKeydown, true);
  });
</script>

<div class="overlay" onclick={onclose} role="presentation">
  <div class="modal" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
    <header>
      <h2>Side Button</h2>
      <button class="x" onclick={onclose} aria-label="Close">×</button>
    </header>

    <p class="lead">
      Make the receiver's side button send a keyboard shortcut instead of Volume Up — for
      example to start and stop hands-free dictation. Turning this off gives the button back
      to the system.
    </p>

    {#if !st}
      <p class="dim">Loading…</p>
    {:else}
      <div class="row">
        <span class="title">Send a shortcut when pressed</span>
        <button
          class="switch"
          class:on={config.enabled}
          role="switch"
          aria-checked={config.enabled}
          aria-label="Send a shortcut when pressed"
          disabled={saving}
          onclick={() => setEnabled(!config.enabled)}
        >
          <span class="knob"></span>
        </button>
      </div>

      <div class="row column" class:dim-row={!config.enabled}>
        <div class="row-head">
          <span class="title">Shortcut</span>
          <kbd class="combo">{describe(shortcut)}</kbd>
        </div>
        <div class="editor">
          <div class="mods" role="group" aria-label="Modifiers">
            {#each MODS as m (m.id)}
              <button
                class="mod"
                class:active={shortcut[m.id]}
                aria-pressed={shortcut[m.id]}
                title={m.label}
                onclick={() => setShortcut({ [m.id]: !shortcut[m.id] })}
              >
                {m.sym}
              </button>
            {/each}
          </div>
          <span class="plus">+</span>
          <select
            value={shortcut.key}
            aria-label="Key"
            onchange={(e) => setShortcut({ key: e.currentTarget.value })}
          >
            {#each st.keys as k (k.code)}
              <option value={k.code}>{k.label}</option>
            {/each}
          </select>
          <button class="btn" class:recording onclick={() => (recording = !recording)}>
            {recording ? "Press keys… (Esc cancels)" : "Record"}
          </button>
        </div>
      </div>

      <div class="row column">
        <span class="title">Permissions</span>
        <ul class="perms">
          <li>
            <span class="dot" class:ok={status.input_monitoring}></span>
            Input Monitoring <span class="dim">— to read the button</span>
            {#if !status.input_monitoring}
              <button class="link" onclick={() => openSideButtonSettings("input-monitoring")}>
                Open Settings
              </button>
            {/if}
          </li>
          <li>
            <span class="dot" class:ok={status.accessibility}></span>
            Accessibility <span class="dim">— to send the shortcut</span>
            {#if !status.accessibility}
              <button class="link" onclick={() => openSideButtonSettings("accessibility")}>
                Open Settings
              </button>
            {/if}
          </li>
        </ul>
        {#if !permissionsOk}
          <button class="btn" onclick={() => requestSideButtonPermissions()}>Request permissions</button>
        {/if}
      </div>

      {#if config.enabled}
        <div class="state">
          {#if status.active}
            <span class="dot ok"></span>
            {status.receivers > 0
              ? `Listening to ${status.receivers} receiver${status.receivers === 1 ? "" : "s"}`
              : "Waiting for a receiver"}
            · {status.presses} press{status.presses === 1 ? "" : "es"}, {status.sent} sent
          {:else}
            <span class="dot"></span> Not active
          {/if}
        </div>
      {/if}
    {/if}

    {#if error || status?.error}
      <div class="err" role="alert">{error ?? status.error}</div>
    {/if}

    <p class="foot">
      macOS may ask you to grant the permissions again after the app is updated. Don't run the
      <code>dji-mic</code> background service at the same time — only one app can hold the button.
    </p>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(8, 10, 14, 0.5);
    display: grid;
    place-items: center;
    padding: 24px;
    z-index: 50;
  }
  .modal {
    width: min(560px, 100%);
    max-height: 90vh;
    overflow-y: auto;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: 20px 22px 22px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  .x {
    border: none;
    background: transparent;
    color: var(--text-dim);
    font-size: 22px;
    line-height: 1;
  }
  .lead {
    color: var(--text-dim);
    line-height: 1.5;
  }
  .dim {
    color: var(--text-dim);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding: 13px 4px;
    border-bottom: 1px solid var(--border);
  }
  .row.column {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
  }
  .row.dim-row {
    opacity: 0.55;
  }
  .row-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .title {
    font-weight: 500;
  }
  .combo {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    padding: 3px 8px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg);
  }
  .editor {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
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
    width: 34px;
    padding: 4px 0;
    border-radius: 999px;
    font-size: 15px;
    transition: background 0.14s, color 0.14s;
  }
  .mod.active {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  .plus {
    color: var(--text-faint);
  }
  select {
    border: 1px solid var(--border-strong);
    background: var(--bg-panel);
    color: var(--text);
    border-radius: 6px;
    padding: 5px 8px;
    font: inherit;
    font-size: 13px;
  }
  .btn {
    border: 1px solid var(--border-strong);
    background: var(--bg-panel);
    color: var(--text);
    border-radius: 6px;
    padding: 5px 12px;
    font-size: 13px;
    align-self: flex-start;
  }
  .btn.recording {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .link {
    border: none;
    background: transparent;
    color: var(--accent);
    padding: 0 0 0 6px;
    font-size: 13px;
  }
  .perms {
    list-style: none;
    margin: 0;
    padding: 0;
    line-height: 1.9;
    font-size: 13px;
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-right: 6px;
    background: var(--danger);
  }
  .dot.ok {
    background: var(--good);
  }
  .state {
    padding: 12px 4px 0;
    font-size: 13px;
    color: var(--text-dim);
  }
  .err {
    margin-top: 12px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--danger) 14%, var(--bg-panel));
    font-size: 13px;
  }
  .foot {
    margin-bottom: 0;
    color: var(--text-faint);
    font-size: 12px;
    line-height: 1.5;
  }
  code {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
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
  .switch.on .knob {
    transform: translateX(17px);
  }
</style>
