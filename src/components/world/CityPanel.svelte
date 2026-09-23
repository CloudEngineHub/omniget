<script lang="ts">
  // Enter the city, claim a plot, talk to whoever is near. Nothing here talks
  // to the network until a button is pressed.
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { t } from "$lib/i18n";
  import { getSettings } from "$lib/stores/settings-store.svelte";
  import { showToast } from "$lib/stores/toast-store.svelte";
  import type { CityChat } from "$lib/world/city";

  interface Props {
    /** The city the canvas is showing, or null when at home. */
    city: { city: string; server: string | null } | null;
    where: { region: string; ent: number; tick: number; interior: boolean } | null;
    onenter: (city: { city: string; server: string | null }) => void;
    onleave: () => void;
    onsay?: (ent: number, text: string) => void;
    ongoto?: (tile: [number, number]) => void;
    /** The crop a click on an empty bed plants; the canvas reads it. */
    crop?: string;
    oncrop?: (crop: string) => void;
  }
  let { city, where, onenter, onleave, onsay, ongoto, crop = "carrot", oncrop }: Props = $props();

  type Home = { home: { id: string; name: string; plot: string; interior_region: string; inventory?: Record<string, number> }; plot: { id: string; address: string; entrance: [number, number]; region: string; kind?: string } | null };
  const CROPS = ["carrot", "tomato", "wheat"];

  let cityId = $state("cidade-piloto");
  let server = $state(getSettings()?.world?.city_server ?? "");
  let showServer = $state(false);
  let signedIn = $state<boolean | null>(null);
  let username = $state("");
  let password = $state("");
  let creating = $state(false);
  let busy = $state(false);
  let home = $state<Home | null>(null);
  let chat = $state<CityChat[]>([]);
  let draft = $state("");

  async function checkSession(): Promise<void> {
    try {
      const r = (await invoke("city_session", { server: server.trim() || null })) as { signed_in: boolean };
      signedIn = r.signed_in;
    } catch {
      signedIn = false;
    }
  }

  async function signIn(): Promise<void> {
    const u = username.trim();
    if (!u || !password) return;
    busy = true;
    try {
      await invoke(creating ? "city_register" : "city_login", { username: u, password, server: server.trim() || null });
      password = "";
      signedIn = true;
    } catch (error) {
      showToast("error", String(error));
    } finally {
      busy = false;
    }
  }

  async function signOut(): Promise<void> {
    try {
      await invoke("city_logout", { server: server.trim() || null });
    } catch {
      // Nothing to forget.
    }
    signedIn = false;
    onleave();
  }

  onMount(() => {
    const stops: Array<() => void> = [];
    void checkSession();
    void (async () => {
      stops.push(
        await listen<CityChat>("city://chat", (e) => {
          chat = [...chat.slice(-49), e.payload];
          onsay?.(e.payload.from_ent, e.payload.text);
        }),
      );
    })();
    return () => stops.forEach((stop) => stop());
  });

  $effect(() => {
    if (city) void loadHome();
    else {
      home = null;
      chat = [];
    }
  });

  /** The canvas tells the page a farm action landed; refresh the inventory. */
  export function refreshHome(): void {
    void loadHome();
  }

  async function loadHome(): Promise<void> {
    if (!city) return;
    try {
      home = (await invoke("city_api", { method: "GET", path: `/api/world/cities/${encodeURIComponent(city.city)}/homes/@me`, body: null, server: city.server })) as Home;
    } catch {
      home = null;
    }
  }

  function enter(): void {
    const id = cityId.trim();
    if (!id) return;
    onenter({ city: id, server: server.trim() || null });
  }

  async function leave(): Promise<void> {
    try {
      await invoke("city_leave");
    } catch {
      // Already gone.
    }
    onleave();
  }

  async function claim(): Promise<void> {
    if (!city) return;
    busy = true;
    try {
      home = (await invoke("city_api", { method: "POST", path: `/api/world/cities/${encodeURIComponent(city.city)}/claim`, body: {}, server: city.server })) as Home;
      showToast("success", $t("world.city.claimed", { address: home.plot?.address ?? "" }) as string);
    } catch (error) {
      showToast("error", String(error));
    } finally {
      busy = false;
    }
  }

  function goHome(): void {
    const e = home?.plot?.entrance;
    if (!e) return;
    ongoto?.([e[0], e[1] + 1]);
  }

  async function sendChat(): Promise<void> {
    const text = draft.trim();
    if (!text) return;
    draft = "";
    try {
      await invoke("city_chat", { text });
    } catch (error) {
      showToast("error", String(error));
    }
  }
</script>

