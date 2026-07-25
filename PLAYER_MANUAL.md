# Tauri & Dragons — Player Manual

> Version 0.1.0 — Debug Campaign

---

## Table of Contents

1. [Overview](#1-overview)
2. [Installation & Setup](#2-installation--setup)
3. [User Interface](#3-user-interface)
4. [Core Game Loop](#4-core-game-loop)
5. [Movement & Exploration](#5-movement--exploration)
6. [Combat System](#6-combat-system)
7. [Actions Reference](#7-actions-reference)
8. [Items & Inventory](#8-items--inventory)
9. [Chests & Lockpicking](#9-chests--lockpicking)
10. [Loot & Discoveries](#10-loot--discoveries)
11. [Player Stats & Abilities](#11-player-stats--abilities)
12. [Enemy Encounters](#12-enemy-encounters)
13. [The Minimap](#13-the-minimap)
14. [Room Graph (World Map)](#14-room-graph-world-map)
15. [LLM & Gemma](#15-llm--gemma)
16. [Dice Rolls](#16-dice-rolls)
17. [Combat Log](#17-combat-log)
18. [Status Effects & Conditions](#18-status-effects--conditions)
19. [Equipment System](#19-equipment-system)
20. [Thieves' Tools & Proficiencies](#20-thieves-tools--proficiencies)
21. [Procedural Generation](#21-procedural-generation)
22. [Trap System](#22-trap-system)
23. [Item Classes & Modifiers](#23-item-classes--modifiers)
24. [Initiative & Turn Order](#24-initiative--turn-order)
25. [Enemy AI](#25-enemy-ai)
26. [Commands & LLM Integration](#26-commands--llm-integration)
27. [Debug Campaign Walkthrough](#27-debug-campaign-walkthrough)
28. [Known Limitations](#28-known-limitations)
29. [Glossary](#29-glossary)
30. [Appendices](#30-appendices)

---

## 1. Overview

**Tauri & Dragons** is a single-player, LLM-driven Dungeons & Dragons role-playing game. It combines a traditional tactical grid (D&D 5e-style combat and movement) with a local large language model (Gemma) that serves as the Dungeon Master. The LLM narrates actions, describes rooms, and responds to free-form player input, while the game engine handles all mechanical rules (combat math, dice rolls, inventory, line-of-sight, item generation).

The game is built with **Tauri** (Rust backend + React/TypeScript frontend) and runs locally on your machine. No internet connection is required after the initial model download.

### Core Philosophy

- **Engine resolves mechanics, LLM narrates story.** The game engine handles all dice rolls, damage calculations, line-of-sight, movement collision, and rule enforcement. The LLM receives a structured "fact packet" describing what happened mechanically and generates natural-language narration in response.
- **Free-form interaction.** You can type anything in the chat input ("I want to intimidate the rat!", "I search for traps", "I examine the goblet closely"), and the LLM will respond creatively. The engine parses structured commands from the LLM's responses to affect the game state.
- **Procedural content.** Items are generated with randomized quality tiers, material types, and components, producing unique variants (e.g., "Flawed Silver Silver Goblet" vs "Test Quality Test Metal Silver Goblet").

---

## 2. Installation & Setup

### System Requirements

- macOS (tested), Windows/Linux support planned
- Minimum 8 GB RAM (16 GB recommended for LLM inference)
- ~4 GB free disk space (for the Gemma model)
- A GPU is beneficial but not required; the application can run on CPU (slower narration generation)

### Installation

1. Install prerequisites:
   - Rust toolchain (`rustup`)
   - Node.js 18+ and npm
   - Tauri CLI: `cargo install tauri-cli`

2. Clone the repository:
   ```
   git clone <repository-url>
   cd tauri-and-dragons
   ```

3. Install frontend dependencies:
   ```
   npm install
   ```

4. Build and run:
   ```
   npm run tauri dev
   ```

On first launch, the application will download the Gemma 2B model (~2 GB). This happens once. A progress indicator is shown in the status bar.

### Save Files

The game auto-saves your session. Save files are stored in the application's data directory. You can delete your save from the Status Bar menu to start a fresh game.

---

## 3. User Interface

The game window is divided into several panels:

```
┌──────────────────────────────────────────────────────────────┐
│  Narrarion Window                                           │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ "You step into a plain stone chamber..."              │  │
│  │                                                        │  │
│  │ [MOVE_NORTH] [MOVE_EAST] [MOVE_SOUTH] [MOVE_WEST]     │  │
│  │ [TAKE_ITEM_Gold Goblet] [OPEN_CHEST_abc123]           │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌─────────────────────┐  ┌──────────┐  ┌──────────────────┐ │
│  │   Minimap           │  │  Room    │  │  Status Panel    │ │
│  │   (Tactical Grid)   │  │  Graph   │  │  HP, AC, GP...   │ │
│  └─────────────────────┘  └──────────┘  └──────────────────┘ │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ Inventory / Combat Log / Enemy Tracker / Loot Display  │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌───────────────────────────────────────────────────────┐   │
│  │ [ I want to...                                    ]▶  │   │
│  └───────────────────────────────────────────────────────┘   │
│                                                              │
│  Status Bar: [d20+3=15] [Mode: Exploration] [Gemma: Ready]  │
└──────────────────────────────────────────────────────────────┘
```

### Key UI Elements

| Element | Location | Purpose |
|---|---|---|
| **Narration Window** | Top center | Displays LLM-generated narration text, action buttons |
| **Free Text Input** | Bottom center | Type anything for the LLM to respond to |
| **Minimap** | Left panel | Tactical grid showing tiles, enemies, loot, chests, player |
| **Room Graph** | Center panel | BFS-layered map of all visited rooms with connections |
| **Status Panel** | Right panel | Player HP bar, AC, gold, actions, status effects |
| **Combat Tracker** | Below status | Initiative order, current turn indicator |
| **Combat Log** | Below minimap | Scrollable history of combat events |
| **Loot Display** | Bottom section | Recently acquired items with GP values |
| **Inventory** | Bottom section | All carried items with Use/Equip/Wear buttons |
| **Ability Scores** | Side panel | STR/DEX/CON/INT/WIS/CHA with modifiers |
| **Status Bar** | Bottom strip | Last dice roll, game mode, engine status, save/menu buttons |

---

## 4. Core Game Loop

The game progresses through a cycle of **exploration → discovery → combat → loot → exploration**.

```
           ┌──────────────────────────────────┐
           │                                  │
           v                                  │
    ┌─────────────┐    ┌──────────────┐       │
    │ Explore     │───>│ Discover     │       │
    │ Move rooms  │    │ Find enemies │       │
    │ Search area │    │ Find loot    │       │
    │             │    │ Find chests  │       │
    └──────┬──────┘    └──────┬───────┘       │
           │                  │               │
           v                  v               │
    ┌─────────────┐    ┌──────────────┐       │
    │ Combat      │───>│ Loot &       │───────┘
    │ Turn-based  │    │ Recover      │
    │ d20 attacks │    │ Pick up items│
    └─────────────┘    └──────────────┘
```

1. **Start**: You begin in the **central hub** room. Read the narration describing your surroundings.
2. **Move**: Click directional buttons or type "I go north" to move to adjacent rooms.
3. **Discover**: When you enter a room, the LLM narrates what you see. Enemies, chests, and loot may be present.
4. **Combat**: If enemies are present, the game enters combat mode. Take turns attacking, using items, or taking tactical actions.
5. **Loot**: After defeating enemies, collect their dropped loot. Search rooms for hidden caches. Open chests for valuables.
6. **Repeat**: Move to new rooms and continue.

---

## 5. Movement & Exploration

### Room-to-Room Movement

Each room has up to 4 connections (North, East, South, West). Click the direction buttons or type the action to move through a doorway:

- `MOVE_NORTH` / `MOVE_SOUTH` / `MOVE_EAST` / `MOVE_WEST` — Move to the adjacent room in that direction.

When you enter a room for the first time, the LLM narrates the atmosphere. The room is added to your **Room Graph** (world map).

### Tile-by-Tile Movement (Within a Room)

Once inside a room, you can move one tile at a time on the tactical grid:

- `MOVE_NORTH` / `MOVE_SOUTH` / `MOVE_EAST` / `MOVE_WEST` — Move one tile in that direction (only available during Exploration).

Movement is blocked by **Walls** (tile type: Wall). You can only move through **Floor**, **Door**, and **Stairs** tiles. Each move updates the **fog of war**: tiles within a 3×3 square around your position become Visible. Previously Visibile tiles degrade to Explored. Unknown tiles remain hidden.

### Fog of War States

| State | Color (Minimap) | Description |
|---|---|---|
| **Unknown** | Dark (#111) | Never been within 3 tiles of this area |
| **Explored** | Dim (#333) | Previously visible, now out of range |
| **Visible** | Bright (#555) | Within 3×3 of your current position |

### Actions Available During Exploration

When no enemies are in the room, you can:

- Move tile-by-tile (4 directions)
- Take items from the floor (`TAKE_ITEM_`)
- Open unlocked chests (`OPEN_CHEST_`)
- Pick locked chests (`PICK_LOCK_`)
- Search the area (`SEARCH_AREA`)
- Use items from inventory (`USE_ITEM_`)
- Equip/unequip weapons and armour (`EQUIP_ITEM_`, `EQUIP_ARMOUR_`, `UNEQUIP_ARMOUR_`)
- Type free-form text for the LLM

---

## 6. Combat System

Combat follows D&D 5e rules with some simplifications. When you enter a room with visible enemies (or enemies see you), the game switches to **Combat Mode**.

### Entering Combat

1. The engine rolls **initiative** for you and all visible enemies: `d20 + DEX modifier`
2. Combatants act in descending order of initiative rolls (ties broken by higher DEX modifier)
3. The game shows the initiative order in the Combat Tracker panel

### Turn Structure

Each turn, a combatant can take:

- **1 Action** (Attack, Dash, Dodge, Disengage, Hide, Ready, Study, Use Item)
- **1 Bonus Action** (Off-hand Attack if dual-wielding)
- **Movement** up to their speed (in tiles)
- **1 Reaction** (Opportunity Attacks — triggered automatically)

### Player Combat Actions

| Action | Description | Type |
|---|---|---|
| `ACTION_ATTACK_<enemy_id>` | Make a melee/ranged attack against a target | Action |
| `ACTION_DASH` | Double your movement speed this turn | Action |
| `ACTION_DODGE` | Focus on defense; attackers have disadvantage (effective +5 AC) | Action |
| `ACTION_DISENGAGE` | Move without provoking opportunity attacks | Action |
| `ACTION_HIDE` | Attempt to conceal yourself (placeholder — ends turn) | Action |
| `ACTION_READY` | Prepare an action to be taken later (prompts LLM) | Action |
| `ACTION_STUDY_<enemy_id>` | Intelligence check (DC 10) to learn enemy abilities | Action |
| `ACTION_FLEE` | Attempt to escape combat (DEX check DC 10); triggers opportunity attacks if not disengaging | Action |
| `BONUS_OFFHAND_ATTACK_<enemy_id>` | Attack with an off-hand light weapon | Bonus Action |
| `USE_ITEM_<item_id>` | Use a consumable item (e.g., health potion) | Action (Combat) |

### Attack Resolution

When you attack an enemy:

1. **Attack Roll**: `d20 + attack_stat_modifier + proficiency_bonus` vs **Enemy AC**
   - Melee weapons (MELEE class): use **STR modifier**
   - Ranged weapons (RANGED class): use **DEX modifier**
   - Magic weapons (MAGIC class): use **INT modifier**
2. **Hit**: If the roll meets or exceeds the target's AC, you hit.
3. **Damage**: Roll the weapon's damage dice + stat modifier + any damage bonus from quality modifiers.
4. **Critical Hit**: A natural 20 doubles all damage dice.
5. **Dodge**: If the enemy is dodging, your effective attack bonus is reduced by 5.
6. **Death**: When an enemy reaches 0 HP, it drops loot and is removed from combat.

### Enemy Turn

Enemies act with full AI (see [Enemy AI](#25-enemy-ai)). The LLM narrates each enemy's actions between turns. After all enemies have acted, control returns to the player.

### End of Combat

Combat ends when:
- All enemies are defeated (0 HP)
- The player flees successfully
- The player dies (HP reaches 0) → Game Over

After combat, loot from defeated enemies is added to `last_loot` and narrated with a delay.

---

## 7. Actions Reference

### Movement Actions

| Action ID | When Available | Effect |
|---|---|---|
| `MOVE_NORTH` | Exploration, adjacent tile not Wall | Move to the room in the North direction (if at a doorway) OR move 1 tile North |
| `MOVE_SOUTH` | Same | Move to the room in the South direction OR 1 tile South |
| `MOVE_EAST` | Same | Move to the room in the East direction OR 1 tile East |
| `MOVE_WEST` | Same | Move to the room in the West direction OR 1 tile West |

### Exploration Actions

| Action ID | When Available | Effect |
|---|---|---|
| `TAKE_ITEM_<instance_id>` | Item on Visible tile, in current room | Removes item from room, adds to inventory (stacks if same variant) |
| `OPEN_CHEST_<chest_id>` | Chest revealed, within 3×3, not broken, unlocked | Drains chest loot into inventory, marks chest as broken |
| `PICK_LOCK_<chest_id>` | Chest revealed, within 3×3, not broken, locked | DEX check vs DC; success drains loot, failure increases break chance |
| `SEARCH_AREA` | Always in Exploration | Wisdom check (DC 12); success reveals hidden caches or chests |

### Combat Actions

| Action ID | When Available | Effect |
|---|---|---|
| `ACTION_ATTACK_<enemy_id>` | Action available, enemy visible | d20 + stat_mod + prof vs AC; damage on hit |
| `ACTION_DASH` | Action available | Double movement speed this turn |
| `ACTION_DODGE` | Action available | +5 effective AC until next turn |
| `ACTION_DISENGAGE` | Action available | Prevents opportunity attacks when moving |
| `ACTION_HIDE` | Action available | Ends turn, attempts to conceal (placeholder) |
| `ACTION_READY` | Action available | Prompts LLM for trigger condition |
| `ACTION_STUDY_<enemy_id>` | Action available | INT check DC 10; reveals enemy perks/abilities |
| `ACTION_FLEE` | Always in Combat | DEX check DC 10; escape combat on success |
| `BONUS_OFFHAND_ATTACK_<enemy_id>` | Bonus action, dual-wielding | DEX-based attack, no stat mod to damage |

### Inventory Actions

| Action ID | When Available | Effect |
|---|---|---|
| `USE_ITEM_<instance_id>` | CONSUMABLE items in inventory | Consumes item, applies effect (e.g., healing) |
| `EQUIP_ITEM_<instance_id>` | Weapon-class items not equipped | Sets as equipped weapon |
| `EQUIP_ARMOUR_<instance_id>` | ARMOR items not already worn | Moves to equipped armour slot |
| `UNEQUIP_ARMOUR_<instance_id>` | Equipped armour items | Returns armour to inventory |

---

## 8. Items & Inventory

### Item Instance Structure

Every item in the game has these properties:

| Field | Type | Description |
|---|---|---|
| `instance_id` | String | Unique identifier (e.g., `proc_abc12345`) |
| `template_id` | String | Base item type (e.g., `shortsword`, `silver_goblet`) |
| `display_name` | String | Full generated name with quality modifiers |
| `description` | String | Generated description text |
| `item_class` | String | Category: `MELEE`, `ARMOR`, `CONSUMABLE`, `VALUABLE`, `TOOL`, etc. |
| `rarity` | int | Quality tier 1-4 (Crude/Standard/Fine/Masterwork) |
| `weight` | float | Encumbrance weight |
| `gp_value` | int | Gold piece value |
| `quantity` | int | Stack size (for stackable items) |
| `damage_dice` | String | Weapon damage (e.g., `1d6`) |
| `damage_bonus` | int | Bonus damage from quality modifiers |
| `weapon_type` | String | Weapon classification |
| `armor_slot` | String | Armour slot (e.g., `CHEST`) |
| `armour_category` | String | Armour category (e.g., `LIGHT`) |
| `dex_cap` | int | Maximum DEX bonus for armour |
| `ac_bonus` | int | Armour class bonus |
| `effect` | String | Special effect (e.g., `HEAL_2d4+2`) |
| `is_quest_item` | bool | Whether the item is quest-critical |
| `variant_id` | String | Exact variant identifier for stacking |

### Item Classes

| Class | Stackable | Examples | Use |
|---|---|---|---|
| `MELEE` | No | Shortsword, Greatsword | Equip as weapon, attack with STR |
| `ARMOR` | No | Leather Armor, Chainmail | Equip to body slot, adds AC |
| `CONSUMABLE` | Yes | Health Potion | Use for healing effects |
| `VALUABLE` | Yes | Silver Goblet, Gold Ring | Sell for GP |
| `TOOL` | No | Thieves' Tools | Required for lockpicking |
| `MAGIC` | No | (future) | Equip as weapon, attack with INT |
| `RANGED` | No | (future) | Equip as weapon, attack with DEX |
| `MATERIAL` | Yes | (future) | Crafting component |
| `AMMO` | Yes | (future) | Ranged weapon ammunition |

### Stacking Rules

Items stack in inventory when they have the same `variant_id`. The `variant_id` encodes the exact combination of base item type + quality tier + material tier + component tier. Two items with different quality tiers (e.g., "Flawed Silver Silver Goblet" vs "Test Quality Silver Silver Goblet") will NOT stack — they are different variants.

Stackable item classes: `CONSUMABLE`, `VALUABLE`, `AMMO`, `MATERIAL`.

### Inventory Panel

The inventory panel (bottom of the screen) lists all items you carry. Each row shows:

- Item icon/name
- Quantity (if stacked)
- Action buttons: **Use** (for CONSUMABLE), **Equip** (for weapons), **Wear** (for armour)

Equipped weapons are shown separately. Equipped armour is shown with an "Unequip" button.

---

## 9. Chests & Lockpicking

### Finding Chests

Chests appear as gold diamond (◆) markers on the minimap. They become visible (is_revealed = true) when you stand within 1 tile of them. The action `OPEN_CHEST_` or `PICK_LOCK_` appears only when the chest is revealed AND within a **3×3 square** centered on your position.

### Opening Unlocked Chests

If a chest is unlocked, the `OPEN_CHEST_<id>` action appears. Clicking it:

1. All items inside are transferred to your inventory
2. The chest is marked as `broken = true` (permanently opened)
3. The LLM narrates what you found

### Picking Locked Chests

If a chest is locked, the `PICK_LOCK_<id>` action appears. Lockpicking requires:

1. **Thieves' Tools** in your inventory
2. **Thieves' Tools Proficiency** (the player starts with this)
3. **DEX modifier** for the roll

The lockpicking process:

1. **Tool Check**: If you lack tools, the action shows a system message saying you need thieves' tools.
2. **Break Check**: Roll d100 vs the chest's `break_chance`. If the roll is <= break_chance, the lock breaks permanently — the chest is jammed and cannot be opened.
3. **Pick Roll**: `d20 + DEX_mod + tool_bonus` vs the chest's **DC**.
   - `tool_bonus` = `proficiency_bonus + quality_bonus` (of your thieves' tools)
   - On success: chest unlocks, all loot transfers to inventory, chest breaks open
   - On failure: `break_chance` increases by 20% (capped at 90%), you can try again
4. **Break Chance Values**: Depend on the lock type:
   - Rusty Lock: 40%
   - Heavy Bolted Lock: 15%
   - Simple Latch: 50%

### Container Parts

Chests are procedurally named from 5 weighted part tables:

| Part | Options |
|---|---|
| Lock Status | Simple Latch, Rusty Lock, Heavy Bolted Lock |
| Condition | Rotting, Standard, Excellent, Pristine |
| Accent Material | Leather-Wrapped, Iron-Banded, Bronze-Studded, Gem-Inlaid |
| Core Material | Oak, Pine, Ironwood, Elderwood |
| Container Type | Chest, Crate, Barrel, Strongbox, Trunk |

Example name: "Rusty Lock, Standard, Iron-Banded, Oak Chest"

---

## 10. Loot & Discoveries

### Floor Loot

Items scattered on the floor appear as **green dots (●)** on the minimap. They are only visible on tiles that are not Unknown. The `TAKE_ITEM_` action appears for items on **Visible** tiles (within your 3×3 visibility range).

To pick up loot:
1. Move adjacent to or onto the item's tile
2. Click the `TAKE_ITEM_<name>` button
3. The item moves to your inventory; the LLM narrates the action

### Loot Discovery Narration

When you enter a room or move within 4 tiles of any loot item, the LLM generates a discovery message: "You notice valuables scattered nearby." This only fires once per room (tracked by the `loot_noticed` flag).

### Hidden Caches

Some items may be hidden (stored in `hidden_caches`). These are not visible on the minimap and must be discovered using the **SEARCH_AREA** action. Searching requires a **Wisdom (Perception) check** vs DC 12:
- **Success**: You find one hidden item or spot a chest in the room
- **Failure**: You find nothing

### Post-Combat Loot

When you defeat an enemy, it drops loot based on its `loot_table`:
- **Debug Rat**: 100% chance of 1 CONSUMABLE item (rarity 1)
- **Debug Zombie**: 100% chance of 1 VALUABLE item (rarity 1)

Loot is also accompanied by GP (gold pieces) equal to the enemy's XP value divided by 5 (minimum 1). The loot collection is narrated after a 5-second delay following combat.

### Loot Display Panel

The Loot Display panel (bottom-right of the combat log) shows recently acquired items with their GP values, grouped by source (enemy name, chest name, etc.).

---

## 11. Player Stats & Abilities

### Core Stats

| Stat | Abbreviation | Default (Debug) | Used For |
|---|---|---|---|
| Strength | STR | 16 (+3) | Melee attack/damage, some skill checks |
| Dexterity | DEX | 16 (+3) | Initiative, AC (light armour), lockpicking, ranged attacks, flee checks |
| Constitution | CON | 16 (+3) | HP, concentration checks |
| Intelligence | INT | 16 (+3) | Study actions, magic attacks |
| Wisdom | WIS | 16 (+3) | Perception checks (searching), spot traps |
| Charisma | CHA | 16 (+3) | Persuasion/intimidation (LLM-driven) |

### Ability Modifier Calculation

`modifier = (stat - 10) / 2` (rounded down)

| Stat Value | Modifier |
|---|---|
| 1 | -5 |
| 8-9 | -1 |
| 10-11 | 0 |
| 12-13 | +1 |
| 14-15 | +2 |
| 16-17 | +3 |
| 18-19 | +4 |
| 20 | +5 |

### Derived Stats

| Stat | Default | Calculation |
|---|---|---|
| HP | 50/50 | Set by campaign (50 for debug) |
| Max HP | 50 | Set by campaign |
| AC | 14 | Base (10) + DEX mod (3) + armour bonus (leather: +1) |
| Proficiency Bonus | +4 | Set by campaign (level ~13 equivalent for debug) |
| Speed | 6 tiles | Set by campaign |
| GP | 100 | Starting gold |

### HP & Death

- Taking damage reduces HP. If HP reaches 0, the game enters **Game Over** mode and you lose.
- Healing (via health potions or other effects) restores HP up to your maximum.
- The HP bar in the Status Panel shows your current and maximum HP visually.

---

## 12. Enemy Encounters

### Debug Campaign Enemies

#### Debug Rat

| Stat | Value |
|---|---|
| HP | 2 |
| AC | 8 |
| Damage | 1d2 |
| Attack Bonus | +1 |
| XP | 5 |
| DEX | 14 (+2) |
| STR | 4 (-3) |
| Perks | None |
| Loot | 100% CONSUMABLE (rarity 1) |

The Debug Rat is a very weak enemy. It dies in 1-2 hits and deals minimal damage. Its high DEX gives it a decent initiative bonus (+2).

#### Debug Zombie

| Stat | Value |
|---|---|
| HP | 3 |
| AC | 9 |
| Damage | 1d3 |
| Attack Bonus | +2 |
| XP | 8 |
| STR | 10 (0) |
| DEX | 6 (-2) |
| Perks | None |
| Loot | 100% VALUABLE (rarity 1) |

The Debug Zombie is slightly tougher than the rat. It has more HP and deals slightly more damage. Its low DEX gives it a -2 initiative penalty, so the player almost always acts first.

### Enemy Visibility

Enemies are only visible (and targetable) if they are on tiles with `visibility == Visible` (within your 3×3 vision range). You cannot attack enemies you cannot see.

### Enemy Detection (Stealth)

Enemies have an awareness system:
- **Unaware**: The enemy has not detected the player
- **Alert**: The enemy has spotted the player and will act aggressively
- Detection range and line-of-sight are calculated each turn

### Enemy Study

Using `ACTION_STUDY_<enemy_id>` allows you to make an **INT check** (DC 10). On success, the enemy's perks and ability scores are revealed in the Enemy Tracker panel. Studying an enemy marks it as `studied = true` and the information persists for the rest of combat.

---

## 13. The Minimap

The minimap (left panel) renders the current room's tactical grid using an HTML5 Canvas.

### Tile Colors

| Tile Type | Unknown | Explored | Visible |
|---|---|---|---|
| Floor | #111 | #333 | #555 |
| Wall | #0a0a0a | #1a1a1a | #2a2a2a |
| Door | #1a0a00 | #3a2a1a | #5a3a1a |
| Rubble | (same as Floor) | (same) | (same) |
| Water | (same as Floor) | (same) | (same) |
| Stairs | (same as Floor) | (same) | (same) |
| Empty | #111 | #111 | #111 |

### Entity Markers

| Marker | Icon | Color | Condition |
|---|---|---|---|
| Player | ● (circle) | Gold (#d4af37) | Always shown at current position |
| Enemy | ● (circle) | Red (#ff4444) | Only on Visible tiles, HP > 0 |
| Loot | ● (dot) | Green (#44dd44) | Only on non-Unknown tiles |
| Chest | ◆ (diamond) | Gold (#d4af37) | Only if revealed, not broken |

### Resizing

The minimap auto-resizes with the window. Each tile is sized to fit the container, capped at 16px maximum.

---

## 14. Room Graph (World Map)

The Room Graph (center panel below the narration) shows all visited rooms in a BFS-layered layout.

### Features

- **Nodes**: Each visited room is shown as a rounded rectangle with its name (truncated to 6 characters)
- **Edges**: Lines connect rooms that share connections
- **Current Room**: Highlighted with a gold border
- **Layout**: Rooms are arranged in layers based on BFS distance from the starting room

### Interaction

Clicking a room node on the Room Graph does not currently trigger navigation (this is a planned feature). Use the directional movement buttons to move between rooms.

---

## 15. LLM & Gemma

### Architecture

The game uses **Gemma 2B** (a Google LLM) running locally via the `gemma.cpp` inference engine. The engine communicates with the LLM via a streaming HTTP API on `http://localhost:8767`.

### Narration Flow

1. **Player Action**: You click a button or type text
2. **Engine Resolution**: The game engine processes the action mechanically (dice rolls, damage, inventory changes)
3. **Fact Packet**: The engine builds a structured prompt called a "fact packet" containing:
   - Event type (e.g., `PlayerAction`, `ModeTransition`, `PlayerMovement`)
   - Resolved action with outcome
   - Dice rolls and results
   - Current situation (room name, description, visible enemies, loot)
4. **LLM Generation**: The fact packet is sent to Gemma, which generates a narration response
5. **Command Parsing**: The LLM's response is parsed for embedded commands (see [Commands Section](#26-commands--llm-integration))
6. **State Update**: Any commands are executed, the game state is emitted to the frontend, and the UI updates

### Fact Packet Types

| Event Type | Trigger | Content |
|---|---|---|
| `PlayerAction` | Any button action | Action, target, outcome, dice rolls |
| `ModeTransition` | Room entry | Room name, description, atmosphere |
| `PlayerMovement` | Tile movement | New position, discoveries, traps, combat initiations |
| `LootCollection` | Post-combat loot | Items acquired, GP found |
| `PlayerCombatAction` | Combat outcome | Attack roll, damage, hit/miss |
| `PlayerExploration` | Search action | Perception check result |

### Free Text (Chat Input)

When you type in the free text input, the LLM receives a full context packet including:
- Current room name and description
- All visible enemies (with HP, AC)
- All visible loot items
- Player inventory and equipped items
- Player stats (HP, AC, ability scores)
- Game mode and recent events

The LLM can respond with narration, issue commands, and roleplay NPC interactions.

### Streaming

Narration text streams token-by-token into the narration window, creating a real-time typing effect. During streaming:
- Action buttons are locked (except equip/unequip/take/search/move/open/pick actions)
- The free text input is disabled
- A cursor blinker indicates active generation

---

## 16. Dice Rolls

### Roll Display

When the engine makes an important dice roll, a popup displays the result at the top of the screen and the roll is logged in the status bar. The format is:

```
d20+3 = 15
```

The roll snap remains visible until the next LLM narration begins.

### Roll Types

| Context | Roll | Formula |
|---|---|---|
| Initiative | d20 | `d20 + DEX_mod` |
| Attack (Melee) | d20 | `d20 + STR_mod + prof_bonus` |
| Attack (Off-hand) | d20 | `d20 + DEX_mod` |
| Damage | Weapon dice | `weapon_dice + STR_mod + damage_bonus` |
| Study (INT check) | d20 | `d20 + INT_mod + prof_bonus` vs DC 10 |
| Search (WIS check) | d20 | `d20 + WIS_mod` vs DC 12 |
| Lockpick | d20 | `d20 + DEX_mod + tool_bonus` vs chest DC |
| Flee | d20 | `d20 + DEX_mod` vs DC 10 |
| Trap Perception | d20 | `d20 + WIS_mod` vs trap DC |
| Lock Break | d100 | `d100` vs break_chance% |

---

## 17. Combat Log

The Combat Log (below the minimap) records a scrollable history of combat events including:

- Attack rolls and outcomes (hit/miss)
- Damage dealt
- Enemy death
- Initiative changes
- Combat mode transitions
- Discovery messages (chests, loot)

Each log entry has a timestamp-context prefix (e.g., `[Round 1]`, `[Discovery]`).

---

## 18. Status Effects & Conditions

### Player Status Effects

| Effect | Trigger | Duration | Effect |
|---|---|---|---|
| **Dodging** | `ACTION_DODGE` | Until next turn | Effective AC +5 |
| **Disengaging** | `ACTION_DISENGAGE` | Until next turn | No opportunity attacks |
| **Hidden** | `ACTION_HIDE` | Until spotted | Placeholder — ends turn |

### Enemy Status Effects

| Effect | Trigger | Effect |
|---|---|---|
| **Studied** | `ACTION_STUDY_` success | Enemy abilities/AC visible in UI |
| **Regeneration** | Enemy perk (Zombie?) | Heal 5 HP/turn if HP > 0 and < max |
| **Berserker** | Enemy perk | +2 attack/damage when below 50% HP |

### Conditions (LLM-Driven)

The LLM can apply/remove conditions via commands:
- `GrantCondition { target, condition, duration }` — e.g., "Prone", "Poisoned"
- `RemoveCondition { target, condition }` — Remove a condition

These are tracked in the player state and displayed in the status panel.

---

## 19. Equipment System

### Weapon Equipment

To equip a weapon:
1. Find a MELEE (or WEAPON/MAGIC/RANGED) item in your inventory
2. Click the **Equip** button next to it
3. The weapon's `instance_id` is stored as `player.equipped_weapon`
4. The equipped weapon's damage dice and bonus are used for attack actions

You can only have one weapon equipped at a time. Equipping a new weapon replaces the previous one (it stays in inventory).

### Armour Equipment

To wear armour:
1. Find an ARMOR item in your inventory
2. Click the **Wear** button
3. The armour moves from inventory to `player.equipped_armour`
4. The armour's `ac_bonus` contributes to your total AC

You can wear multiple armour pieces (one per slot: CHEST, HEAD, LEGS, etc.). To remove armour:
1. Find the armour piece in the Equipped Armour section
2. Click **Unequip** — it returns to inventory

### Damage Calculation with Equipment

When attacking:
1. Your equipped weapon's `damage_dice` is rolled (e.g., `1d6` for shortsword)
2. Add your STR modifier (for MELEE) or DEX modifier (for RANGED)
3. Add any `damage_bonus` from the weapon's quality modifiers
4. On a critical hit (natural 20), double all damage dice

When taking damage:
1. Your AC = 10 (base) + DEX modifier + sum of all equipped armour `ac_bonus` values
2. An attack roll must meet or exceed your AC to hit

---

## 20. Thieves' Tools & Proficiencies

### Thieves' Tools

Thieves' Tools are a TOOL-class item required for lockpicking. The player starts with one set in inventory.

### Tool Quality Bonus

Thieves' Tools have a quality level (tier 1-4) that affects lockpicking:

| Rarity Tier | Quality Name | Tool Bonus |
|---|---|---|
| 1 | Crude | +0 |
| 2 | Standard | +1 |
| 3 | Superior | +2 |
| 4 | Masterwork | +3 |

### Lockpicking Formula

```
d20 + DEX_mod + proficiency_bonus + quality_bonus  vs  Chest DC
```

- Without tools: cannot attempt (system message)
- With tools but no proficiency: roll is `d20 + DEX_mod` only (no proficiency or quality bonus)
- With tools and proficiency: full bonus applied

The player starts with `thieves_tools_proficiency: true`.

---

## 21. Procedural Generation

### Item Generation

Items are generated using a three-tier quality system:

1. **Quality Tier** (rarity 1-4): Affects name prefix and stat bonus
   - Tier 1: +0 bonus, cheapest
   - Tier 2: +0 to +1 bonus
   - Tier 3: +1 to +2 bonus
   - Tier 4: +2 to +3 bonus, most expensive

2. **Material Tier**: Affects name and value multiplier
3. **Component Tier**: Affects name and may add special effects

Each tier is rolled independently using `pick_tier(rarity_min, rarity_max)`, which generates a random value between min and max (capped at 1-4, with max limited to min+2).

### Name Generation

Display names follow the pattern:

```
[Quality Name] [Component Name] [Material Name] [Base Name]
```

Example: "Flawed Silver Silver Goblet" (Quality: Flawed, Material: Silver, Base: Silver Goblet)

### Room Generation

Rooms are generated with:
- **Layout Seed**: Determines tile layout (currently all rooms use a simple rectangular pattern with wall thickness)
- **Loot Seed**: Determines item placement on floor tiles
- **Threat Seed**: Determines enemy placement and chest positions

### Loot Positioning

Loot items are placed on random **Floor** tiles using a seeded RNG. Positions are deterministic for the same seed. Chests prefer **wall-adjacent** positions (x=1, y=1, x=width-2, y=height-2) and are excluded from a 2-tile radius around the room entrance.

---

## 22. Trap System

### Trap Detection

When entering a room with traps:
1. The engine makes a **Perception check**: `d20 + WIS_mod` vs the trap's DC
2. On success: you spot the trap before triggering it (the LLM narrates this)
3. On failure: the trap triggers automatically

### Trap Effects

When a trap triggers:
1. Damage is rolled using the trap's damage expression (e.g., `1d6`)
2. The damage is applied to the player
3. The trap is marked as `is_trap_triggered = true` (prevents retriggering)
4. The LLM narrates the event with damage details

---

## 23. Item Classes & Modifiers

### MELEE Modifiers

| Quality | STAT | Value Mult |
|---|---|---|
| Test-Grade | +0 | x1.0 |
| Standard | +0 | x1.0 |
| Good | +1 | x2.0 |
| Perfect | +2 | x5.0 |

| Material | STAT | Value Mult |
|---|---|---|
| Debug Alloy | +0 | x1.0 |
| Iron | +1 | x1.5 |
| Steel | +2 | x3.0 |
| Mithril | +3 | x8.0 |

| Component | STAT | Value Mult | Effect |
|---|---|---|---|
| Plain | +0 | x1.0 | — |
| Test | +0 | x1.2 | plus_one_atk |
| Refined | +0 | x2.0 | crit_19 |
| Masterwork | +1 | x5.0 | — |

### ARMOR Modifiers

| Quality | STAT | Value Mult |
|---|---|---|
| Test-Grade | +0 | x1.0 |
| Standard | +0 | x1.0 |
| Reinforced | +1 | x2.0 |
| Immaculate | +2 | x5.0 |

| Component | STAT | Value Mult | Effect |
|---|---|---|---|
| Plain | +0 | x1.0 | — |
| Test-Trim | +0 | x1.2 | plus_one_atk |
| Silver-Trim | +0 | x2.0 | plus_one_save |
| Gold-Trim | +1 | x5.0 | — |

### CONSUMABLE Modifiers

| Quality | Value Mult | Effect Mult |
|---|---|---|
| Test Quality | x1.0 | 1.0x |
| Standard | x1.0 | 1.0x |
| Clear | x2.0 | 1.5x |
| Radiant | x5.0 | 2.0x |

| Material | Value Mult |
|---|---|
| Test Vial | x1.0 |
| Glass Vial | x1.0 |
| Crystal Flask | x3.0 |
| Golden Chalice | x6.0 |

### VALUABLE Modifiers

| Quality | Value Mult |
|---|---|
| Test Quality | x1.0 |
| Flawed | x0.8 |
| Flawless | x2.0 |
| Perfect | x5.0 |

| Material | Value Mult |
|---|---|
| Test Metal | x1.0 |
| Silver | x1.0 |
| Gold | x5.0 |
| Platinum | x20.0 |

---

## 24. Initiative & Turn Order

### Rolling Initiative

When combat starts:
1. Player rolls `d20 + DEX_mod`
2. Each visible enemy rolls `d20 + DEX_mod`
3. Results are sorted descending by roll, with ties broken by higher DEX modifier

### Turn Progression

1. **Current turn indicator**: The initiative tracker highlights the active combatant
2. **Action resolution**: The active combatant takes their action(s)
3. **End turn**: Resources reset, status effects expire
4. **Advance**: Next combatant in initiative order
5. **New round**: When all combatants have acted, round number increments, reactions reset

### Combat Resources

Each combatant tracks per-turn resources:

| Resource | Default | Reset |
|---|---|---|
| Action | 1 | Start of turn |
| Bonus Action | 1 | Start of turn |
| Reaction | 1 | Start of new round |
| Movement | Speed (tiles) | Start of turn |

---

## 25. Enemy AI

Enemy AI is driven by a decision tree evaluated each turn:

### Awareness Check

```
if can_see_player():
    if not Alert:
        set Alert, log "enemy spots you"
    # Proceed to combat AI
else:
    run patrol behavior (wander/guard/idle)
```

### Combat AI Decision Tree

```
if has NIMBLE_ESCAPE and HP < 50%:
    if 50% chance:
        Disengage
    else:
        Hide
elif HP < 25% and 40% chance:
    Dodge
elif can't attack and has movement:
    MoveTowardPlayer (A* pathfinding)
elif can attack:
    if has movement:
        AttackAndMove (attack then move)
    else:
        Attack
else:
    NoOp (do nothing)
```

### A* Pathfinding

When enemies need to move toward the player, they use A* pathfinding:
- Walkable tiles: Floor, Door (not Wall)
- Enemies can move through each other
- Path is calculated to minimize distance to the player

### Attack Range

- **Melee enemies**: Range = 1 tile (adjacent)
- **Ranged enemies** (with `RANGED_ATTACK` perk): Range = 6 tiles
- Enemies also use line-of-sight checks (no wall obstruction)

### Opportunity Attacks

When the player (or an enemy) moves out of an adjacent tile without disengaging:
1. Adjacent enemies with a reaction available make an opportunity attack
2. Attack: `d20 + enemy_attack_bonus` vs target AC
3. On hit: normal damage is dealt

---

## 26. Commands & LLM Integration

### LLM Response Format

The LLM responds with a JSON object:

```json
{
  "narration": "Your vivid description here...",
  "commands": [
    { "type": "DAMAGE", "target": "player", "amount": 5, "damage_type": "poison" },
    { "type": "AUDIO_CUE", "cue": "footsteps" }
  ]
}
```

### Available Commands

| Command Type | Fields | Purpose |
|---|---|---|
| `Damage` | target, amount, damage_type | Deal damage to a character |
| `Heal` | target, amount | Restore HP |
| `AlterStat` | target, key, operation, value | Modify a stat (add/subtract/set) |
| `MoveEntity` | target, to_location | Move entity to another room |
| `AddItem` | container, item_id, quantity | Add item to inventory/chest |
| `RemoveItem` | container, item_id, quantity | Remove item from inventory/chest |
| `GrantCondition` | target, condition, duration | Apply a status condition |
| `RemoveCondition` | target, condition | Remove a status condition |
| `RollCheck` | stat, dc, advantage | Request a skill check |
| `Narrate` | text | Queue narration text |
| `AudioCue` | cue | Play a sound effect |
| `VisualEffect` | effect | Trigger a visual effect |
| `SetFlag` | key, value | Set a game state flag |
| `UseItem` | target, item_id | Use an item on a target |
| `Flee` | target | Force a flee attempt |
| `Attack` | target, weapon_id | Force an attack roll |
| `EquipItem` | target, item_id | Force equipment change |
| `DmChoose` | action_id | Automatically click a UI action button |

### Command Processing

Commands are processed after the narration is received:
1. Each command is deserialized and executed
2. The engine applies damage, healing, stat changes, etc.
3. If an attack was issued, the engine advances the turn
4. Dead enemies are cleaned up and loot is generated
5. The game state is emitted to the frontend

### DM_Choose Auto-Resolution

The `DmChoose` command allows the LLM to automatically trigger an action button. When received, the frontend programmatically clicks the corresponding button, which flows through the normal action resolution pipeline. This enables the DM to "force" actions in response to narration (e.g., the DM describes a trap and then triggers a perception check).

---

## 27. Debug Campaign Walkthrough

### Room Layout

```
             [try_combat_1]
                  │
[try_chest]──[central_hub]──[try_loot]
                  │
             [try_combat_2]
```

### Starting Room: Central Hub

A plain stone chamber with four doorways (North, East, South, West). No enemies, no loot, no chests. Use this room to orient yourself.

### North: Try Combat 1 (Debug Rat)

- **Description**: "A rat scuttles in the corner..."
- **Enemy**: 1 Debug Rat (HP 2, AC 8)
- **Tactics**: The rat is very weak. Attack it directly. It drops a CONSUMABLE (health potion) on death.
- **Loot**: After defeating the rat, you'll receive its dropped loot and GP.

### East: Try Chest

- **Description**: "A locked chest sits against the wall."
- **Enemies**: None
- **Chest 1**: Locked (100% locked, DC 10). Contains MELEE items. Requires thieves' tools to open. Attempt lockpicking or find another way.
- **Chest 2**: Unlocked (0% locked). Contains VALUABLE items. Can be opened directly.
- **Tactics**: Walk to the chest (move tiles until adjacent). PICK_LOCK on the locked chest, OPEN_CHEST on the unlocked one.

### West: Try Loot

- **Description**: "Valuables are scattered on the floor."
- **Enemies**: None
- **Loot**: 5 VALUABLE items (Silver Goblet variants) scattered on floor tiles.
- **Tactics**: Walk around the room. Loot items become TAKE_ITEM actions when you're close enough. Pick up everything.

### South: Try Combat 2 (Debug Zombie)

- **Description**: "A shambling zombie lurches toward you."
- **Enemy**: 1 Debug Zombie (HP 3, AC 9)
- **Tactics**: The zombie is slightly tougher than the rat. Attack it. It drops a VALUABLE item on death.

### Recommended Play Order

1. Start in Central Hub
2. Go East to Try Chest — open the unlocked chest
3. Go West to Try Loot — collect all valuables
4. Go North to Try Combat 1 — fight the rat, collect potion
5. Go South to Try Combat 2 — fight the zombie, collect valuable
6. Return to Try Chest — attempt lockpicking on the locked chest

---

## 28. Known Limitations

### LLM
- **Narration latency**: The Gemma 2B model may take 2-10 seconds to generate responses on CPU
- **Fact packet size**: Very large room descriptions or inventory lists may be truncated
- **Command reliability**: The LLM may occasionally produce malformed JSON or inappropriate commands
- **Free text scope**: The LLM's knowledge is limited to the game context provided in the fact packet

### Combat
- **Line-of-sight**: Enemy LOS checks use a simple Manhattan-distance + wall-obstruction model
- **A* pathfinding**: Enemies pathfind each turn; paths can be expensive for large rooms
- **Ranged weapons**: Ranged weapon class (RANGED) exists in the schema but is not fully implemented with ammo tracking
- **Spell system**: No spellcasting system yet; magic weapons use INT for attack but have no spell effects

### UI
- **Mobile support**: The UI is optimized for desktop; mobile responsiveness is limited
- **Accessibility**: Keyboard navigation and screen reader support are minimal
- **Canvas rendering**: The minimap uses Canvas API; very large rooms may cause rendering performance issues

### Items
- **Item effects**: Only `HEAL_` effects are implemented; other effect types are parsed but may not have game-mechanical impact
- **Item descriptions**: Generated descriptions may be repetitive for similar items
- **Stacking edge cases**: Items with the same `variant_id` but different descriptions may stack unexpectedly

### Rooms
- **Tile types**: Only Floor, Wall, Door, and Stairs are fully implemented; Rubble and Water exist in the schema
- **Room templates**: Only `small_room` (rectangular) is implemented; no irregular shapes
- **Multi-floor**: The dungeon seed hierarchy supports multiple levels, but only level 1 is implemented

### Enemies
- **AI simplicity**: Enemy AI uses a straightforward decision tree; no complex tactics (flanking, targeting, etc.)
- **Enemy variety**: Only 2 enemy types exist in the debug campaign
- **Perks**: Enemy perks (NIMBLE_ESCAPE, RANGED_ATTACK, REGENERATION, BERSERKER) exist in the schema but most are untested

---

## 29. Glossary

| Term | Definition |
|---|---|
| **AC** | Armour Class — the target number to hit a character in combat |
| **Action** | A major activity in combat (attack, cast spell, dash, etc.) |
| **Bonus Action** | A minor activity in combat (off-hand attack, certain spells) |
| **BFS** | Breadth-First Search — algorithm used for room graph layout |
| **Canvas** | HTML5 Canvas API used for minimap rendering |
| **Combat Log** | Scrollable history of combat events and dice rolls |
| **DC** | Difficulty Class — target number for skill checks |
| **DEX** | Dexterity — ability score affecting AC, initiative, lockpicking |
| **DM** | Dungeon Master — role played by the Gemma LLM |
| **Fact Packet** | Structured prompt sent to the LLM with game mechanics information |
| **Fog of War** | System tracking which tiles the player has seen |
| **Gemma** | Google's open-source LLM used as the game's Dungeon Master |
| **GP** | Gold Pieces — currency used for buying/selling |
| **HP** | Hit Points — health value; reaching 0 causes death |
| **INT** | Intelligence — ability score used for study actions and magic |
| **Initiative** | Turn order in combat, determined by a DEX check |
| **Instance ID** | Unique identifier for a specific item instance |
| **LLM** | Large Language Model — the AI that narrates the game |
| **LOS** | Line of Sight — check for unobstructed visibility |
| **Minimap** | Tactical grid view of the current room |
| **OA** | Opportunity Attack — attack triggered by leaving an adjacent tile without disengaging |
| **Procedural** | Algorithmically generated content (items, rooms, loot placement) |
| **Reaction** | A response to a trigger (opportunity attacks) |
| **Room Graph** | BFS-layered map showing all visited rooms and connections |
| **STR** | Strength — ability score for melee attacks |
| **Template ID** | Base item type identifier shared by all items of the same base type |
| **Variant ID** | Identifier encoding the exact modifier combination for stacking |
| **WIS** | Wisdom — ability score for perception checks |
| **XP** | Experience points — value of defeating an enemy |
| **Tauri** | Cross-platform framework combining Rust backend with web frontend |

---

## 30. Appendices

### Appendix A: Keyboard Shortcuts

| Key | Action |
|---|---|
| Arrow Up | MOVE_NORTH |
| Arrow Down | MOVE_SOUTH |
| Arrow Left | MOVE_WEST |
| Arrow Right | MOVE_EAST |
| Enter | Submit free text input |
| Escape | Close popups / cancel |

### Appendix B: File Structure

```
tauri-and-dragons/
├── src/                          # Frontend (React/TypeScript)
│   ├── App.tsx                   # Main application component
│   ├── App.css                   # Styles
│   ├── main.tsx                  # Entry point
│   └── vite-env.d.ts             # Vite type declarations
├── src-tauri/                    # Backend (Rust)
│   ├── src/
│   │   ├── main.rs               # Tauri commands, event handlers, startup
│   │   ├── lib.rs                # Library entry point
│   │   ├── engine/
│   │   │   ├── mod.rs            # Game engine: all action handlers
│   │   │   ├── state.rs          # State structs (Player, Room, Item, etc.)
│   │   │   ├── commands.rs       # Command enum and parsing
│   │   │   ├── procedural.rs     # Procedural generation (items, chests, placement)
│   │   │   └── validator.rs      # Action validation
│   │   ├── campaign/
│   │   │   ├── mod.rs            # Campaign loading
│   │   │   └── schema.rs         # Campaign data structures
│   │   └── llm/
│   │       ├── mod.rs            # LLM interface
│   │       └── gemma.rs          # Gemma integration (HTTP client, streaming)
│   └── campaigns/
│       └── debug_campaign/       # Debug campaign data
│           ├── main.json         # Campaign settings, player template
│           ├── items.json        # Base item definitions
│           ├── enemies.json      # Enemy definitions
│           ├── modifiers.json    # Quality/material/component modifiers
│           ├── lore_template.json # World lore for LLM
│           └── maps/
│               └── level_1.json  # Room definitions and connections
├── package.json                  # Node.js dependencies
├── tsconfig.json                 # TypeScript configuration
└── vite.config.ts                # Vite frontend configuration
```

### Appendix C: Configuration Reference

The campaign `main.json` supports the following player template fields:

```json
{
  "campaign_id": "debug",
  "campaign_name": "Debug Campaign",
  "description": "A simple test campaign with one of each room type.",
  "starting_map": "central_hub",
  "player_template": {
    "hp": 50,
    "ac": 14,
    "gp": 100,
    "strength": 16,
    "dexterity": 16,
    "constitution": 16,
    "intelligence": 16,
    "wisdom": 16,
    "charisma": 16,
    "proficiency_bonus": 4,
    "starting_inventory": [
      { "item_id": "health_potion", "quantity": 5 },
      { "item_id": "shortsword", "quantity": 1 },
      { "item_id": "leather_armor", "quantity": 1 },
      { "item_id": "thieves_tools", "quantity": 1 }
    ],
    "equipped_weapon": "shortsword",
    "thieves_tools_proficiency": true,
    "speed": 6
  }
}
```

### Appendix D: Procedural Slot Configuration

Room procedural slots support the following spawn types:

**GENERATE_ITEM**
```json
{
  "type": "GENERATE_ITEM",
  "item_class": "VALUABLE",
  "rarity_min": 1,
  "rarity_max": 2
}
```

**SPAWN_CHEST**
```json
{
  "type": "SPAWN_CHEST",
  "locked_chance": 100,
  "dc": 10,
  "item_class": "MELEE",
  "rarity_min": 1,
  "rarity_max": 2,
  "tier_bias": 0.0
}
```

**SPAWN_ENEMY**
```json
{
  "type": "SPAWN_ENEMY",
  "enemy_id": "debug_rat",
  "scale": 1.0
}
```

**GENERATE_HIDDEN_LOOT**
```json
{
  "type": "GENERATE_HIDDEN_LOOT",
  "item_class": "VALUABLE",
  "rarity_min": 1,
  "rarity_max": 2
}
```

**TRAP**
```json
{
  "type": "TRAP",
  "id": "pit_trap",
  "name": "Pit Trap",
  "dc": 10,
  "damage": "1d6",
  "damage_type": "piercing"
}
```

### Appendix E: Dice Notation

The game uses standard D&D dice notation:

| Notation | Meaning |
|---|---|
| `1d6` | Roll 1 six-sided die |
| `2d4+2` | Roll 2 four-sided dice, add 2 |
| `1d20+3` | Roll 1 twenty-sided die, add 3 |
| `d100` | Roll 1 hundred-sided die (percentile) |

---

*End of Player Manual — Version 0.1.0*
