<script lang="ts">
  /**
   * The city on screen: one canvas, the region the avatar is in as a live
   * replica, the neighbouring exterior blocks as static scenery, and the
   * server as the only authority. Where `WorldCanvas` owns a house,
   * this only looks at one.
   *
   * Regions arrive from `/api/world/regions/{id}/map` and are baked into the
   * renderer's chunk cache; exterior blocks share one coordinate space, so
   * walking from one to the next shows no seam. An interior lives at its own
   * origin and overlaps the exterior on paper, so entering a home swaps the
   * drawable chunk set and rebakes it.
   */
  import { onMount } from "svelte";
  import { t } from "$lib/i18n";
  import { getSettings } from "$lib/stores/settings-store.svelte";
  import { codecErrorCode } from "$lib/world/codec";
  import { agentSprite, buildChunkTiles, loadWorldAtlas, objectFrame, type MapDef } from "$lib/world/assets";
  import { buildHud, capAgents, gameClockLabel, type AgentFrame } from "$lib/world/hud";
  import { attachInput, type Pickable } from "$lib/world/input";
  import { sample } from "$lib/world/interp";
  import { STATE_ERR, WorldState } from "$lib/world/state";
  import { createCitySession, fetchRegionMap, type CityControl, type CityFrame, type CityInput, type CityReady, type CitySession } from "$lib/world/city";
  import type { AtlasData } from "$lib/world/render/atlas";
  import type { Camera, ChunkId, ChunkTile, FrameStats, Renderer, Scene, SpriteInst, TextInst, Tier } from "$lib/world/render/types";

  interface Props {
    city: string;
    server?: string | null;
    onready?: (ready: CityReady) => void;
    onfailed?: (error: string) => void;
    onstate?: (state: { region: string; ent: number; tick: number; interior: boolean }) => void;
    onstats?: (stats: FrameStats, fps: number, tier: Tier) => void;
  }
  let { city, server = null, onready, onfailed, onstate, onstats }: Props = $props();

  /** A chat line over someone's head. */
  export function say(ent: number, text: string): void {
    const a = world.agents.get(ent);
    if (!a) return;
    a.saying = text.length > 80 ? `${text.slice(0, 79)}…` : text;
    a.sayingUntilMs = performance.now() + 6000;
  }

  /** Walk to my front door and go in (the panel's "go home"). */
  export async function goTo(tile: [number, number]): Promise<void> {
    await send({ type: "move", to: tile });
  }

  let canvas = $state<HTMLCanvasElement | null>(null);
  let status = $state<"loading" | "ready" | "error">("loading");
  let errorCode = $state("");
  let fps = $state(0);
  let tier = $state<Tier>(1);
  let region = $state("");
  let ent = $state(0);
  let connection = $state<"online" | "reconnecting">("online");

  /** The avatar's region: inputs, HUD and the clock come from here. */
  let world = new WorldState();
  /** Neighbouring blocks the server lets us watch: agents drawn, no input. */
  const observed = new Map<string, WorldState>();
  let renderer: Renderer | null = null;
  let loop: { start(): void; stop(): void } | null = null;
  let session: CitySession | null = null;
  let atlas: AtlasData | null = null;
  let camera: Camera = { x: 0, y: 0, zoom: 1, width: 960, height: 540 };
  let detachInput: (() => void) | null = null;
  let stops: Array<() => void> = [];
  let pickables: Pickable[] = [];
  let hovered = $state<number | null>(null);
  let selected = $state<number | null>(null);
  let clockLine = "";
  let projectFn: ((x: number, y: number, z?: number) => { px: number; py: number }) | null = null;
  let visibleChunksOf: ((cam: Camera) => ChunkId[]) | null = null;

  /** Region id → its map; `chunks` is the baked tile list per chunk id. */
  const maps = new Map<string, MapDef>();
  const regionChunks = new Map<string, Map<ChunkId, ChunkTile[]>>();
  /** Chunk ids the renderer may draw right now (exterior union, or one interior). */
  let drawable = new Set<ChunkId>();
  let interior = $state(false);
  let exteriorRegions: string[] = [];
  let lastInterestMs = 0;
  let lastInterestKey = "";

  function isInterior(id: string): boolean {
    return id.includes("/home:");
  }

  async function loadRegion(id: string): Promise<MapDef | null> {
    const cached = maps.get(id);
    if (cached) return cached;
    if (!session || !atlas) return null;
    try {
      const res = await fetchRegionMap(session, id);
      const map = res.map as MapDef;
      maps.set(id, map);
      regionChunks.set(id, buildChunkTiles(map, atlas));
      return map;
    } catch (e) {
      errorCode = codecErrorCode(e);
      return null;
    }
  }

  /** Make the set of chunks for the current mode and bake it. */
  function rebake(): void {
    if (!renderer) return;
    const next = new Set<ChunkId>();
    const ids = interior ? [region] : exteriorRegions.length ? exteriorRegions : [region];
    for (const rid of ids) {
      const chunks = regionChunks.get(rid);
      if (!chunks) continue;
      for (const [cid, tiles] of chunks) {
        renderer.bakeChunk(cid, tiles);
        next.add(cid);
      }
    }
    drawable = next;
  }

  async function enterRegion(id: string, at: [number, number]): Promise<void> {
    region = id;
    interior = isInterior(id);
    world = new WorldState();
    observed.clear();
    camera.x = at[0] + 0.5;
    camera.y = at[1] + 0.5;
    await loadRegion(id);
    if (!interior && exteriorRegions.length === 0 && session) {
      // The other blocks of the city, once: scenery for the horizon.
      try {
        const info = await session.api<{ regions: Array<{ id: string; kind: string }> }>("GET", `/api/world/cities/${encodeURIComponent(city)}`);
        exteriorRegions = info.regions.filter((r) => r.kind === "exterior").map((r) => r.id);
        await Promise.all(exteriorRegions.map((rid) => loadRegion(rid)));
      } catch {
        exteriorRegions = [id];
      }
    }
    rebake();
    onstate?.({ region, ent, tick: world.tick, interior });
  }

  function onFrame(frame: CityFrame): void {
    const nowMs = performance.now();
    if (frame.region !== region) {
      // A neighbouring block, or a straggler from a region we left. Only a
      // snapshot opens a replica; a diff for an unknown region is dropped.
      let state = observed.get(frame.region);
      if (!state) {
        if (frame.blob.kind !== "snapshot" || interior) return;
        state = new WorldState();
        observed.set(frame.region, state);
      }
      const r = frame.blob.kind === "snapshot" ? state.applySnapshot(frame.blob.snapshot, nowMs) : state.applyDiff(frame.blob.diff, nowMs);
      if (!r.ok && state.needsResync) observed.delete(frame.region); // the next snapshot reopens it
      return;
    }
    const result = frame.blob.kind === "snapshot" ? world.applySnapshot(frame.blob.snapshot, nowMs) : world.applyDiff(frame.blob.diff, nowMs);
    if (result.ok) {
      if (frame.blob.kind === "snapshot" && (errorCode === STATE_ERR.DIFF_GAP || errorCode === STATE_ERR.MAP_MISMATCH)) errorCode = "";
      return;
    }
    if (world.needsResync) void session?.resync().catch(() => (errorCode = result.code ?? ""));
  }

  function onControl(msg: CityControl): void {
    switch (msg.op) {
      case "region": {
        const m = msg as { region: string; at: [number, number] };
        void enterRegion(m.region, m.at);
        break;
      }
      case "reconnected": {
        const m = msg as CityReady;
        connection = "online";
        ent = m.ent;
        void enterRegion(m.region, m.spawn);
        break;
      }
      case "reconnecting":
        connection = "reconnecting";
        break;
      case "error": {
        const m = msg as { code: string; message: string };
        errorCode = m.code;
        status = "error";
        onfailed?.(`${m.code}: ${m.message}`);
        break;
      }
      default:
        break;
    }
  }

  async function send(input: CityInput): Promise<void> {
    try {
      await session?.send(input);
    } catch (e) {
      errorCode = codecErrorCode(e);
    }
  }

  /** The door marker on a tile of the current region, if any. */
  function portalAt(x: number, y: number): string | null {
    const map = maps.get(region);
    if (!map) return null;
    const m = map.markers?.find((k) => k.tile[0] === x && k.tile[1] === y && (k.name.startsWith("house-door:") || k.name === "front-door"));
    return m ? m.name : null;
  }

  function pushInterest(): void {
    const now = performance.now();
    const key = `${Math.round(camera.x)}:${Math.round(camera.y)}`;
    if (key === lastInterestKey || now - lastInterestMs < 400) return;
    lastInterestKey = key;
    lastInterestMs = now;
    void session?.setInterest(camera.x, camera.y, 40).catch(() => {});
  }

  async function boot(): Promise<void> {
    if (!canvas) return;
    status = "loading";
    try {
      await bootRender();
      session = createCitySession({ city, server });
      stops.push(session.onClosed((reason) => {
        if (reason !== "left") {
          errorCode = `ERR_CITY_${reason.toUpperCase()}`;
          status = "error";
          onfailed?.(errorCode);
        }
      }));
      const ready = await session.open(onFrame, onControl, (code) => (errorCode = code));
      ent = ready.ent;
      await enterRegion(ready.region, ready.spawn);
      status = "ready";
      onready?.(ready);
    } catch (e) {
      errorCode = String(e);
      status = "error";
      onfailed?.(String(e));
    }
  }

  async function bootRender(): Promise<void> {
    const mod = await import("$lib/world/render");
    visibleChunksOf = (cam) => mod.visibleChunks(cam);
    projectFn = mod.project;
    const override = getSettings()?.world?.tier_override;
    const measured = getSettings()?.world?.tier_measured;
    const pick = (v: unknown): Tier | null => (v === 0 || v === 1 || v === 2 || v === 3 ? v : null);
    tier = pick(override) ?? pick(measured) ?? 1;
    const r = mod.createRenderer();
    const caps = await r.init(canvas!, { tier });
    tier = caps.tier;
    const loaded = await loadWorldAtlas();
    r.loadAtlas(loaded.json, loaded.pages);
    atlas = mod.parseAtlas(loaded.json);
    const rect = canvas!.getBoundingClientRect();
    r.resize(rect.width, rect.height, window.devicePixelRatio || 1);
    camera.width = canvas!.width;
    camera.height = canvas!.height;
    renderer = r;
    loop = mod.createLoop(r, (dt) => buildScene(dt), {
      watchdog: pick(override) === null,
      onStats: (s, f) => {
        fps = f;
        clockLine = world.ready ? gameClockLabel(world.tick) : "";
        onstats?.(s, f, tier);
        pushInterest();
        if (import.meta.env.DEV) {
          (window as unknown as Record<string, unknown>).__omnigetCityStats = {
            fps: f,
            cpuMs: s.cpuMs,
            drawCalls: s.drawCalls,
            sprites: s.sprites,
            region,
            interior,
            ent,
            tick: world.tick,
            agents: world.agents.size,
            objects: world.objects.size,
            observed: [...observed.keys()],
            observedAgents: [...observed.values()].reduce((n, st) => n + st.agents.size, 0),
            drawable: drawable.size,
            status,
            errorCode,
            connection,
          };
        }
      },
      onTierChanged: (next) => (tier = next),
    });
    detachInput = attachInput(
      canvas!,
      { camera, pickables: () => pickables, dpr: () => (canvas!.width || 1) / (canvas!.getBoundingClientRect().width || 1) },
      {
        onTile: (tile) => {
          if (!world.ready) return;
          const portal = portalAt(tile.x, tile.y);
          if (portal) {
            void send({ type: "enter", portal });
            return;
          }
          void send({ type: "move", to: [tile.x, tile.y] });
        },
        onAgent: (id) => {
          selected = selected === id ? null : id;
          if (id !== ent) void send({ type: "wave", ent: id });
        },
        onHover: (id) => (hovered = id),
        onEscape: () => (selected = null),
      },
    );
    loop.start();
  }

  let visibleCache: ChunkId[] = [];
  function visibleChunkIds(): ChunkId[] {
    visibleCache.length = 0;
    if (!visibleChunksOf) return visibleCache;
    for (const id of visibleChunksOf(camera)) {
      if (drawable.has(id)) visibleCache.push(id);
    }
    return visibleCache;
  }

  function buildScene(_dt: number): Scene {
    const now = performance.now();
    camera.width = canvas?.width ?? camera.width;
    camera.height = canvas?.height ?? camera.height;
    const frames: AgentFrame[] = [];
    pickables = [];
    for (const agent of world.list()) {
      const p = sample(agent.track, now);
      frames.push({ agent, x: p.x, y: p.y, z: p.z });
      pickables.push({ id: agent.id, x: p.x, y: p.y });
    }
    // Neighbours: drawn and waved at, never the HUD's business.
    const farFrames: AgentFrame[] = [];
    for (const state of observed.values()) {
      for (const agent of state.list()) {
        const p = sample(agent.track, now);
        farFrames.push({ agent, x: p.x, y: p.y, z: p.z });
      }
    }
    const shown = capAgents(frames.concat(farFrames), tier, camera);
    const sprites: SpriteInst[] = [];
    if (atlas) {
      const objectLists = [world.objects.values(), ...[...observed.values()].map((s) => s.objects.values())];
      for (const list of objectLists) {
        for (const o of list) {
          const frame = objectFrame(atlas, o.kind);
          if (!frame) continue;
          sprites.push({ frame, x: o.tx + 0.5, y: o.ty + 0.5, z: 0 });
        }
      }
      for (const f of shown) {
        const sprite = agentSprite(atlas, f.agent.anim, f.agent.dir, now - f.agent.animStartedMs);
        sprites.push({ frame: sprite.frame, x: f.x, y: f.y, z: f.z, flip: sprite.flip, tint: f.agent.id === ent ? 0xfff2c8 : undefined });
      }
    }
    let texts: TextInst[] = [];
    if (renderer && atlas) {
      const hud = buildHud(world, shown, {
        tier,
        nowMs: now,
        camera,
        hovered,
        selected: selected ?? ent,
        dark: false,
        clockLine,
        scale: canvas ? camera.width / (canvas.clientWidth || camera.width) : 1,
        text: (str, style) => renderer!.text(str, style),
      });
      sprites.push(...hud.sprites);
      texts = hud.texts;
    }
    return { camera, chunksVisible: visibleChunkIds(), sprites, texts };
  }

  onMount(() => {
    void boot();
    const onResize = () => {
      if (!canvas || !renderer) return;
      const rect = canvas.getBoundingClientRect();
      renderer.resize(rect.width, rect.height, window.devicePixelRatio || 1);
    };
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      detachInput?.();
      loop?.stop();
      for (const stop of stops.splice(0)) stop();
      void session?.close().catch(() => {});
      session?.dispose();
      renderer?.destroy();
    };
  });
  void projectFn;