<section class="city">
  {#if !city}
    {#if signedIn === false}
      <form class="row" onsubmit={(e) => { e.preventDefault(); void signIn(); }}>
        <span class="label">{$t("world.city.sign_in")}</span>
        <input type="text" bind:value={username} maxlength="32" autocomplete="username" placeholder={$t("world.city.username") as string} />
        <input type="password" bind:value={password} maxlength="256" autocomplete="current-password" placeholder={$t("world.city.password") as string} />
        <button type="submit" class="btn primary" disabled={busy || !username.trim() || !password}>{creating ? $t("world.city.create_account") : $t("world.city.sign_in")}</button>
        <button type="button" class="link" onclick={() => (creating = !creating)}>{creating ? $t("world.city.have_account") : $t("world.city.no_account")}</button>
        <button type="button" class="link" onclick={() => (showServer = !showServer)}>{$t("world.city.server")}</button>
        {#if showServer}
          <input type="text" class="server" bind:value={server} placeholder="https://chat.tonho.wtf" title={$t("world.city.server_hint") as string} onchange={() => void checkSession()} />
        {/if}
      </form>
    {:else}
      <form class="row" onsubmit={(e) => { e.preventDefault(); enter(); }}>
        <span class="label">{$t("world.city.title")}</span>
        <input type="text" bind:value={cityId} maxlength="64" placeholder="cidade-piloto" />
        <button type="submit" class="btn primary" disabled={signedIn !== true}>{$t("world.city.enter")}</button>
        <button type="button" class="link" onclick={() => (showServer = !showServer)}>{$t("world.city.server")}</button>
        {#if showServer}
          <input type="text" class="server" bind:value={server} placeholder="https://chat.tonho.wtf" title={$t("world.city.server_hint") as string} onchange={() => void checkSession()} />
        {/if}
        {#if signedIn}<button type="button" class="link" onclick={signOut}>{$t("world.city.sign_out")}</button>{/if}
      </form>
    {/if}
    <p class="hint">{$t("world.city.hint")}</p>
  {:else}
    <div class="row">
      <span class="dot on" aria-hidden="true"></span>
      <span class="label">{$t("world.city.in", { city: city.city })}</span>
      {#if where}<span class="chip">{where.interior ? $t("world.city.inside") : $t("world.city.outside")} · {where.region.split("/").pop()}</span>{/if}
      <button type="button" class="btn" onclick={leave}>{$t("world.city.leave")}</button>
    </div>
    <div class="row">
      {#if home?.plot}
        <span class="label">{$t("world.city.address")}</span>
        <span class="chip host">{home.plot.address}</span>
        <button type="button" class="btn" onclick={goHome}>{$t("world.city.go_home")}</button>
        {#if home.home.inventory && Object.keys(home.home.inventory).length > 0}
          <span class="chip" title={$t("world.city.inventory") as string}>{Object.entries(home.home.inventory).map(([k, v]) => `${$t(`world.city.crop_${k}`)} ×${v}`).join(" · ")}</span>
        {/if}
        <label class="label">{$t("world.city.plant")}
          <select value={crop} onchange={(e) => oncrop?.((e.currentTarget as HTMLSelectElement).value)}>
            {#each CROPS as c}<option value={c}>{$t(`world.city.crop_${c}`)}</option>{/each}
          </select>
        </label>
      {:else}
        <button type="button" class="btn primary" onclick={claim} disabled={busy}>{$t("world.city.claim")}</button>
        <span class="hint">{$t("world.city.claim_hint")}</span>
      {/if}
    </div>
    <div class="chat">
      <div class="chat-log">
        {#each chat as line (line.ts + ":" + line.from_ent)}
          <p><strong>{line.name}</strong> {line.text}</p>
        {/each}
      </div>
      <form class="chat-form" onsubmit={(e) => { e.preventDefault(); void sendChat(); }}>
        <input type="text" bind:value={draft} maxlength="500" placeholder={$t("world.city.chat_placeholder") as string} />
        <button type="submit" class="btn" disabled={!draft.trim()}>{$t("world.house.send")}</button>
      </form>
    </div>
  {/if}
</section>

<style>
  .city {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    max-width: 960px;
  }
  .row,
  .chat-form {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .btn {
    padding: 0.35rem 0.8rem;
    border-radius: 8px;
    border: 1px solid rgba(127, 127, 127, 0.3);
    background: rgba(127, 127, 127, 0.12);
    color: inherit;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .btn.primary {
    background: var(--accent, #0a84ff);
    border-color: transparent;
    color: #fff;
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  select {
    padding: 0.3rem 0.5rem;
    border-radius: 8px;
    border: 1px solid rgba(127, 127, 127, 0.3);
    background: rgba(127, 127, 127, 0.08);
    color: inherit;
    font: inherit;
    font-size: 0.85rem;
    margin-left: 0.3rem;
  }
  input {
    padding: 0.35rem 0.6rem;
    border-radius: 8px;
    border: 1px solid rgba(127, 127, 127, 0.3);
    background: rgba(127, 127, 127, 0.08);
    color: inherit;
    font: inherit;
    font-size: 0.85rem;
  }
  .server {
    width: 18rem;
    font-family: ui-monospace, monospace;
  }
  .chat-form input {
    flex: 1;
    min-width: 0;
  }
  .link {
    background: none;
    border: 0;
    color: inherit;
    opacity: 0.6;
    font-size: 0.8rem;
    cursor: pointer;
    text-decoration: underline;
  }
  .hint,
  .label {
    font-size: 0.85rem;
    opacity: 0.7;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #30d158;
    display: inline-block;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.15rem 0.6rem;
    border-radius: 999px;
    background: rgba(127, 127, 127, 0.14);
    font-size: 0.8rem;
  }
  .chip.host {
    font-weight: 600;
  }
  .chat-log {
    max-height: 9rem;
    overflow-y: auto;
    font-size: 0.85rem;
  }
  .chat-log p {
    margin: 0.15rem 0;
  }
</style>
