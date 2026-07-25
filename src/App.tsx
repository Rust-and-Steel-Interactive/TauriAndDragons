import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

interface Item {
  instance_id: string;
  template_id: string;
  display_name: string;
  description: string | null;
  item_class: string;
  rarity: number;
  weight: number;
  gp_value: number;
  damage_dice: string | null;
  damage_bonus: number | null;
  weapon_type: string | null;
  armor_slot: string | null;
  armour_category: string | null;
  dex_cap: number | null;
  ac_bonus: number | null;
  effect: string | null;
  is_quest_item: boolean;
  quantity: number;
  placed_x: number | null;
  placed_y: number | null;
  handedness: string | null;
}

type TileType = "Floor" | "Wall" | "Rubble" | "Water" | "Door" | "Stairs" | "Empty";
type TileVisibility = "Unknown" | "Explored" | "Visible";
type AwarenessState = "Unaware" | "Suspicious" | "Alert" | "Searching";

interface Tile {
  x: number;
  y: number;
  tile_type: TileType;
  visibility: TileVisibility;
  ground_light_source: ActiveLightSource | null;
}

interface ActiveLightSource {
  item_id: string;
  radius: number;
  remaining_turns: number;
  is_belt_mounted: boolean;
}

interface Player {
  name: string;
  hp: number;
  max_hp: number;
  ac: number;
  gp: number;
  strength: number;
  dexterity: number;
  constitution: number;
  intelligence: number;
  wisdom: number;
  charisma: number;
  proficiency_bonus: number;
  inventory: Item[];
  primary_hand: string | null;
  secondary_hand: string | null;
  equipped_armour: Item[];
  thieves_tools_proficiency: boolean;
  speed: number;
  x: number;
  y: number;
  active_light_source: ActiveLightSource | null;
  equipped_belt: Item | null;
  utility_slots: (Item | null)[];
}

interface ContainerParts {
  lock_status: string;
  condition: string;
  accent_material: string;
  core_material: string;
  container_type: string;
}

interface Chest {
  id: string;
  name: string;
  locked: boolean;
  dc: number;
  break_chance: number;
  broken: boolean;
  loot: Item[];
  parts: ContainerParts;
  is_revealed: boolean;
}

interface Trap {
  id: string;
  name: string;
  dc: number;
  damage: string;
  damage_type: string;
}

interface Room {
  id: string;
  name: string;
  description: string;
  connections: string[];
  traps: Trap[];
  enemies: Enemy[];
  loot: Item[];
  chests: Chest[];
  hidden_caches: Item[];
  is_looted: boolean;
  is_trap_triggered: boolean;
  visited: boolean;
  tiles: Tile[][];
  tile_width: number;
  tile_height: number;
  entrance_x: number;
  entrance_y: number;
}

interface Enemy {
  id: string;
  template_id: string;
  name: string;
  hp: number;
  max_hp: number;
  ac: number;
  strength: number;
  dexterity: number;
  constitution: number;
  intelligence: number;
  wisdom: number;
  charisma: number;
  damage_dice: string;
  attack_bonus: number;
  xp: number;
  studied: boolean;
  equipped_armour: Item[];
  perks: string[];
  speed: number;
  x: number;
  y: number;
  awareness: AwarenessState;
}

interface CampaignInfo {
  id: string;
  name: string;
  description: string;
}

interface LootGroup {
  source_name: string;
  gp: number;
  items: Item[];
}

interface CombatResources {
  has_action: boolean;
  has_bonus_action: boolean;
  has_reaction: boolean;
  remaining_movement_ft: number;
  is_dodging: boolean;
  is_disengaging: boolean;
  has_readied_action: boolean;
}

interface CombatLogEntry {
  round: number;
  actor: string;
  text: string;
}

interface InitiativeEntry {
  id: string;
  roll: number;
  bonus: number;
  name: string;
}

interface SessionState {
  player: Player;
  current_room_id: string;
  game_mode: "Exploration" | "Combat" | "GameOver";
  last_roll: string;
  available_actions: string[];
  rooms: Room[];
  campaign_name: string;
  last_loot: LootGroup[];
  combat_resources: Record<string, CombatResources>;
  combat_log: CombatLogEntry[];
  round_number: number;
  current_turn_index: number;
  initiative_entries: InitiativeEntry[];
}

const isInventoryAction = (action: string) => action.startsWith("USE_ITEM_") || action.startsWith("EQUIP_ITEM_") || action.startsWith("EQUIP_ARMOUR_") || action.startsWith("UNEQUIP_ARMOUR_") || action.startsWith("EQUIP_BELT_") || action === "UNEQUIP_BELT" || action.startsWith("MOUNT_UTILITY_") || action.startsWith("UNMOUNT_UTILITY_") || action.startsWith("REFILL_LANTERN_") || action.startsWith("PICK_UP_TORCH_") || action === "UNEQUIP_HAND_PRIMARY" || action === "UNEQUIP_HAND_SECONDARY";
const abilMod = (score: number) => { const m = Math.floor((score - 10) / 2); return m >= 0 ? `+${m}` : `${m}`; };

function MainMenu({ onStart }: { onStart: (campaignId: string) => void }) {
  const [campaigns, setCampaigns] = useState<CampaignInfo[]>([]);
  const [hasSave, setHasSave] = useState(false);

  useEffect(() => {
    invoke<CampaignInfo[]>("list_campaigns").then(setCampaigns).catch(console.error);
    invoke<boolean>("load_save_exists").then(setHasSave).catch(console.error);
  }, []);

  return (
    <div className="main-menu">
      <div className="menu-header">
        <h1>Tauri &amp; Dragons</h1>
        <p className="subtitle">A Text RPG with LLM Narration</p>
      </div>
      <div className="campaign-grid">
        {campaigns.map((c) => (
          <div key={c.id} className="campaign-card" onClick={() => onStart(c.id)}>
            <h3>{c.name}</h3>
            <p>{c.description}</p>
            <span className="card-hint">Click to start</span>
          </div>
        ))}
      </div>
      {hasSave && (
        <div className="continue-section">
          <p>A saved game exists</p>
          <button className="continue-btn" onClick={() => onStart("")}>Continue</button>
        </div>
      )}
    </div>
  );
}