</script>

<div class="stage">
  <canvas bind:this={canvas} class="canvas" width="960" height="540" aria-label={$t("world.city.canvas_label") as string}></canvas>
  <div class="overlay">
    {#if status === "loading"}
      <span class="badge">{$t("world.city.entering")}</span>
    {:else if status === "error"}
      <span class="badge error">{$t("world.error")} {errorCode}</span>
    {:else}
      <span class="badge">{interior ? $t("world.city.inside") : $t("world.city.outside")} · {region.split("/").pop()} · {fps} fps</span>
      {#if connection === "reconnecting"}<span class="badge warn">{$t("world.city.reconnecting")}</span>{/if}
      {#if errorCode}<span class="badge warn">{errorCode}</span>{/if}
    {/if}
  </div>
</div>

<style>
  .stage {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 9;
    border-radius: 12px;
    overflow: hidden;
    background: #10131a;
  }
  .canvas {
    width: 100%;
    height: 100%;
    display: block;
    touch-action: none;
  }
  .overlay {
    position: absolute;
    left: 0.6rem;
    top: 0.6rem;
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
    pointer-events: none;
  }
  .badge {
    font-size: 0.75rem;
    padding: 0.2rem 0.55rem;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.55);
    color: #fff;
  }
  .badge.error {
    background: rgba(200, 40, 40, 0.8);
  }
  .badge.warn {
    background: rgba(200, 140, 20, 0.85);
  }
</style>
