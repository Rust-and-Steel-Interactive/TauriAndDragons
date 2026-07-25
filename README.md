# Tauri & Dragons

A Tauri v2 + React 19 desktop RPG with D&D 5e mechanics, LLM-powered narration, and a procedurally generated dungeon crawl.

---

## Architecture

```
src/                          # Frontend (React 19 + TypeScript + Vite)
├── App.tsx                   # Single-page game UI component
├── App.css                   # All game styling (dark fantasy theme)
├── main.tsx                  # React bootstrap entry point
├── styles/main.css           # (empty placeholder)
├── vite-env.d.ts             # Vite type declarations
├── index.html                # HTML shell
├── package.json              # Dependencies and scripts
├── tsconfig.json             # TypeScript config
├── vite.config.ts            # Vite config (port 1420, HMR 1421)

src-tauri/                    # Backend (Rust + Tauri v2)
├── src/
│   ├── main.rs               # Tauri entry, commands, event emission
│   ├── lib.rs                # Mobile entry point (unused)
│   ├── engine/
│   │   ├── mod.rs            # GameEngine: core gameplay loop
│   │   ├── commands.rs       # Command enum + LlmResponse
│   │   ├── state.rs          # SessionState, Player, Room, Enemy, etc.
│   │   ├── validator.rs      # Command validation + dice rolling
│   │   ├── procedural.rs     # Procedural generation (loot, enemies, armour)
│   │   ├── errors.rs         # (empty placeholder)
│   │   ├── fudge.rs          # (empty placeholder)
│   │   ├── resolution.rs     # (empty placeholder)
│   │   ├── game_loop.rs      # (empty placeholder)
│   │   ├── npc_ai.rs         # (empty placeholder)
│   │   ├── view_filter.rs    # (empty placeholder)
│   │   ├── allegiance.rs     # (empty placeholder)
│   │   └── combat.rs         # (empty placeholder)
│   ├── llm/
│   │   ├── mod.rs            # LlmProvider trait + LlmManager
│   │   ├── gemma.rs          # Gemma 4 LLM integration (Metal GPU)
│   │   ├── mock.rs           # MockLlm for development
│   │   ├── parser.rs         # LLM output → LlmResponse parser
│   │   ├── prompts.rs        # (empty placeholder)
│   │   └── fallback.rs       # (empty placeholder)
│   ├── commands/
│   │   ├── mod.rs            # (empty placeholder)
│   │   ├── ui_events.rs      # (empty placeholder)
│   │   └── input.rs          # (empty placeholder)
│   └── campaign/
│       ├── mod.rs            # Module declarations
│       ├── schema.rs         # Campaign data structs + enums
│       └── loader.rs         # JSON file loader

src-tauri/campaigns/          # Campaign data (JSON)
└── the_sunless_crypt/
    ├── main.json             # Campaign config + player template
    ├── items.json            # 22+ base items (weapons, armour, consumables)
    ├── modifiers.json        # Item quality/material/component affixes
    ├── enemies.json          # 5 enemy types with stats + loot tables
    ├── lore_template.json    # World-building context for LLM
    ├── the_sunless_crypt.json # (alternate template, unused at runtime)
    └── maps/
        └── level_1.json      # 12-room procedural dungeon map
```

---

## Tauri Commands (backend → frontend invoke)

| Command | Handler | Called From | Purpose |
|---|---|---|---|
| `get_game_state` | `main.rs:fn get_game_state` | Initial `useEffect` | Fetches initial `SessionState` on app boot |
| `generate_narration` | `main.rs:fn generate_narration` | `handleSubmit` | Sends free-text prompt → LLM → process commands |
| `player_button_action` | `main.rs:fn player_button_action` | `handleButtonClick` | Sends structured action ID → engine → enemy turn loop |
| `initialize_gemma` | `main.rs:fn initialize_gemma` | `handleInitGemma` | Downloads + loads Gemma 4 model (Metal GPU) |

---

## Tauri Events (backend → frontend)

