<script lang="ts">
  // Presentation follows 21st AI Input by Hayden Bleasel (1887): one autosizing input and a compact action row. Original Svelte implementation with no simulated attachments or capabilities.
  /**
   * Composer: one auto-growing textarea, Enter sends, Shift+Enter breaks the
   * line. While a turn runs the primary action becomes Stop, so there is only
   * ever one primary action on screen.
   */
  import { t } from "$lib/i18n";

  let {
    value = $bindable(""),
    disabled = false,
    running = false,
    onsend,
    onstop,
  }: {
    value?: string;
    disabled?: boolean;
    running?: boolean;
    onsend: (text: string) => boolean | Promise<boolean>;
    onstop: () => void;
  } = $props();

  const MAX_HEIGHT = 200;

  let sending = $state(false);
  $effect(() => { value; queueMicrotask(autosize); });
  let textarea = $state<HTMLTextAreaElement | null>(null);

  function autosize() {
    const el = textarea;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, MAX_HEIGHT)}px`;
  }

  async function send() {
    const text = value.trim();
    if (!text || disabled || running || sending) return;
    sending = true;
    try {
      const accepted = await onsend(text);
      // Keep text typed during startup, and retain the entire draft on failure.
      if (accepted && value.trim() === text) value = "";
    } finally { sending = false; queueMicrotask(autosize); }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      send();
    }
  }
</script>

<div class="composer">
  <textarea
    bind:this={textarea}
    bind:value
    class="input composer-input"
    rows="1"
    {disabled}
    placeholder={$t("llm.composer.placeholder")}
    aria-label={$t("llm.composer.placeholder")}
    oninput={autosize}
    onkeydown={onKeydown}
  ></textarea>
  <div class="composer-actions">
    <span class="composer-hint">{$t("llm.composer.hint")}</span>
    {#if running}
      <button type="button" class="button composer-send" onclick={onstop}>
        {$t("llm.conv.stop")}
      </button>
    {:else}
      <button
        type="button"
        class="button primary composer-send"
        disabled={disabled || sending || value.trim().length === 0}
        onclick={send}
      >
        {$t("llm.composer.send")}
      </button>
    {/if}
  </div>
</div>

<style>
  .composer {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-5) var(--space-4);
    width: calc(100% - 40px);
    max-width: 760px;
    box-sizing: border-box;
    align-self: center;
    margin: 12px 20px 20px;
    border: 1px solid var(--separator);
    border-radius: var(--radius-xl);
    background: var(--fill-1);
    box-shadow: 0 2px 8px color-mix(in srgb, var(--text) 4%, transparent);
  }

  .composer-input {
    width: 100%;
    resize: none;
    border: none;
    box-shadow: none;
    background: transparent;
    font-size: 15px;
    min-height: 40px;
    max-height: 200px;
    line-height: var(--leading-base);
    padding: var(--space-2) var(--space-3);
  }

  .composer-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  @media (max-width: 480px) { .composer-actions { flex-wrap: wrap; } .composer-hint { flex: 1 1 140px; } }

  .composer-hint {
    font-size: var(--text-caption);
    color: var(--text-dim);
  }

  .composer-send {
    min-width: 96px;
  }
</style>