function App() {
  const [screen, setScreen] = useState<"menu" | "game">("menu");
  const [state, setState] = useState<SessionState | null>(null);
  const [narration, setNarration] = useState<string>("");
  const [systemText, setSystemText] = useState<string>("");
  const [inputText, setInputText] = useState<string>("");
  const [isStreaming, setIsStreaming] = useState<boolean>(false);
  
  // UI Lock & Cached Actions
  const [isLocked, setIsLocked] = useState(false);
  const [lastTurnActions, setLastTurnActions] = useState<string[]>([]);
  const [lastInventoryActions, setLastInventoryActions] = useState<string[]>([]);

  const [dicePopUp, setDicePopUp] = useState<string | null>(null);
  const [engineStatus, setEngineStatus] = useState<string>("Starting Gemma...");

  const handleActionRef = useRef<((id: string) => void) | null>(null);

  const handleStart = useCallback(async (campaignId: string) => {
    if (campaignId) {
      try {
        await invoke("start_game", { campaignId });
        await invoke("delete_save");
      } catch (e) {
        console.error(e);
        return;
      }
    }
    const gameState = await invoke<SessionState>("get_game_state");
    setState(gameState);
    setNarration(gameState.rooms.find(r => r.id === gameState.current_room_id)?.description || "The adventure begins...");
    setScreen("game");
  }, []);

  useEffect(() => {
    if (screen !== "game") return;

    const unlistenStart = listen("llm-start", () => {
      setNarration("");
      setSystemText("");
      setIsStreaming(true);
    });

    const unlistenToken = listen<string>("llm-token", (event) => {
      setNarration((prev) => prev + event.payload);
    });
    
    const unlistenDone = listen("llm-done", () => setIsStreaming(false));
    
    const unlistenState = listen<SessionState>("state-updated", (event) => {
      const newState = event.payload;
      setState(newState);
      
      if (newState.game_mode === "GameOver") {
        setIsLocked(false);
      } else if (newState.available_actions.length > 0) {
        setLastTurnActions(newState.available_actions.filter(a => !isInventoryAction(a)));
        setLastInventoryActions(newState.available_actions.filter(a => isInventoryAction(a)));
        setIsLocked(false);
      } else if (newState.game_mode === "Combat") {
        setIsLocked(true);
      }
    });

    const unlistenStatus = listen<string>("gemma-status", (event) => {
      setEngineStatus(event.payload);
    });

    const unlistenProgress = listen<{ downloaded_bytes: number, total_bytes: number | null }>("gemma-download-progress", () => {});

    const unlistenDice = listen<string>("dice-rolled", (event) => {
      setDicePopUp(event.payload);
      setSystemText((prev) => prev + `\n[ ⚔️ Engine Roll: ${event.payload} ]\n`);
    });

    const unlistenSystemMsg = listen<string>("system-message", (event) => {
      setSystemText((prev) => prev + `\n[ System: ${event.payload} ]\n`);
      setIsStreaming(false);
      setIsLocked(false);
    });

    const unlistenDmChoose = listen<string>("dm-choose", (event) => {
      handleActionRef.current?.(event.payload);
    });

    return () => {
      unlistenStart.then((fn) => fn());
      unlistenToken.then((fn) => fn());
      unlistenDone.then((fn) => fn());
      unlistenState.then((fn) => fn());
      unlistenStatus.then((fn) => fn());
      unlistenProgress.then((fn) => fn());
      unlistenDice.then((fn) => fn());
      unlistenDmChoose.then((fn) => fn());
      unlistenSystemMsg.then((fn) => fn());
    };
  }, [screen]);

  const handleSubmit = async () => {
    if (!inputText.trim() || isStreaming || isLocked) return;
    setIsLocked(true);
    const prompt = inputText;
    setInputText("");
    try {
      await invoke("generate_narration", { prompt });
    } catch (e) {
      console.error(e);
      setIsStreaming(false);
      setIsLocked(false);
    }
  };

  const handleButtonClick = async (actionId: string) => {
    if (isStreaming || isLocked) return;
    if (!actionId.startsWith("EQUIP_ITEM_") && !actionId.startsWith("EQUIP_ARMOUR_") && !actionId.startsWith("UNEQUIP_ARMOUR_") && !actionId.startsWith("TAKE_ITEM_") && !actionId.startsWith("SEARCH_AREA") && !actionId.startsWith("MOVE_") && !actionId.startsWith("OPEN_CHEST_") && !actionId.startsWith("PICK_LOCK_")) {
      setIsLocked(true);
    }
    try {
      await invoke("player_button_action", { actionId });
    } catch (e) {
      console.error(e);
      setIsStreaming(false);
      setIsLocked(false);
    }
  };
  handleActionRef.current = handleButtonClick;

  const formatActionName = (action: string) => {
    if (action === "MOVE_NORTH") return "▲ North";
    if (action === "MOVE_SOUTH") return "▼ South";
    if (action === "MOVE_EAST") return "▶ East";
    if (action === "MOVE_WEST") return "◀ West";
    if (action.startsWith("USE_ITEM_")) {
      const itemId = action.replace("USE_ITEM_", "");
      const item = state?.player.inventory.find(i => i.instance_id === itemId);
      if (item?.template_id === "lantern") return `Ignite ${item.display_name}`;
      return `Use ${item?.display_name || itemId}`;
    }
    if (action.startsWith("TAKE_ITEM_")) {
      const itemId = action.replace("TAKE_ITEM_", "");
      const item = state?.rooms.find(r => r.id === state.current_room_id)?.loot.find(i => i.instance_id === itemId);
      return `Take ${item?.display_name || itemId}`;
    }
    if (action.startsWith("EQUIP_ITEM_")) {
      const itemId = action.replace("EQUIP_ITEM_", "");
      const item = state?.player.inventory.find(i => i.instance_id === itemId);
      return `Equip ${item?.display_name || itemId}`;
    }
    if (action.startsWith("EQUIP_ARMOUR_")) {
      const itemId = action.replace("EQUIP_ARMOUR_", "");
      const item = state?.player.inventory.find(i => i.instance_id === itemId);
      return `Wear ${item?.display_name || itemId}`;
    }
    if (action.startsWith("UNEQUIP_ARMOUR_")) {
      const itemId = action.replace("UNEQUIP_ARMOUR_", "");
      const item = state?.player.equipped_armour.find(i => i.instance_id === itemId);
      return `Remove ${item?.display_name || itemId}`;
    }
    if (action.startsWith("PICK_UP_TORCH_")) {
      return "🔥 Pick Up Torch";
    }
    if (action.startsWith("REFILL_LANTERN_")) {
      const lanternId = action.replace("REFILL_LANTERN_", "");
      const item = state?.player.inventory.find(i => i.instance_id === lanternId);
      return `Refill ${item?.display_name || "Lantern"}`;
    }
    if (action.startsWith("EQUIP_BELT_")) {
      const itemId = action.replace("EQUIP_BELT_", "");
      const item = state?.player.inventory.find(i => i.instance_id === itemId);
      return `Equip ${item?.display_name || "Belt"}`;
    }
    if (action === "UNEQUIP_BELT") return "Remove Belt";
    if (action === "UNEQUIP_HAND_PRIMARY") return "Stow Main Hand";
    if (action === "UNEQUIP_HAND_SECONDARY") return "Stow Off Hand";
    if (action.startsWith("MOUNT_UTILITY_")) {
      const parts = action.split("_");
      const slotIdx = parseInt(parts[2]);
      const itemId = parts.slice(3).join("_");
      const item = state?.player.inventory.find(i => i.instance_id === itemId);
      return `Mount ${item?.display_name || itemId} to slot ${slotIdx + 1}`;
    }
    if (action.startsWith("UNMOUNT_UTILITY_")) {
      const slotIdx = parseInt(action.replace("UNMOUNT_UTILITY_", ""));
      const item = state?.player.utility_slots[slotIdx];
      return `Unmount ${item?.display_name || "item"} from slot ${slotIdx + 1}`;
    }
    if (action.startsWith("ACTION_ATTACK_")) {
      const enemyId = action.replace("ACTION_ATTACK_", "");
      const enemy = state?.rooms.find(r => r.id === state.current_room_id)?.enemies.find(e => e.id === enemyId);
      return `Attack ${enemy?.name || enemyId}`;
    }
    if (action.startsWith("BONUS_OFFHAND_ATTACK_")) {
      const enemyId = action.replace("BONUS_OFFHAND_ATTACK_", "");
      const enemy = state?.rooms.find(r => r.id === state.current_room_id)?.enemies.find(e => e.id === enemyId);
      return `Off-hand Attack ${enemy?.name || enemyId}`;
    }
    if (action.startsWith("ACTION_STUDY_")) {
      const enemyId = action.replace("ACTION_STUDY_", "");
      const enemy = state?.rooms.find(r => r.id === state.current_room_id)?.enemies.find(e => e.id === enemyId);
      return `Study ${enemy?.name || enemyId}`;
    }
    if (action === "ACTION_DASH") return "Dash (+movement)";
    if (action === "ACTION_DODGE") return "Dodge (+defense)";
    if (action === "ACTION_DISENGAGE") return "Disengage (safe retreat)";
    if (action === "ACTION_HIDE") return "Hide";
    if (action === "ACTION_READY") return "Ready Action";
    if (action === "ACTION_FLEE") return "Flee";
    if (action === "ACTION_END_TURN") return "End Turn";
    if (action === "SEARCH_AREA") return "Search Area";
    if (action.startsWith("OPEN_CHEST_")) {
      const chestId = action.replace("OPEN_CHEST_", "");
      const chest = state?.rooms.find(r => r.id === state.current_room_id)?.chests.find(c => c.id === chestId);
      return `Open ${chest?.name || "Chest"}`;
    }
    if (action.startsWith("PICK_LOCK_")) {
      const chestId = action.replace("PICK_LOCK_", "");
      const chest = state?.rooms.find(r => r.id === state.current_room_id)?.chests.find(c => c.id === chestId);
      return `Pick Lock (${chest?.name || "Chest"})`;
    }
    if (action.startsWith("ATTACK_")) {
      const enemyId = action.replace("ATTACK_", "");
      const enemy = state?.rooms.find(r => r.id === state.current_room_id)?.enemies.find(e => e.id === enemyId);
      return `Attack ${enemy?.name || enemyId}`;
    }
    if (action.startsWith("STUDY_")) {
      const enemyId = action.replace("STUDY_", "");
      const enemy = state?.rooms.find(r => r.id === state.current_room_id)?.enemies.find(e => e.id === enemyId);
      return `Study ${enemy?.name || enemyId}`;
    }
    if (action === "FLEE") return "Flee";
    return action.split("_")
      .map(word => word.charAt(0) + word.slice(1).toLowerCase())
      .join(" ");
  };

  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const graphCanvasRef = useRef<HTMLCanvasElement>(null);

  const TILE_COLORS: Record<TileType, Record<TileVisibility, string>> = {
    Floor: { Unknown: "#111", Explored: "#1a2a1a", Visible: "#2a4a2a" },
    Wall: { Unknown: "#111", Explored: "#2a2a3e", Visible: "#4a4a6e" },
    Rubble: { Unknown: "#111", Explored: "#2a2a1a", Visible: "#4a4a2a" },
    Water: { Unknown: "#111", Explored: "#0a1a2a", Visible: "#1a3a5a" },
    Door: { Unknown: "#111", Explored: "#3a2a1a", Visible: "#6a4a2a" },
    Stairs: { Unknown: "#111", Explored: "#2a1a3a", Visible: "#4a2a6a" },
    Empty: { Unknown: "#111", Explored: "#111", Visible: "#111" },
  };

  useEffect(() => {
    const container = containerRef.current;
    const canvas = canvasRef.current;
    if (!canvas || !container || !state) return;

    const ctx = canvas.getContext("2d")!;
    let curDpr = 1;

    const draw = (w: number, h: number) => {
      const room = state.rooms.find(r => r.id === state.current_room_id);
      if (!room || !room.tiles || room.tiles.length === 0) return;

      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, canvas.width, canvas.height);

      const tileSize = Math.min(
        (w - 8) / room.tile_width,
        (h - 8) / room.tile_height,
        16
      );
      const gridW = room.tile_width * tileSize;
      const gridH = room.tile_height * tileSize;
      const offsetX = (w - gridW) / 2;
      const offsetY = (h - gridH) / 2;

      ctx.setTransform(curDpr, 0, 0, curDpr, 0, 0);

      // Draw tiles
      for (const row of room.tiles) {
        for (const tile of row) {
          const x = offsetX + tile.x * tileSize;
          const y = offsetY + tile.y * tileSize;
          const colorMap = TILE_COLORS[tile.tile_type];
          const color = colorMap ? colorMap[tile.visibility] : "#111";
          ctx.fillStyle = color;
          ctx.fillRect(x, y, tileSize, tileSize);

          if (tile.visibility !== "Unknown") {
            ctx.strokeStyle = "rgba(255,255,255,0.05)";
            ctx.strokeRect(x, y, tileSize, tileSize);
          }
        }
      }

      // Draw ground torch flames
      for (const row of room.tiles) {
        for (const tile of row) {
          if (tile.ground_light_source && tile.visibility !== "Unknown") {
            const cx = offsetX + tile.x * tileSize + tileSize / 2;
            const cy = offsetY + tile.y * tileSize + tileSize / 2;
            ctx.font = `${tileSize * 0.7}px serif`;
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";
            ctx.fillStyle = "#ff8800";
            ctx.fillText("🔥", cx, cy);
          }
        }
      }

      // Draw enemies (only if on a visible tile)
      for (const enemy of room.enemies) {
        if (enemy.hp <= 0) continue;
        const tileVis = room.tiles[enemy.y]?.[enemy.x]?.visibility;
        if (tileVis !== "Visible") continue;
        const ex = offsetX + enemy.x * tileSize + tileSize / 2;
        const ey = offsetY + enemy.y * tileSize + tileSize / 2;
        ctx.beginPath();
        ctx.arc(ex, ey, tileSize * 0.3, 0, Math.PI * 2);
        ctx.fillStyle = "#ff4444";
        ctx.fill();
        ctx.strokeStyle = "#ff0000";
        ctx.lineWidth = 1;
        ctx.stroke();
      }

      // Draw loot items (only if on a non-Unknown tile)
      for (const item of room.loot) {
        if (item.placed_x == null || item.placed_y == null) continue;
        const lx = item.placed_x;
        const ly = item.placed_y;
        const tileVis = room.tiles[ly]?.[lx]?.visibility;
        if (tileVis === "Unknown") continue;
        const lpx = offsetX + lx * tileSize + tileSize / 2;
        const lpy = offsetY + ly * tileSize + tileSize / 2;
        ctx.fillStyle = "#44dd44";
        ctx.font = `${tileSize * 0.5}px sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText("●", lpx, lpy);
      }

      // Draw chests (only if revealed)
      for (const chest of room.chests) {
        if (chest.broken || !chest.is_revealed) continue;
        const [cx, cy] = chest.name.match(/\[(\d+):(\d+)\]/) 
          ? [parseInt(chest.name.match(/\[(\d+)/)![1]), parseInt(chest.name.match(/:(\d+)\]/)![1])]
          : [0, 0];
        const cpx = offsetX + cx * tileSize + tileSize / 2;
        const cpy = offsetY + cy * tileSize + tileSize / 2;
        ctx.fillStyle = "#d4af37";
        ctx.font = `${tileSize * 0.6}px sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText("◆", cpx, cpy);
      }

      // Draw player
      const px = offsetX + state.player.x * tileSize + tileSize / 2;
      const py = offsetY + state.player.y * tileSize + tileSize / 2;
      ctx.beginPath();
      ctx.arc(px, py, tileSize * 0.35, 0, Math.PI * 2);
      ctx.fillStyle = "#d4af37";
      ctx.fill();
      ctx.strokeStyle = "#fff";
      ctx.lineWidth = 1.5;
      ctx.stroke();
    };

    const resize = () => {
      const rect = container.getBoundingClientRect();
      curDpr = window.devicePixelRatio || 1;
      canvas.width = rect.width * curDpr;
      canvas.height = rect.height * curDpr;
      canvas.style.width = rect.width + "px";
      canvas.style.height = rect.height + "px";
      draw(rect.width, rect.height);
    };

    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(container);
    return () => observer.disconnect();
  }, [state]);

  // Room graph canvas rendering
  useEffect(() => {
    const canvas = graphCanvasRef.current;
    if (!canvas || !state) return;
    const parent = canvas.parentElement;
    if (!parent) return;

    const curDpr = window.devicePixelRatio || 1;
    const w = parent.clientWidth;
    const h = parent.clientHeight - 22;
    canvas.width = w * curDpr;
    canvas.height = h * curDpr;
    canvas.style.width = w + "px";
    canvas.style.height = h + "px";

    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(curDpr, 0, 0, curDpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const visited = state.rooms.filter(r => r.visited);
    if (visited.length === 0) return;

    // BFS layered layout
    const positions: Record<string, {x: number, y: number}> = {};
    const placed = new Set<string>();
    const layers: string[][] = [];
    const edges: [string, string][] = [];

    const startRoom = state.rooms.find(r => r.visited && r.id === state.rooms[0]?.id) || visited[0];
    layers.push([startRoom.id]);
    placed.add(startRoom.id);

    let queue = [startRoom.id];
    while (queue.length > 0) {
      const next: string[] = [];
      for (const rid of queue) {
        const room = state.rooms.find(r => r.id === rid);
        if (!room) continue;
        for (const cid of room.connections) {
          const connRoom = state.rooms.find(r => r.id === cid);
          if (connRoom?.visited && !placed.has(cid)) {
            placed.add(cid);
            next.push(cid);
            edges.push([rid, cid]);
          }
          if (connRoom?.visited && placed.has(cid)) {
            if (!edges.some(e => (e[0] === rid && e[1] === cid) || (e[0] === cid && e[1] === rid))) {
              edges.push([rid, cid]);
            }
          }
        }
      }
      if (next.length > 0) layers.push(next);
      queue = next;
    }

    const layerGap = Math.min(55, (h - 30) / Math.max(layers.length, 1));
    const nodeGap = Math.min(50, (w - 20) / (Math.max(...layers.map(l => l.length), 1)));

    for (let li = 0; li < layers.length; li++) {
      const count = layers[li].length;
      const totalW = (count - 1) * nodeGap;
      const startX = w / 2;
      const startY = 18 + li * layerGap;
      for (let ni = 0; ni < count; ni++) {
        positions[layers[li][ni]] = {
          x: startX - totalW / 2 + ni * nodeGap,
          y: startY,
        };
      }
    }

    const nodeRadius = 12;
    const currentRoomId = state.current_room_id;

    // Draw edges
    for (const [a, b] of edges) {
      const pa = positions[a];
      const pb = positions[b];
      if (!pa || !pb) continue;
      ctx.beginPath();
      ctx.moveTo(pa.x, pa.y);
      ctx.lineTo(pb.x, pb.y);
      ctx.strokeStyle = "#4a4a6e";
      ctx.lineWidth = 2;
      ctx.stroke();
    }

    // Draw nodes
    for (const room of visited) {
      const pos = positions[room.id];
      if (!pos) continue;
      const isCurrent = room.id === currentRoomId;

      ctx.beginPath();
      ctx.arc(pos.x, pos.y, nodeRadius, 0, Math.PI * 2);
      ctx.fillStyle = isCurrent ? "#d4af37" : "#2a2a3e";
      ctx.fill();
      ctx.strokeStyle = isCurrent ? "#fff" : "#4a4a6e";
      ctx.lineWidth = isCurrent ? 2.5 : 1.5;
      ctx.stroke();

      ctx.fillStyle = isCurrent ? "#1a1a2e" : "#8b9dc3";
      ctx.font = "bold 9px 'Times New Roman', serif";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      const label = room.name.length > 8 ? room.name.slice(0, 6) + '..' : room.name;
      ctx.fillText(label, pos.x, pos.y + 1);
    }
  }, [state]);

  if (screen === "menu") {
    return <MainMenu onStart={handleStart} />;
  }

  if (!state) return <div className="loading">Loading the realm...</div>;

  const currentRoom = state.rooms.find(r => r.id === state.current_room_id);

  const primaryItem = state.player.inventory.find(i => i.instance_id === state.player.primary_hand);
  const secondaryItem = state.player.inventory.find(i => i.instance_id === state.player.secondary_hand);
  const primaryIsTwoHanded = primaryItem?.handedness === "TWO_HANDED";

  const secondaryHandLabel = (() => {
    if (primaryIsTwoHanded) return "Locked (2-Handed)";
    if (!secondaryItem) return "None";
    if (secondaryItem.handedness === "OFF_HAND_ONLY") {
      const acBonus = secondaryItem.ac_bonus ?? 2;
      return `${secondaryItem.display_name} (+${acBonus} AC)`;
    }
    if (state.player.active_light_source?.item_id === "torch" && state.player.secondary_hand) {
      const isTorchInHand = secondaryItem.template_id === "torch";
      if (isTorchInHand) {
        return `${secondaryItem.display_name} (Lit - ${state.player.active_light_source.remaining_turns} turns)`;
      }
    }
    return secondaryItem.display_name;
  })();

  const equippedWeaponName = primaryItem?.display_name || "Unarmed";
  
  const turnActionsToRender = isLocked ? lastTurnActions : state.available_actions.filter(a => !isInventoryAction(a));
  const inventoryActionsToRender = isLocked ? lastInventoryActions : state.available_actions.filter(a => isInventoryAction(a));

  const playerResources = state.combat_resources?.["player"];

  const handleSave = async () => {
    try {
      await invoke("save_game");
    } catch (e) {
      console.error(e);
    }
  };

  const handleBackToMenu = () => {
    setScreen("menu");
    setState(null);
    setNarration("");
    setSystemText("");
  };

  return (
    <div className="app-container">
      {dicePopUp && (
        <div key={dicePopUp} className="dice-tray-popup">
          🎲 {dicePopUp}
        </div>
      )}

      <div className="left-panel">
        <div className="narration-window">
          <h2>{state.campaign_name}</h2>
          
          <div className="narration-scroll">
            <p className="narration-text">
              {narration}
              {isStreaming && <span className="cursor">▋</span>}
            </p>
            
            {systemText && (
              <p className="system-text">{systemText}</p>
            )}
          </div>
          
          <div className="action-row">
            {state.game_mode === "GameOver" ? (
              <p className="system-text gameover-text">You have perished. The adventure ends here.</p>
            ) : (
              (() => {
                const dirActions = turnActionsToRender.filter(a => a.startsWith("MOVE_") && a.length <= 10);
                const nonDirActions = turnActionsToRender.filter(a => !a.startsWith("MOVE_") || a.length > 10);
                return (
                  <>
                    <div className="dpad-container">
                      <button className={"dpad-btn dpad-north" + (dirActions.includes("MOVE_NORTH") ? " dpad-btn-available" : "")}
                        disabled={isStreaming || isLocked || !dirActions.includes("MOVE_NORTH")}
                        onClick={() => handleButtonClick("MOVE_NORTH")}>▲</button>
                      <div className="dpad-row">
                        <button className={"dpad-btn dpad-west" + (dirActions.includes("MOVE_WEST") ? " dpad-btn-available" : "")}
                          disabled={isStreaming || isLocked || !dirActions.includes("MOVE_WEST")}
                          onClick={() => handleButtonClick("MOVE_WEST")}>◀</button>
                        <div className="dpad-center">+</div>
                        <button className={"dpad-btn dpad-east" + (dirActions.includes("MOVE_EAST") ? " dpad-btn-available" : "")}
                          disabled={isStreaming || isLocked || !dirActions.includes("MOVE_EAST")}
                          onClick={() => handleButtonClick("MOVE_EAST")}>▶</button>
                      </div>
                      <button className={"dpad-btn dpad-south" + (dirActions.includes("MOVE_SOUTH") ? " dpad-btn-available" : "")}
                        disabled={isStreaming || isLocked || !dirActions.includes("MOVE_SOUTH")}
                        onClick={() => handleButtonClick("MOVE_SOUTH")}>▼</button>
                    </div>
                    {nonDirActions.map((action) => (
                      <button 
                        key={action} 
                        className="action-btn contextual-btn" 
                        disabled={isStreaming || isLocked}
                        onClick={() => handleButtonClick(action)}
                      >
                        {formatActionName(action)}
                      </button>
                    ))}
                  </>
                );
              })()
            )}
          </div>
        </div>
        
        <div className="minimap-area" ref={containerRef}>
          <canvas ref={canvasRef} className="minimap-canvas" />
        </div>
        <div className="dungeon-graph">
          <h4 className="graph-title">Dungeon Map</h4>
          <canvas ref={graphCanvasRef} className="room-graph" />
        </div>
      </div>

      <div className="right-panel">
        {/* Initiative Tracker */}
        {state.game_mode === "Combat" && state.initiative_entries.length > 0 && (
          <div className="combat-tracker initiative-tracker">
            <h3>⚔️ Round {state.round_number}</h3>
            <div className="initiative-list">
              {state.initiative_entries.map((entry, i) => {
                const isCurrentTurn = i === state.current_turn_index;
                const isPlayer = entry.id === "player";
                return (
                  <div key={entry.id} className={`initiative-entry ${isCurrentTurn ? 'active-turn' : ''} ${isPlayer ? 'player-entry' : ''}`}>
                    <span className="init-roll">🎲 {entry.roll}</span>
                    <span className="init-name">{entry.name}</span>
                    {isCurrentTurn && <span className="turn-badge">◀ TURN</span>}
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* Combat Resources */}
        {state.game_mode === "Combat" && playerResources && (
          <div className="combat-tracker resource-tracker">
            <h3>Resources</h3>
            <div className="resource-grid">
              <div className={`resource-cell ${playerResources.has_action ? 'available' : 'spent'}`}>
                <span className="resource-label">Action</span>
                <span className="resource-value">{playerResources.has_action ? 'Available' : 'Spent'}</span>
              </div>
              <div className={`resource-cell ${playerResources.has_bonus_action ? 'available' : 'spent'}`}>
                <span className="resource-label">Bonus</span>
                <span className="resource-value">{playerResources.has_bonus_action ? 'Available' : 'Spent'}</span>
              </div>
              <div className={`resource-cell ${playerResources.has_reaction ? 'available' : 'spent'}`}>
                <span className="resource-label">Reaction</span>
                <span className="resource-value">{playerResources.has_reaction ? 'Ready' : 'Used'}</span>
              </div>
              <div className="resource-cell movement-cell">
                <span className="resource-label">Movement</span>
                <span className="resource-value">{playerResources.remaining_movement_ft} / {state.player.speed} ft</span>
              </div>
              {playerResources.is_dodging && <div className="resource-cell buff">Dodging (+5 AC)</div>}
              {playerResources.is_disengaging && <div className="resource-cell buff">Disengaging</div>}
            </div>
          </div>
        )}

        {/* Enemy Tracker (only visible enemies shown) */}
        {state.game_mode === "Combat" && currentRoom && currentRoom.enemies.length > 0 && (
          <div className="combat-tracker">
            <h3>Enemies</h3>
            {currentRoom.enemies.filter(e => {
              const t = currentRoom.tiles[e.y]?.[e.x];
              return t && t.visibility === "Visible";
            }).map((enemy) => {
              const enemyRes = state.combat_resources?.[enemy.id];
              return (
                <div key={enemy.id} className="enemy-card">
                  <span>{enemy.name}</span>
                  <div className="hp-bar">
                    <div className="hp-fill enemy-hp-fill" style={{ width: `${(enemy.hp / enemy.max_hp) * 100}%` }}></div>
                    <span className="hp-text">{enemy.hp} HP{enemy.studied ? ` | AC ${enemy.ac}` : ''}</span>
                  </div>
                  {enemy.studied && (
                    <>
                      <div className="enemy-abilities">
                        <span>STR {enemy.strength} ({abilMod(enemy.strength)})</span>
                        <span>DEX {enemy.dexterity} ({abilMod(enemy.dexterity)})</span>
                        <span>CON {enemy.constitution} ({abilMod(enemy.constitution)})</span>
                        <span>INT {enemy.intelligence} ({abilMod(enemy.intelligence)})</span>
                        <span>WIS {enemy.wisdom} ({abilMod(enemy.wisdom)})</span>
                        <span>CHA {enemy.charisma} ({abilMod(enemy.charisma)})</span>
                      </div>
                      {enemy.equipped_armour.length > 0 && (
                        <div className="enemy-armour">
                          {enemy.equipped_armour.map((a) => (
                            <span key={a.instance_id}>{a.display_name} (+{a.ac_bonus ?? 0})</span>
                          ))}
                        </div>
                      )}
                      {enemy.perks.length > 0 && (
                        <div className="enemy-perks">
                          {enemy.perks.map((p) => (
                            <span key={p} className="perk-badge">{p.replace(/_/g, ' ')}</span>
                          ))}
                        </div>
                      )}
                    </>
                  )}
                  {enemyRes && (
                    <div className="enemy-resources">
                      {enemyRes.is_dodging && <span className="status-tag">Dodging</span>}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}

        {/* Combat Log */}
        {state.combat_log.length > 0 && (
          <div className="combat-tracker">
            <h3>Combat Log</h3>
            <div className="combat-log">
              {state.combat_log.map((entry, i) => (
                <div key={i} className={`log-entry round-${entry.round}`}>
                  {entry.text}
                </div>
              ))}
            </div>
          </div>
        )}

        {state.last_loot.length > 0 && (
          <div className="combat-tracker">
            <h3>Loot Acquired</h3>
            {state.last_loot.map((group, gi) => (
              <div key={gi} className="enemy-card">
                <span>{group.source_name}</span>
                {group.gp > 0 && <div className="loot-gp">+{group.gp} gp</div>}
                {group.items.map((item) => (
                  <div key={item.instance_id} className="loot-item-row">
                    <span>{item.display_name}{item.quantity > 1 ? ` x${item.quantity}` : ''}</span>
                    <span className="loot-item-value">{item.gp_value} gp</span>
                  </div>
                ))}
              </div>
            ))}
          </div>
        )}

        <div className="status-panel">
          <h3>Status</h3>
          
          <div className="stat-row">
            <span>❤️ HP:</span> 
            <div className="hp-bar">
              <div className="hp-fill" style={{ width: `${(state.player.hp / state.player.max_hp) * 100}%` }}></div>
              <span className="hp-text">{state.player.hp} / {state.player.max_hp}</span>
            </div>
          </div>
          <div className="stat-row"><span>🛡️ AC:</span> {state.player.ac}</div>
          <div className="stat-row"><span>💰 GP:</span> {state.player.gp}</div>
          {state.player.active_light_source && (
            <div className="stat-row torch-status">
              <span>{state.player.active_light_source.item_id === "lantern" ? "🏮 Lantern:" : "🔥 Torch:"}</span>
              <span className="light-status">
                Lit ({state.player.active_light_source.remaining_turns} turns{state.player.active_light_source.is_belt_mounted ? ", belt-mounted" : ""})
              </span>
            </div>
          )}
          <div className="stat-row stat-divider"></div>

            <div className="equipment-section">
            <h4>Equipment</h4>
            <div className="hand-slot primary-hand">
              <span className="hand-label">🗡️ Main Hand:</span>
              <span className="hand-item hand-item-active">
                {equippedWeaponName}
                {state.player.primary_hand && inventoryActionsToRender.includes("UNEQUIP_HAND_PRIMARY") && (
                  <button className="remove-btn" onClick={() => handleButtonClick("UNEQUIP_HAND_PRIMARY")}>{primaryItem?.template_id === "torch" && state.player.active_light_source?.item_id === "torch" ? "Discard" : "Stow"}</button>
                )}
              </span>
            </div>
            <div className={`hand-slot secondary-hand ${primaryIsTwoHanded ? 'locked' : ''}`}>
              <span className="hand-label">🛡️ Off Hand:</span>
              <span className={"hand-item" + (primaryIsTwoHanded ? " hand-item-inactive" : " hand-item-active")}>
                {secondaryHandLabel}
                {state.player.secondary_hand && inventoryActionsToRender.includes("UNEQUIP_HAND_SECONDARY") && (
                  <button className="remove-btn" onClick={() => handleButtonClick("UNEQUIP_HAND_SECONDARY")}>{secondaryItem?.template_id === "torch" && state.player.active_light_source?.item_id === "torch" ? "Discard" : "Stow"}</button>
                )}
                {state.player.secondary_hand && inventoryActionsToRender.filter(a => a.startsWith('MOUNT_UTILITY_') && a.endsWith(`_${state.player.secondary_hand}`)).map(actionId => (
                  <button key={actionId} className="use-item-btn" onClick={() => handleButtonClick(actionId)} disabled={isStreaming}>
                    To Slot {parseInt(actionId.split('_')[2]) + 1}
                  </button>
                ))}
              </span>
            </div>
            {state.player.equipped_belt && state.player.utility_slots.map((slot, idx) => (
              <div key={idx} className="hand-slot">
                <span className="hand-label">Slot {idx + 1}:</span>
                <span className={"hand-item" + (slot ? " hand-item-active" : " hand-item-inactive")}>
                  {slot ? <>{slot.display_name} {inventoryActionsToRender.includes(`UNMOUNT_UTILITY_${idx}`) && (
                    <button className="remove-btn" onClick={() => handleButtonClick(`UNMOUNT_UTILITY_${idx}`)}>Unmount</button>
                  )}</> : "Empty"}
                </span>
              </div>
            ))}
          </div>

          {(state.player.equipped_armour.length > 0 || state.player.equipped_belt) && (
            <div className="equipped-armour">
              <h4>Armour</h4>
              {state.player.equipped_armour.map((a) => (
                <div key={a.instance_id} className="armour-piece">
                  <span>{a.armour_category} {a.display_name} <small className="armour-slot">({a.armor_slot})</small></span>
                  <span className="armour-controls">
                    <span className="ac-val">AC +{a.ac_bonus ?? 0}</span>
                    {inventoryActionsToRender.includes(`UNEQUIP_ARMOUR_${a.instance_id}`) && (
                      <button className="remove-btn" onClick={() => handleButtonClick(`UNEQUIP_ARMOUR_${a.instance_id}`)}>Remove</button>
                    )}
                  </span>
                </div>
              ))}
              {state.player.equipped_belt && (
                <div className="armour-piece">
                  <span className="belt-item-name">{state.player.equipped_belt.display_name} <small className="armour-slot">({state.player.equipped_belt.armor_slot ?? "WAIST"})</small></span>
                  <span className="armour-controls">
                    {inventoryActionsToRender.includes("UNEQUIP_BELT") && (
                      <button className="remove-btn" onClick={() => handleButtonClick("UNEQUIP_BELT")}>Remove</button>
                    )}
                  </span>
                </div>
              )}
            </div>
          )}
        </div>

        <div className="ability-scores">
          <h4>Abilities</h4>
          <div className="abilities-grid">
            <div className="ability">STR <span>{state.player.strength}</span> <small>{abilMod(state.player.strength)}</small></div>
            <div className="ability">DEX <span>{state.player.dexterity}</span> <small>{abilMod(state.player.dexterity)}</small></div>
            <div className="ability">CON <span>{state.player.constitution}</span> <small>{abilMod(state.player.constitution)}</small></div>
            <div className="ability">INT <span>{state.player.intelligence}</span> <small>{abilMod(state.player.intelligence)}</small></div>
            <div className="ability">WIS <span>{state.player.wisdom}</span> <small>{abilMod(state.player.wisdom)}</small></div>
            <div className="ability">CHA <span>{state.player.charisma}</span> <small>{abilMod(state.player.charisma)}</small></div>
          </div>
          <div className="prof-bonus">Proficiency <span>+{state.player.proficiency_bonus}</span></div>
        </div>

        <div className="inventory-panel">
          <h3>Inventory</h3>
          <div className="inventory-list">
            {state.player.inventory.length === 0 && <p className="empty-inv-text">Your pack is empty.</p>}
            {state.player.inventory.map((item) => (
              item.quantity > 0 && item.instance_id !== state.player.primary_hand && item.instance_id !== state.player.secondary_hand && (
                <div key={item.instance_id} className="inventory-item">
                  <div className="item-info item-tooltip-wrap">
                    <span className="item-name">{item.display_name}</span>
                    <span className="item-qty">x{item.quantity}</span>
                    {item.description && (
                      <div className="item-tooltip">{item.description}</div>
                    )}
                  </div>
                  <div className="inv-actions">
                    {inventoryActionsToRender.includes(`USE_ITEM_${item.instance_id}`) && (
                      <button 
                        className="use-item-btn" 
                        onClick={() => handleButtonClick(`USE_ITEM_${item.instance_id}`)}
                        disabled={isStreaming || isLocked}
                      >
                        {item.template_id === "lantern" ? "Ignite" : "Use"}
                      </button>
                    )}
                    {inventoryActionsToRender.includes(`EQUIP_ITEM_${item.instance_id}`) && (
                      <>
                        <button 
                          className="use-item-btn equip-primary-btn" 
                          onClick={() => handleButtonClick(`EQUIP_ITEM_PRIMARY_${item.instance_id}`)}
                          disabled={isStreaming}
                          title="Equip to Main Hand"
                        >
                          Main
                        </button>
                        <button 
                          className="use-item-btn equip-secondary-btn" 
                          onClick={() => handleButtonClick(`EQUIP_ITEM_SECONDARY_${item.instance_id}`)}
                          disabled={isStreaming || primaryIsTwoHanded}
                          title={primaryIsTwoHanded ? "Locked: Two-Handed weapon in Main Hand" : "Equip to Off Hand"}
                        >
                          Off
                        </button>
                      </>
                    )}
                    {inventoryActionsToRender.includes(`EQUIP_ARMOUR_${item.instance_id}`) && (
                      <button 
                        className="use-item-btn" 
                        onClick={() => handleButtonClick(`EQUIP_ARMOUR_${item.instance_id}`)}
                        disabled={isStreaming}
                      >
                        Wear
                      </button>
                    )}
                    {inventoryActionsToRender.includes(`EQUIP_BELT_${item.instance_id}`) && (
                      <button 
                        className="use-item-btn" 
                        onClick={() => handleButtonClick(`EQUIP_BELT_${item.instance_id}`)}
                        disabled={isStreaming}
                      >
                        Equip Belt
                      </button>
                    )}
                    {inventoryActionsToRender.filter(a => a.startsWith(`MOUNT_UTILITY_`) && a.endsWith(`_${item.instance_id}`)).map(actionId => (
                      <button 
                        key={actionId}
                        className="use-item-btn" 
                        onClick={() => handleButtonClick(actionId)}
                        disabled={isStreaming}
                      >
                        To Slot {actionId.split('_')[2]}
                      </button>
                    ))}
                  </div>
                </div>
              )
            ))}
          </div>
        </div>

        <div className="free-text-input">
          <input 
            type="text" 
            placeholder="I want to..." 
            value={inputText}
            onChange={(e) => setInputText(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSubmit()}
            disabled={isStreaming || state.game_mode === "GameOver" || isLocked}
          />
          <button 
            className="submit-btn" 
            onClick={handleSubmit} 
            disabled={isStreaming || !inputText.trim() || state.game_mode === "GameOver" || isLocked}
          >
            Submit
          </button>
        </div>
      </div>

      <div className="status-bar">
        <span>Last Roll: {state.last_roll}</span>
        <span>Mode: {state.game_mode}</span>
        <span>🧠 {engineStatus}</span>
        
        <div className="footer-controls">
          <button className="save-btn" onClick={handleSave}>💾 Save</button>
          <button className="save-btn" onClick={handleBackToMenu}>🏠 Menu</button>
        </div>
      </div>
    </div>
  );
}

export default App;