| Event | Payload Type | Emitted From | Consumed In | Purpose |
|---|---|---|---|---|
| `llm-start` | unit | `stream_llm_narration` | `App.tsx:useEffect` | Clears narration, sets streaming flag |
| `llm-token` | `String` | `GemmaEngine::generate_response` | `App.tsx:useEffect` | Streams narration tokens word by word |
| `llm-done` | unit | `MockLlm` / `GemmaEngine` | `App.tsx:useEffect` | Sets streaming = false |
| `state-updated` | `SessionState` | `generate_narration` / `player_button_action` | `App.tsx:useEffect` | Full state refresh + action cache update |
| `gemma-status` | `String` | `GemmaEngine` download helpers | `App.tsx:useEffect` | Model download/load status text |
| `gemma-download-progress` | `{downloaded_bytes, total_bytes}` | `try_download_once` | `App.tsx:useEffect` | Download progress percentage |
| `dice-rolled` | `String` | Backend validation/combat | `App.tsx:useEffect` | Floating dice popup + system message |
| `dm-choose` | `String` (action_id) | `generate_narration` | `App.tsx:useEffect` | LLM triggers a UI action button programmatically |

---

## Rust Backend

### `main.rs` — Entry Point & Command Handlers

| Function | Purpose |
|---|---|
| `fn main()` | Loads campaign JSON, creates `SessionState` + `GameEngine`, configures Tauri with state + commands |
| `fn get_game_state()` | Clone and return current `SessionState` to frontend |
| `fn stream_llm_narration(app, fact_packet)` | Spawns LLM generation, emits `llm-start`, processes commands on result, handles combat turns, emits `dm-choose` for DmChoose commands |
| `fn generate_narration(app, state, prompt)` | Tauri command: handles free-text input → fact packet → LLM → command processing + enemy turn loop |
| `fn player_button_action(app, state, action_id)` | Tauri command: handles button click → engine.handle_button_action → enemy turn loop |
| `fn initialize_gemma(app, state)` | Tauri command: loads Gemma 4 model via Metal GPU |

### `engine/mod.rs` — GameEngine (8 methods)

| Method | Purpose |
|---|---|
| `new_with_state(state, campaign)` | Constructor |
| `check_combat_state()` | Transitions between Exploration ↔ Combat modes, rolls initiative |
| `roll_initiative()` | d20 + Dex mod for player + each enemy, sorted descending |
| `advance_turn()` | Removes dead enemies, cycles to next in initiative order |
| `process_commands(llm_response)` | Deserializes LLM commands, calls `validate_and_execute` for each |
| `build_outcome_packet()` | Builds fact-packet string for LLM from `last_combat_event` |
| `handle_enemy_turn(enemy_id)` | Rolls enemy attack vs player AC, applies damage, returns fact packet |
| `handle_button_action(action_id)` | **Core dispatcher**: handles `MOVE_TO_`, `ATTACK_`, `STUDY_`, `FLEE_`, `USE_ITEM_`, `EQUIP_ITEM_`, `EQUIP_ARMOUR_`, `UNEQUIP_ARMOUR_`, `TAKE_ITEM_`, `SEARCH_AREA` |
| `handle_free_text(user_input)` | Builds detailed fact packet with room context for LLM free-text input |

### `engine/commands.rs` — Command Enum (18 variants)

| Variant | Fields | Purpose |
|---|---|---|
| `Damage` | `target, amount, damage_type` | Deal damage to a target |
| `Heal` | `target, amount` | Heal a target |
| `AlterStat` | `target, key, operation, value` | Modify a stat |
| `MoveEntity` | `target, to_location` | Move entity to a location |
| `AddItem` | `container, item_id, quantity` | Add item to container |
| `RemoveItem` | `container, item_id, quantity` | Remove item from container |
| `GrantCondition` | `target, condition, duration` | Apply a condition |
| `RemoveCondition` | `target, condition` | Remove a condition |
| `RollCheck` | `stat, dc, advantage` | Roll a skill check |
| `Narrate` | `text` | Pure narration (no engine effect) |
| `AudioCue` | `cue` | Play an audio cue |
| `VisualEffect` | `effect` | Show a visual effect |
| `SetFlag` | `key, value` | Set a game flag |
| `UseItem` | `target, item_id` | Use a consumable item |
| `Flee` | `target` | Flee from combat |
| `Attack` | `target, weapon_id` | Attack a target |
| `EquipItem` | `target, item_id` | Equip a weapon |
| `DmChoose` | `action_id` | Tell frontend to click a UI action button |

