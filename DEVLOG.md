# Devlog — Tauri & Dragons

## Phase 0: Project Bootstrap

Set up a Tauri v2 + React 19 + Vite project with a Rust backend. The core idea: a single‑player D&D 5e dungeon crawler where a locally‑running LLM (Gemma 2B) acts as the Dungeon Master, narrating actions and responding to free‑form input, while the engine handles all mechanical rules.

Stubbed out the module layout: `engine/` for game logic, `campaign/` for data models & JSON loading, `llm/` for the AI layer, `commands/` for Tauri IPC. Many files left as empty placeholders for future phases.

Created the first campaign, **The Sunless Crypt** — a 12‑room dungeon with 5 enemy types, 22+ base items, and a procedural item modifier system (quality / material / component across 4 rarity tiers).

## Phase 1: Core Engine

Built out `SessionState`, `Player`, `Room`, `Enemy`, `ItemInstance` — the full save‑game state. Implemented `new_from_campaign` to bootstrap from JSON, then `generate_available_actions` which populates a flat action‑ID list per turn. Added D&D 5e‑style AC computation (armour categories with DEX caps) and `ability_modifier`.

Wired the Tauri command layer (`get_game_state`, `generate_narration`, `player_button_action`, `initialize_gemma`) with event streaming (`llm-token`, `state-updated`, `dice-rolled`, `dm-choose`). The UI is a single React component with a minimap canvas, narration panel, action buttons, inventory, and combat log.

Added `loader.rs` to deserialize `main.json`, `items.json`, `modifiers.json`, `enemies.json`, `lore_template.json`, and `maps/level_1.json` into the `CampaignData` struct hierarchy. Procedural generation (`procedural.rs`) uses a seeded ChaCha RNG for deterministic loot, enemy armour, and room content from `ProceduralSlot` roll tables.

LLM integration: `MockLlm` for development (hardcoded narration), `GemmaEngine` for production (Metal GPU offloading, HuggingFace model download with resume support, streaming token‑by‑token). The LLM outputs structured JSON commands (Damage, Heal, MoveEntity, Attack, etc.) parsed by `parser.rs`.

## Phase 2: Combat & Tactical Depth

Replaced the original flat attack flow with proper D&D 5e combat resolution. Extracted `resolve_attack_roll` → `Result<AttackRollResult, AttackBlockReason>` and `resolve_damage_roll` as pure functions. Added `RangeBand` / `classify_range` for ranged weapon ranges, `AttackBlockReason` for blocking conditions (out of range, no ammunition, adjacent hostile penalty).

Implemented ammo‑gated action visibility. Added `RANGED` and `AMMO` modifier pools and weapons (shortbow, light crossbow) with `ammo_type` tracking. `ammo_consumed_this_combat` resets per‑encounter.

Added 6 scenario tests: in‑range attack, long‑range penalty, out‑of‑range block, no‑ammo block, adjacent‑hostile penalty, and damage resistance/vulnerability/immunity.

Extended the loader with non‑fatal file handling — missing campaign files emit warnings instead of crashing.

## Phase 3: Campaign Expansion

Added 5 more campaigns: `debug_campaign`, `d2_debug_campaign`, `d3_debug_campaign` (test layouts), `the_hollow_chapel`, and `the_curse_of_blackwood` — each with their own items, enemies, modifiers, and map.

Added A* pathfinding (`pathfinding.rs`) for enemy movement.

Built a fog‑of‑war system with tile visibility (Unknown / Explored / Visible), line‑of‑sight checks, and `TileVisibility` per tile.

Add `SpellRegistry` / `BaseSpell` to the campaign schema, with `AoEShape` enum (Circle, Line). Loaded non‑fatally from `spells.json` — `debug_campaign` has the first three spells (fire_bolt, heal_wounds, fireball). Added `validate_spell_weapon_references` for cross‑reference checking against items.json.

## Phase 4: Magic Data Model

Added `has_resonance`, `mana`, `max_mana` to Player — the gating fields for magic aptitude and resource tracking. All `#[serde(default)]` for save compatibility.

Defined `Spellbook` struct (id, name, discipline, tier, known spell IDs) and attached `spellbooks: Vec<Spellbook>` and `known_spell_ids: HashSet<String>` to Player. Plumbed with explicit empty initializers in `new_from_campaign`.

---

**Stats:** ~9,150 lines of Rust across 14 engine files, 3 campaign files, 6 LLM files. 47 unit tests. 6 campaigns. Frontend is a single React component + canvas minimap.

**Next:** Spellbook / scroll item classes, LEARN_SPELLBOOK action, mana‑cost gating for casting, UI for spellbook display, and the cast‑spell action.
