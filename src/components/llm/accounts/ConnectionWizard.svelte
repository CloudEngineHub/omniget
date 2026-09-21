<script lang="ts">
  // Stepper composition adapted from 21st progress-02; native Svelte controls and real IPC.
  import { onDestroy, onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { locale } from '$lib/i18n';
  import { invoke } from '@tauri-apps/api/core';
  import { testConnection, parseConnectionDraft, CONNECTION_TOKEN_LIMIT, connectionMatchesIntent } from '$lib/llm/connection-setup';
  import { loadAccounts, type AccountView, type AccountsSnapshot, type CliDetected } from '$lib/stores/llm-accounts-store.svelte';
  import { loadRoster, selectAgent } from '$lib/stores/llm-store.svelte';
  import type { AgentDef } from '$lib/llm/types';
  let { initial = '' }: { initial?: string } = $props();
  const copy = (pt: string, en: string) => String($locale).startsWith('pt') ? pt : en;
  type Kind = { id: string; name: string; base_url: string };
  let mode = $state('');
  let step = $state(0);
  let busy = $state(false);
  let error = $state('');
  let clis = $state<CliDetected[]>([]);
  let accounts = $state<AccountView[]>([]);
  let cli = $state('claude');
  let account = $state('');
  let name = $state('');
  let provider = $state('openai');
  let kinds = $state<Kind[]>([]);
  let key = $state('');
  let models = $state<string[]>([]);
  let model = $state('');
  let loginOpened = $state(false);
  let verified = $state(false);
  let agentId = $state('');
  let attempt = $state('');
  let credentialId = $state('');
  let controller: AbortController | null = null;
  let hydrated = $state(false);
  onMount(() => {
    try {
      const saved = parseConnectionDraft(JSON.parse(localStorage.getItem('omniget-connection-setup') || 'null'));
      if (saved && ['subscription', 'api', 'local'].includes(saved.mode) && (!initial || initial === saved.mode)) {
        ({ mode, step, cli, account, name, provider, models, model, loginOpened, agentId, attempt, credentialId } = saved);
        void guard(async () => {
          if (mode === 'subscription') {
            clis = await invoke('llm_accounts_detect');
            accounts = (await invoke<AccountsSnapshot>('llm_accounts_list')).accounts;
          } else if (mode === 'api') kinds = await invoke('tool_keys_kinds');
        });
      } else if (initial) void choose(initial);
    } catch { /* A malformed draft can safely be discarded. */ }
    hydrated = true;
  });
  function persist() {
    const draft = { version: 1, mode, step, cli, account, name, provider, models, model, loginOpened, agentId, attempt, credentialId };
    try { localStorage.setItem('omniget-connection-setup', JSON.stringify(draft)); } catch { /* Persistence is optional when storage is unavailable. */ }
  }
  $effect(() => { if (hydrated) persist(); });
  function changed(providerChanged = false) {
    attempt = crypto.randomUUID(); agentId = ''; verified = false;
    if (providerChanged) { credentialId = ''; key = ''; models = []; model = ''; }
    persist();
  }
  onDestroy(() => controller?.abort());
  async function guard(fn: () => Promise<void>) {
    if (busy) return; busy = true; error = '';
    try { await fn(); } catch (e) { error = String(e); } finally { busy = false; }
  }
  async function choose(next: string) {
    mode = next; credentialId = ''; key = ''; step = 1; verified = false; agentId = ''; attempt = crypto.randomUUID(); models = []; model = ''; account = ''; loginOpened = false;
    await guard(async () => {
      if (mode === 'subscription') {
        const [detected, snapshot] = await Promise.all([invoke<CliDetected[]>('llm_accounts_detect'), invoke<AccountsSnapshot>('llm_accounts_list')]);
        clis = detected; accounts = snapshot.accounts;
      } else if (mode === 'api') { kinds = await invoke<Kind[]>('tool_keys_kinds'); provider = 'openai'; }
      else provider = 'ollama';
    });
  }
  async function connect() {
    persist();
    await guard(async () => {
      if (mode === 'subscription') {
        if (!account) {
          const snapshot = await invoke<AccountsSnapshot>('llm_accounts_create', { cli, label: name.trim() || cli, share: true, requestId: attempt });
          accounts = snapshot.accounts; account = `setup-${attempt}`; await loadAccounts();
        }
        await invoke('llm_accounts_login', { id: account }); loginOpened = true;
      } else {
        if (mode === 'api') {
          const kind = kinds.find(k => k.id === provider);
          if (!kind) throw new Error('Unknown provider');
          if (key) {
            credentialId ||= `setup-${attempt}`; persist();
            await invoke('tool_keys_save', { entry: { id: credentialId, name: name.trim() || kind.name, kind: provider, base_url: kind.base_url, key } });
            key = '';
          }
          const tested = await invoke<{ last_ok: boolean; error: string | null }>('tool_keys_test', { id: credentialId });
          if (!tested.last_ok) throw new Error(tested.error || 'Credential validation failed');
        }
        const response = await invoke<{ models: string[] }>('llm_models_list', { provider: mode === 'api' ? credentialId : provider, refresh: true });
        models = response.models ?? []; model = models[0] ?? '';
        if (!model) throw new Error(copy('Nenhum modelo disponível. Confira o serviço, a chave ou baixe um modelo em Local.', 'No models available. Check the service or key, or download a model in Local.'));
        step = 2;
      }
    });
  }
  function intendedAgent(): AgentDef {
        const selectedAccount = accounts.find(a => a.id === account);
        return {
          id: `connection-${attempt}`, name: name.trim() || (mode === 'subscription' ? selectedAccount?.label || cli : model), role: 'worker',
          system_prompt: '', tools: [], skills: [], budget: { tokens_per_turn: CONNECTION_TOKEN_LIMIT, max_tool_calls_per_turn: 0 },
          model: { policy: 'fixed', model: { provider: mode === 'subscription' ? (selectedAccount?.cli || cli) : mode === 'api' ? credentialId : provider, model: mode === 'subscription' ? 'default' : model } },
          runtime: mode === 'subscription' ? { kind: 'cli', cli: selectedAccount?.cli || cli, account } : { kind: 'native' },
        };
  }
  function matchesIntent(agent: AgentDef) {
    const expected = intendedAgent();
    return connectionMatchesIntent(agent, expected);
  }
  async function prepare() {
    await guard(async () => {
      const id = `connection-${attempt}`;
      persist();
      const roster = await invoke<AgentDef[]>('llm_roster_list');
      const existing = roster.find(a => a.id === id);
      if (existing && !matchesIntent(existing)) throw new Error(copy('O agente foi alterado. Recomece para preparar outro agente.', 'This agent changed. Start over to prepare another agent.'));
      if (!existing) {
        const agent = intendedAgent();
        await invoke('llm_roster_create', { agent });
      }
      agentId = id; await loadRoster(true); step = 3;
    });
  }
  async function test() {
    controller = new AbortController(); verified = false;
    await guard(async () => {
      const roster = await invoke<AgentDef[]>('llm_roster_list');
      const agent = roster.find(a => a.id === agentId);
      if (!agent || !matchesIntent(agent)) throw new Error(copy('A configuração mudou. Recomece para verificar outra conexão.', 'Configuration changed. Start over to verify another connection.'));
      await testConnection(agentId, controller!.signal); verified = true;
    });
  }
</script>

<section class="connection surface-card" aria-label={copy('Conectar minha IA', 'Connect my AI')}>
  <header><div><h2>{copy('Conectar minha IA', 'Connect my AI')}</h2><p>{copy('Use uma assinatura, uma chave API ou um modelo neste computador.', 'Use a subscription, an API key or a model on this computer.')}</p></div>
  {#if mode}<button class="button" disabled={busy} onclick={() => { mode = ''; step = 0; }}>{copy('Recomeçar', 'Start over')}</button>{/if}</header>
  {#if !mode}
    <div class="choices">
      {#each [['subscription', copy('Usar minha assinatura', 'Use my subscription'), 'Claude / Codex'], ['api', copy('Chave API', 'API key'), copy('Credencial do provedor', 'Provider credential')], ['local', copy('Modelo local', 'Local model'), 'Ollama / LM Studio / llama.cpp']] as choice}
        <button class="choice" onclick={() => choose(choice[0])}><strong>{choice[1]}</strong><span>{choice[2]}</span><span aria-hidden="true">→</span></button>
      {/each}
    </div>
  {:else}
    <ol class="steps" aria-label={copy('Progresso', 'Progress')}>
      {#each [copy('Escolher', 'Choose'), copy('Conectar', 'Connect'), copy('Preparar agente', 'Prepare agent'), copy('Testar e abrir', 'Test and open')] as label, i}
        <li class:current={step === i} class:done={step > i} aria-current={step === i ? 'step' : undefined}><span>{i + 1}</span>{label}</li>
      {/each}
    </ol>
    {#if step === 1}
      <label>{copy('Nome da conexão / agente', 'Connection / agent name')}<input class="input" bind:value={name} oninput={() => changed()} disabled={busy} /></label>
      {#if mode === 'subscription'}
        <p>{copy('A assinatura do chat não é crédito de API. O login acontece no CLI oficial, em um terminal visível.', 'A chat subscription is not API credit. Sign in through the official CLI in a visible terminal.')}</p>
        <label>{copy('Conta', 'Account')}<select class="input" bind:value={account} onchange={() => changed()} disabled={busy || loginOpened}><option value="">{copy('Nova conta isolada', 'New isolated account')}</option>{#each accounts.filter(a => !a.disabled) as a}<option value={a.id}>{a.label} · {a.cli}</option>{/each}</select></label>
        {#if !account}<label>CLI<select class="input" bind:value={cli} onchange={() => changed()} disabled={busy}><option>claude</option><option>codex</option></select></label>{/if}
        <p class="status">{clis.some(c => c.cli === (accounts.find(a => a.id === account)?.cli || cli)) ? copy('CLI instalado · login ainda não verificado', 'CLI installed · sign-in not yet verified') : copy('CLI não detectado. Instale o CLI oficial e tente novamente.', 'CLI not detected. Install the official CLI and try again.')}</p>
        <div class="actions"><button class="button" disabled={busy} onclick={() => guard(async () => { clis = await invoke('llm_accounts_detect'); })}>{copy('Detectar novamente', 'Detect again')}</button>
        <button class="button active" disabled={busy || !clis.some(c => c.cli === (accounts.find(a => a.id === account)?.cli || cli))} onclick={connect}>{copy('Abrir login no terminal', 'Open terminal sign-in')}</button></div>
        {#if loginOpened}<p role="status">{copy('Terminal aberto. Conclua o login lá; apenas o teste confirma a conexão.', 'Terminal opened. Finish signing in there; only the test confirms the connection.')}</p>{/if}
        {#if loginOpened || account}<button class="button" disabled={busy} onclick={() => step = 2}>{copy('Já fiz login · continuar', 'Signed in · continue')}</button>{/if}
      {:else}
        <label>{copy('Provedor', 'Provider')}<select class="input" bind:value={provider} onchange={() => changed(true)} disabled={busy}>{#if mode === 'api'}{#each kinds.filter(k => !['custom', 'newapi', 'ollama'].includes(k.id)) as k}<option value={k.id}>{k.name}</option>{/each}{:else}<option value="ollama">Ollama</option><option value="lmstudio">LM Studio</option><option value="llama-server">llama.cpp</option>{/if}</select></label>
        {#if mode === 'api'}<label>{copy('Chave API', 'API key')}<input class="input" type="password" autocomplete="off" bind:value={key} disabled={busy} /></label><p>{copy('A chave é salva no cofre do sistema. O teste consulta os modelos disponíveis.', 'The key is saved in the system vault. The test requests available models.')}</p>{:else}<p>{copy('Inicie o servidor local antes de verificar. A lista vem do serviço em execução.', 'Start the local server before checking. Models come from the running service.')}</p><a href="/llm/local">{copy('Abrir modelos locais', 'Open local models')}</a>{/if}
        <button class="button active" disabled={busy} onclick={connect}>{copy('Verificar conexão e modelos', 'Check connection and models')}</button>
      {/if}
    {:else if step === 2}
      <p>{copy('Será criado um agente sem ferramentas, com limite de contexto de 8.192 tokens por turno. Você pode ajustar permissões e limites depois em Agentes.', 'A new agent will have no tools and an 8,192-token context limit per turn. Adjust permissions and limits later in Agents.')}</p>
      {#if mode !== 'subscription'}<label>{copy('Modelo', 'Model')}<select class="input" bind:value={model} onchange={() => changed()}>{#each models as m}<option>{m}</option>{/each}</select></label>{/if}
      <div class="actions"><button class="button" disabled={busy} onclick={() => step = 1}>{copy('Voltar', 'Back')}</button><button class="button active" disabled={busy} onclick={prepare}>{copy('Preparar agente', 'Prepare agent')}</button></div>
    {:else}
      <p>{copy('O teste envia uma mensagem curta real, que pode consumir sua cota. Não concede permissões adicionais; o CLI mantém o sandbox da conta.', 'The test sends one real short message and may consume quota. It grants no additional permissions; the CLI retains the account sandbox.')}</p>
      <p role="status">{verified ? copy('Conexão verificada: o agente respondeu.', 'Connection verified: the agent responded.') : busy ? copy('Aguardando resposta real…', 'Waiting for a real response…') : copy('Agente preparado · resposta ainda não verificada', 'Agent prepared · response not yet verified')}</p>
      {#if mode === 'subscription'}<button class="button" disabled={busy} onclick={() => guard(async () => { await invoke('llm_accounts_login', { id: account }); verified = false; })}>{copy('Refazer login no terminal', 'Sign in again in terminal')}</button>{/if}
      <div class="actions"><button class="button active" disabled={busy} onclick={test}>{copy('Testar agente', 'Test agent')}</button>{#if busy}<button class="button" onclick={() => controller?.abort()}>{copy('Cancelar teste', 'Cancel test')}</button>{/if}{#if verified}<button class="button active" onclick={() => { selectAgent(agentId); goto('/llm'); }}>{copy('Abrir conversa', 'Open conversation')}</button>{/if}</div>
    {/if}
    {#if error}<p class="error" role="alert">{error}</p><p>{copy('Sua conta e seu agente são preservados. Tente novamente sem criar duplicatas.', 'Your account and agent are preserved. Retry without creating duplicates.')}</p>{/if}
    {#if busy && step !== 3}<p role="status">{copy('Conectando…', 'Connecting…')}</p>{/if}
  {/if}
</section>
<style>
  .connection { padding:24px; margin-bottom:24px; display:grid; gap:16px; }
  header,.actions { display:flex; align-items:center; justify-content:space-between; gap:12px; flex-wrap:wrap; }
  h2 { font-size:20px; margin:0 0 6px; } p { color:var(--text-muted); font-size:14px; max-width:75ch; margin:0; line-height:1.6; }
  .choices { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); gap:12px; }
  .choice { display:flex; flex-direction:column; align-items:flex-start; gap:8px; text-align:left; padding:20px; border:1px solid var(--separator); border-radius:12px; background:var(--fill-quaternary); color:var(--text); cursor:pointer; }
  .choice:hover { background:var(--fill-tertiary); } .choice span { color:var(--text-muted); font-size:13px; }
  label { display:grid; gap:6px; font-size:13px; max-width:480px; } .input { width:100%; }
  .steps { display:flex; list-style:none; padding:0; margin:0 0 8px; gap:16px; flex-wrap:wrap; }
  .steps li { display:flex; align-items:center; gap:8px; font-size:12px; color:var(--text-muted); }
  .steps li span { display:grid; place-items:center; width:26px; height:26px; border:1px solid var(--separator); border-radius:50%; }
  .steps .current { color:var(--text); font-weight:600; } .steps .current span,.steps .done span { background:var(--fill-secondary); }
  .error { color:var(--error, #b42318); overflow-wrap:anywhere; } .actions { justify-content:flex-start; }
  @media(max-width:650px) { .choices { grid-template-columns:1fr; } .connection { padding:16px; } }
</style>