### `engine/state.rs` — Structs & State Methods

**Structs:**
- `SessionState` — Complete game session: player, rooms, combat state, initiative, actions
- `Player` — Name, HP, AC, 6 D&D stats, proficiency bonus, inventory, equipped weapon/armour
- `ItemInstance` — Full item: template_id, display_name, item_class, damage dice/bonus, armour slot/category/dex_cap/ac_bonus, effect, quantity
- `Room` — id, name, description, connections, traps, enemies, loot, visited flags
- `Trap` — id, name, DC, damage, damage_type
- `Enemy` — id, template_id, name, HP, AC, 6 stats, damage_dice, attack_bonus, XP, studied flag, equipped_armour, perks, loot_table
- `GameMode` — Enum: Exploration, Combat, GameOver

**Free functions:**
- `ability_modifier(score)` → `(score - 10) / 2`
- `compute_player_ac(player)` → 10 + Dex mod (capped by armour category) + armour AC bonuses
- `compute_enemy_ac(enemy)` — Same AC computation for enemies

**`SessionState` methods:**

| Method | Purpose |
|---|---|
| `new_from_campaign(campaign)` | Builds initial state from campaign JSON: processes procedural slots, generates starting inventory, builds lore context |
| `generate_available_actions()` | Populates action list based on game mode, turn, room contents (attack/study/move/search/use/equip/unequip) |
| `apply_damage(target, amount)` | Applies damage to player/enemy, sets `GameOver` if player HP ≤ 0 |
| `apply_heal(target, amount)` | Heals player, caps at max_hp |
| `get_current_room()` / `get_current_room_mut()` | Gets current room by `current_room_id` |
| `get_equipped_weapon()` | Returns equipped weapon from inventory |
| `get_current_turn_id()` | Returns entity ID at `current_turn_index` |

### `engine/validator.rs` — Command Validation

| Function | Purpose |
|---|---|
| `roll_dice(expression)` | Parses dice notation (`2d6+3`), returns rolled sum |
| `validate_and_execute(cmd, state)` | Validates + executes: Damage, Heal, RollCheck, UseItem, EquipItem, Attack, AddItem. No-ops: Narrate, AudioCue, VisualEffect, Flee, DmChoose |

### `engine/procedural.rs` — Procedural Generation

| Function | Purpose |
|---|---|
| `roll_dice(expression)` | Dice roller (duplicate for module independence) |
| `roll_matches_key(roll, key)` | Checks if roll matches a key (single number or range like `1-5`) |
| `evaluate_slot(slot)` | Rolls slot dice, returns matching spawn templates |
| `pick_tier(rarity_min, rarity_max)` | Random tier 1-4 within range |
| `generate_item_instance(campaign, item_class, rarity_min, rarity_max, specific_item_id)` | Generates procedurally modified item: picks base, rolls quality/material/component, computes stats/name/value |
| `generate_enemy_armour(campaign, config, drop_chance)` | Generates enemy armour from config |
| `generate_enemy_instance(campaign, enemy_id, scale)` | Creates enemy instance from template, scaling HP/AC, generating armour |

### `campaign/schema.rs` — Data Models (deserialized from JSON)

