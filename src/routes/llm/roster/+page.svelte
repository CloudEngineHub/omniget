<script lang="ts">
  import SurfaceGuide from "$components/llm/SurfaceGuide.svelte";
  import { surfaceCopy } from "$components/llm/surface-copy";
  /** Roster tab: the agent list, the editor and the team templates. */
  import { onMount } from "svelte";
  import { t } from "$lib/i18n";
  import type { AgentDef } from "$lib/llm/types";
  import { agentTint, roleLabelKey, roleText, modelLabel } from "$lib/llm/types";
  import {
    applyTemplate,
    deleteAgent,
    getAgents,
    isRosterAvailable,
    loadRoster,
    saveAgent,
  } from "$lib/stores/llm-store.svelte";
  import { tintToCss } from "$lib/stores/profile-store.svelte";
  import RosterEditor from "$components/llm/RosterEditor.svelte";
  import TemplatePicker from "$components/llm/TemplatePicker.svelte";

  let agents = $derived(getAgents());
  let editing = $state<AgentDef | null>(null);
  let query = $state("");
  let filtered = $derived(agents.filter(agent => `${agent.name} ${agent.system_prompt}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())));

  onMount(() => {
    void loadRoster();
  });

  function blankAgent(): AgentDef {
    return {
      id: `agent-${Date.now().toString(36)}`,
      name: "",
      role: "worker",
      system_prompt: "",
      model: { policy: "fixed", model: { provider: "openai", model: "" } },
      tools: [],
      skills: [],
      budget: {},
      runtime: { kind: "native" },
      skin: { id: "omni-default", tint: [110, 139, 255] },
    };
  }

  let saving = $state(false);
  let saveError = $state(false);
  let saved = $state(false);
  async function onSave(agent: AgentDef) {
    if (saving) return;
    saving = true;
    saveError = false;
    saved = false;
    const ok = await saveAgent(agent);
    saving = false;
    if (ok) { editing = null; saved = true; }
    else saveError = true;
  }
  async function onTemplate(id: string) {
    if (saving) return;
    saving = true; saveError = false; saved = false;
    const ok = await applyTemplate(id);
    saving = false;
    saveError = !ok;
  }
  async function onDelete(id: string) {
    if (saving) return;
    saving = true;
    saveError = false;
    const ok = await deleteAgent(id);
    saving = false;
    if (ok) editing = null;
    else saveError = true;
  }
</script>

<svelte:head><title>{$t("llm.roster.title")}</title></svelte:head>

<div class="page page-wide roster-page">
  <header class="page-head">
    <div>
      <h1 class="page-title">{$t("llm.roster.title")}</h1>
      <p class="page-lede">{$t("llm.roster.roles_hint")}</p>
    </div>
    <button type="button" class="button primary" disabled={saving} onclick={() => { saved = false; saveError = false; editing = blankAgent(); }}>
      {$t("llm.roster.new")}
    </button>
  </header>
  <SurfaceGuide text={$surfaceCopy.rosterHint} href="/help?article=agent#guide" />

  {#if !isRosterAvailable()}
    <p class="notice" role="status">{$t("llm.roster.unavailable")}</p>
    <button type="button" class="button" onclick={() => void loadRoster(true)}>{$surfaceCopy.retry}</button>
  {/if}

  {#if saveError}<p class="notice" role="alert">{$t("llm.roster.save_error")}</p>{/if}
  {#if saved}<p class="notice" role="status">{$t("llm.roster.saved")}</p>{/if}
  {#if editing}
    {#key editing.id}
    <RosterEditor
      agent={editing}
      busy={saving}
      onsave={onSave}
      oncancel={() => { editing = null; saveError = false; }}
      ondelete={agents.some((a) => a.id === editing?.id)
        ? onDelete
        : undefined}
    />
    {/key}
  {:else}
    <label class="agent-search"><span>{$surfaceCopy.search}</span><input class="input" type="search" bind:value={query} /></label>
    <div class="group">
      {#each filtered as agent (agent.id)}
        <button type="button" class="group-row agent-row" onclick={() => (editing = agent)}>
          <span class="dot" style:background={tintToCss(agentTint(agent))}></span>
          <span class="group-row-content">
            <span class="group-row-title">{agent.name}</span>
            <span class="group-row-sub">
              {agent.system_prompt.split("\n")[0] || (typeof agent.role === "string" && agent.role in $surfaceCopy ? $surfaceCopy[agent.role as "worker"] : roleText(agent.role))}
              <span class="agent-connection">{modelLabel(agent.model)}</span>
            </span>
          </span>
          <span class="group-row-trailing chevron">›</span>
        </button>
      {/each}
    </div>

    {#if filtered.length === 0 && query}<p role="status">{$surfaceCopy.noMatches}</p>{/if}
    <details class="team-templates"><summary>{$surfaceCopy.team}</summary><p>{$surfaceCopy.templatesImpact}</p><TemplatePicker disabled={saving} onapply={onTemplate} /></details>
  {/if}
</div>

<style>
  .agent-search { display:flex; flex-direction:column; gap:8px; max-width:400px; margin-bottom:20px; font-size:13px; color:var(--text-muted); }
  .agent-connection { display:block; margin-top:6px; color:var(--text-muted); }
  .team-templates summary { cursor:pointer; padding:16px 0; font-weight:600; }
  .team-templates p { color:var(--text-muted); font-size:13px; }
  /* Block layout on purpose: as a flex column the groups would shrink to fit
     the viewport and clip their own rows instead of letting the page scroll. */
  .roster-page {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .roster-page :global(.group) {
    margin-bottom: var(--space-5);
  }

  .notice {
    margin: 0 0 var(--space-4);
    font-size: var(--text-sm);
    color: var(--text-dim);
  }

  .agent-row {
    width: 100%;
    border: none;
    background: transparent;
    text-align: left;
    cursor: pointer;
    color: inherit;
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .dot {
    width: 24px;
    height: 24px;
    border-radius: var(--radius-full);
    flex-shrink: 0;
  }
</style>