| Struct | Source | Fields |
|---|---|---|
| `CampaignData` | Aggregated | main, items, modifiers, enemies, lore, map |
| `CampaignMain` | `main.json` | campaign_id, name, description, starting_map, player_template |
| `PlayerTemplate` | (in main.json) | starting_hp/ac/gp/stats, proficiency_bonus, starting_inventory, starting_equipped_weapon |
| `ItemRegistry` | `items.json` | Map of base_items keyed by id |
| `BaseItem` | (in items.json) | name, item_class, damage_dice, weight, value, armor_slot, ac_bonus, armour_category, dex_cap, effect |
| `ModifierConfig` | `modifiers.json` | Map of item classes → quality/material/component pools |
| `LoreTemplate` | `lore_template.json` | world_name, tone, setting_lore, factions, key_locations, narrative_rules |
| `EnemyRegistry` | `enemies.json` | Map of base_enemies keyed by id |
| `BaseEnemy` | (in enemies.json) | name, enemy_class, base_hp/ac, damage_dice, attack_bonus, xp, stats, armour_config, perks, loot_table |
| `GameMap` | `maps/level_1.json` | map_id, rooms (Vec<RoomTemplate>) |
| `RoomTemplate` | (in map) | id, name, description, connections, procedural_slots |
| `ProceduralSlot` | (in room) | roll (dice string), results (roll range → SpawnTemplate list) |

### `campaign/loader.rs`

| Function | Purpose |
|---|---|
| `load_campaign(campaign_dir)` | Reads main.json, items.json, modifiers.json, lore_template.json, enemies.json, maps/<starting_map>.json; deserializes + assembles `CampaignData` |

### `llm/mod.rs` — LLM Abstraction

| Item | Purpose |
|---|---|
| `trait LlmProvider` | `async fn generate_response(app, prompt) → LlmResponse` |
| `struct LlmManager` | Holds `active_engine: RwLock<Arc<dyn LlmProvider>>` (swappable) |

### `llm/gemma.rs` — Gemma 4 LLM Integration

| Function / Method | Purpose |
|---|---|
| `GemmaEngine::load(app)` | Synchronous: creates LlamaBackend, downloads model from HuggingFace, loads with Metal GPU offloading |
| `GemmaEngine::load_async(app)` | Async wrapper via `spawn_blocking` |
| `generate_response(app, prompt)` | Formats prompt with Gemma chat template, tokenizes, generates with sampler (temp=0.1), parses output, streams tokens via `llm-token` events |
| `emit_status(...)` | Emits `gemma-download-status` to frontend |
| `try_download_once(app, client, url, part_path, attempt)` | Downloads with resume support (Range header), streaming to `.part` file, stall detection |
| `download_with_retry(app, target_path)` | Orchestrates download with up to 5 retries |

### `llm/mock.rs` — MockLlm

| Method | Purpose |
|---|---|
| `generate_response(app, prompt)` | Returns hardcoded narration, streams word-by-word at 50ms intervals |

### `llm/parser.rs`

| Function | Purpose |
|---|---|
| `parse_llm_output(raw_text)` | Strips Gemma chat artifacts, extracts + deserializes JSON into `LlmResponse` |

---

## Frontend — React Component

### `App.tsx` — Single-Page Game UI

**State (useState, 12 variables):**

| Variable | Type | Initial | Purpose |
|---|---|---|---|
| `state` | `SessionState \| null` | `null` | Full game state from backend |
| `narration` | `string` | Opening narration | Main story text |
| `systemText` | `string` | `""` | System messages (dice rolls, etc.) |
| `inputText` | `string` | `""` | Free-text input value |
| `isStreaming` | `boolean` | `false` | LLM actively streaming |
| `isLocked` | `boolean` | `false` | UI locked during enemy turn |
| `lastTurnActions` | `string[]` | `[]` | Cached turn actions (shown when locked) |
| `lastInventoryActions` | `string[]` | `[]` | Cached inventory actions |
| `dicePopUp` | `string \| null` | `null` | Floating dice roll notification |
| `isDownloading` | `boolean` | `false` | Gemma model downloading |
| `engineStatus` | `string` | `"Mock LLM Active"` | Status bar text |
| `downloadProgress` | `number` | `0` | Download percentage |

**Refs:**
- `handleActionRef` — Latest `handleButtonClick` for `dm-choose` event (avoids stale closures)
- `containerRef` / `canvasRef` — Minimap container + canvas DOM refs
- `panRef` — Pan state: offsetX/Y, dragging flag, start positions

**Module-level functions:**

| Function | Purpose |
|---|---|
| `isInventoryAction(action)` | Returns true if action is USE_ITEM_/EQUIP_ITEM_/EQUIP_ARMOUR_/UNEQUIP_ARMOUR_ |
| `abilMod(score)` | Returns `Math.floor((score - 10) / 2)` as `+N` or `-N` string |

**Component event handlers:**

| Handler | Trigger | Purpose |
|---|---|---|
| `handleInitGemma()` | "Load Gemma" button | Calls `initialize_gemma` backend command |
| `handleSubmit()` | Submit button / Enter key | Sends free-text prompt via `generate_narration` |
| `handleButtonClick(actionId)` | Any action button / `dm-choose` event | Calls `player_button_action`, conditionally locks UI |
| `formatActionName(action)` | (render helper) | Converts action IDs to human-readable button labels |

**Canvas minimap (inside useEffect [state]):**

| Closure | Purpose |
|---|---|
| `draw(w, h)` | BFS room layout by depth, draws connections (solid/dashed), room nodes (gold for current, dark for visited, black for hidden), labels |
| `resize()` | Reads container size, applies `devicePixelRatio`, triggers redraw |
| `onMouseDown/Up/Move` | Click-drag panning with `grab`/`grabbing` cursor |

**Tauri event listeners (useEffect, runs once on mount):**
Listens to `llm-start`, `llm-token`, `llm-done`, `state-updated`, `gemma-status`, `gemma-download-progress`, `dice-rolled`, `dm-choose`.

---

## Campaign Data: The Sunless Crypt

**Loaded from:** `campaigns/the_sunless_crypt/`

**12-room dungeon map (`maps/level_1.json`):**

| Room | Type | Key Content |
|---|---|---|
| Crypt Entrance | Starting room | Hidden MELEE loot |
| Grand Hall | Hub | Skeletal Hound encounter |
| Ruined Armory | Branch | ARMOR loot |
| Trophy Room | Branch | VALUABLE loot |
| Guard Post | Branch | Goblin Scout encounter |
| Goblin Barracks | Branch | Goblin Brute + VALUABLE loot |
| Storage Room | Branch | CONSUMABLE loot |
| Abandoned Kitchen | Branch | Skeletal Hound encounter |
| Dark Passage | Corridor | Poison Dart Trap (DC 12, 1d6 poison) |
| Decaying Archive | Branch | MAGIC loot |
| Ritual Chamber | Pre-boss | Cultist Zealot + MAGIC/CONSUMABLE loot |
| Chamber of the Warden | Boss | Skeletal Warden + VALUABLE/ARMOR treasure |

**5 enemy types:** Skeletal Warden (boss), Skeletal Hound, Cultist Zealot, Goblin Scout, Goblin Brute

**22+ base items:** MELEE (5), MAGIC (2), RANGED (2), ARMOR (10 chest/shield/head/hands/feet across LIGHT/MEDIUM/HEAVY categories), CONSUMABLE (3), VALUABLE (2)

**Item modifiers:** 4 rarity tiers × 3 modifier types (quality, material, component) per item class, affecting name, stats, value, and effects

---

## Key Game Mechanics

- **AC Computation:** `10 + Dex modifier (capped by armour category) + sum of armour AC bonuses`. Categories: LIGHT (no cap), MEDIUM (cap +2), HEAVY (cap 0), SHIELD (independent)
- **Attack:** d20 + ability mod (STR for MELEE, DEX for RANGED, INT for MAGIC) + proficiency vs target AC
- **Damage:** Weapon dice + ability mod + damage bonus
- **Study Action:** d20 + INT mod + proficiency vs DC 10 — reveals enemy stats + AC
- **Flee:** d20 + DEX mod vs DC 12 — success escapes combat with AoO from enemies
- **Initiative:** d20 + DEX mod, sorted descending, cycling turn order
- **Proficiency bonus:** +2 base
- **Enemy armour:** Procedurally generated from `armour_config` (slot list, rarity range, drop chance)
