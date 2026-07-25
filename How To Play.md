# How To Play — Tauri & Dragons

> A player-friendly guide to adventuring in the Debug Realm

---

## What Is This Game?

Tauri & Dragons is a single-player tabletop RPG where you explore dungeons, fight monsters, and collect loot. A local AI (Gemma) acts as your Dungeon Master — it describes what you see, narrates combat, and responds to whatever you type. The game engine handles all the rules (dice rolls, damage, line-of-sight, inventory) so you can focus on playing.

---

## Quick Start

1. Launch the game. On first run, it downloads the Gemma AI model (~2 GB).
2. You start in the **Central Hub** — a stone chamber with four doors.
3. Read the narration describing the room.
4. Click a direction button (**N/S/E/W**) to move through a doorway.
5. Encounter enemies, find loot, open chests. Survive.

---

## The Screen

```
┌──────────────────────────────────────────────────────┐
│  NARRATION WINDOW                                     │
│  "You step into a dim chamber..."                     │
│                                                       │
│  [↑] [→] [↓] [←]  [TAKE_ITEM] [OPEN_CHEST]           │
├──────────┬───────────┬────────────────────────────────┤
│ MINIMAP  │ ROOM MAP  │ STATUS: HP ████ AC 14 GP 100  │
│ (grid)   │ (visited  │        Initiative Tracker      │
│ tiles ·  │  rooms)   │        Enemy Tracker           │
│ enemies  │           │        Ability Scores          │
│ ● loot ◆ │           │        Combat Log              │
├──────────┴───────────┴────────────────────────────────┤
│ INVENTORY: Health Potion (x5) [Use]                   │
│           Shortsword [Equip]                          │
│           Thieves' Tools                              │
├───────────────────────────────────────────────────────┤
│ [ I want to...                                   ] ▶  │
├───────────────────────────────────────────────────────┤
│ d20+3=15  │ Mode: Exploration  │ Gemma: Ready         │
└───────────────────────────────────────────────────────┘
```

---

## Moving Around

### Between Rooms
Click **N / E / S / W** to move through a doorway into the next room. The room is added to your world map.

### Inside a Room
Click **N / E / S / W** to move one tile at a time on the tactical grid.

### What Blocks You
- **Walls** — you can't walk through them
- You can walk on **Floor**, **Door**, and **Stairs** tiles

### Fog of War
- **Dark tiles**: You've never been there — unknown
- **Dim tiles**: You've seen them before — explored
- **Bright tiles**: You're currently near them — visible (3×3 area around you)

---

## Finding Things (Discoveries)

When you enter a room or walk near interesting things, the DM tells you:

- **"You spot a chest nearby"** — a chest is close enough to interact with
- **"You notice valuables scattered nearby"** — loot items are on the floor
- **"You spot X nearby"** — you found something specific

These messages only appear once per room, so pay attention.

---

## Loot & Items

### Floor Loot
Items on the ground show as **green dots (●)** on the minimap. Walk near one and a `TAKE_ITEM` button appears. Click it to pick it up.

### What You Find
- **VALUABLE items** (Silver Goblets, Gold Rings) — worth GP, stack in inventory
- **CONSUMABLE items** (Health Potions) — use them to heal
- **WEAPONS / ARMOR** — equip them for better combat

### Searching
Use `SEARCH_AREA` to look for hidden items. This is a **Wisdom check** (DC 12). Success reveals hidden caches or spots chests you might have missed.

---

## Chests

Chests show as **gold diamonds (◆)** on the minimap. They become interactable when you walk next to them (3×3 area).

### Unlocked Chests
Click `OPEN_CHEST` to take everything inside. The chest breaks open afterward.

### Locked Chests
Click `PICK_LOCK` to attempt opening. You need:
1. **Thieves' Tools** in your inventory (you start with a set)
2. **Proficiency** with them (you have this)

The game rolls: `d20 + DEX + proficiency + tool quality` vs the chest's DC.

**Watch out**: Each failed attempt increases the chance your lockpicks break. If they break, the chest is permanently jammed.

---

## Combat

When you enter a room with enemies, the game switches to **Combat Mode**.

### How Combat Works

1. **Initiative** is rolled — everyone takes turns in order
2. On your turn, you have:
   - **1 Action** (attack, dash, dodge, disengage, hide, study, use item)
   - **1 Bonus Action** (off-hand attack if dual-wielding)
   - **Movement** (up to 6 tiles)
3. Click an action button to do it
4. After you act, each enemy takes its turn
5. When all enemies are dead, combat ends

### Combat Actions

| Button | What It Does |
|---|---|
| **ATTACK `<enemy>`** | Roll to hit. If you succeed, deal damage. |
| **DASH** | Double your movement this turn |
| **DODGE** | Enemies have a harder time hitting you (+5 effective AC) |
| **DISENGAGE** | Move away without getting hit |
| **HIDE** | Try to conceal yourself (ends your turn) |
| **STUDY `<enemy>`** | Intelligence check to learn the enemy's abilities |
| **FLEE** | Try to escape combat (Dex check). You might get hit on the way out. |
| **OFF-HAND ATTACK** | Bonus action attack if you have a light weapon in each hand |

### How Attacking Works

1. Roll `d20 + STR/DEX + proficiency` vs enemy **Armour Class (AC)**
2. If the roll is >= the enemy's AC, you **hit**
3. Roll your weapon's damage dice + stat bonus
4. A **natural 20** is a critical hit — double damage!

---

## Taking Damage & Dying

- Your **HP** is shown in the status panel
- When enemies hit you, HP decreases
- If HP reaches 0, **Game Over** — your adventure ends
- Use **Health Potions** (`USE_ITEM`) to heal during or after combat

---

## Using & Equipping Items

### Use
Click **Use** on a CONSUMABLE item (like a Health Potion) to drink/eat/activate it. The effect applies immediately.

### Equip Weapon
Click **Equip** on a MELEE or weapon-class item to wield it. You can only have one weapon equipped at a time.

### Wear Armour
Click **Wear** on an ARMOR item to put it on. It adds to your Armour Class. Unequip it later if you find better.

### Equipped Items
- Your current weapon is shown in the status panel
- Worn armour is shown with an **Unequip** button

---

## Player Stats & Abilities

Your character has six core abilities:

| Ability | What It's Used For |
|---|---|
| **Strength (STR)** | Melee attacks, breaking things |
| **Dexterity (DEX)** | Initiative, AC, lockpicking, dodging, fleeing |
| **Constitution (CON)** | HP, resisting poison |
| **Intelligence (INT)** | Studying enemies, magic |
| **Wisdom (WIS)** | Perception (searching), spotting traps |
| **Charisma (CHA)** | Talking to NPCs, intimidating |

Your **debug character** starts with all stats at 16 (modifier +3) and a proficiency bonus of +4 — effectively a very capable level 13-ish adventurer.

---

## Enemies (Debug Realm)

### Debug Rat
- HP 2, AC 8
- Damage: 1d2
- Very weak. Drops a CONSUMABLE (health potion).

### Debug Zombie
- HP 3, AC 9
- Damage: 1d3
- Slightly tougher. Drops a VALUABLE item.

---

## Dice Rolls

When something important happens, the game shows a dice popup:

```
d20+3 = 15
```

The roll stays on screen until the next narration. Common rolls:

| Situation | Roll |
|---|---|
| Initiative | d20 + DEX |
| Attack | d20 + STR/DEX + proficiency |
| Damage | weapon dice + STR/DEX |
| Lockpick | d20 + DEX + tool bonus |
| Search | d20 + WIS vs DC 12 |
| Flee | d20 + DEX vs DC 10 |

---

## Free Text (Chat)

You're not limited to buttons! Type anything in the chat box:

- *"I search for traps"*
- *"I try to intimidate the rat"*
- *"I examine the silver goblet more closely"*
- *"What do I see?"*

The DM responds to whatever you type and may issue game commands (damage, healing, adding items) based on your actions.

---

## The Minimap

The left panel shows the current room as a top-down grid:

| Icon | What | Color |
|---|---|---|
| ● | You (player) | Gold |
| ● | Enemy | Red |
| ● | Loot on floor | Green |
| ◆ | Chest | Gold |

Only tiles you've seen are revealed. The rest are dark.

---

## The World Map

The center panel shows all rooms you've visited, connected by lines. Your current room is highlighted in gold.

---

## The DM (Gemma AI)

The game runs a local AI model (Gemma 2B) that acts as your Dungeon Master. It:

- Describes rooms atmospherically when you enter
- Narrates combat actions with vivid prose
- Responds to whatever you type
- Issues commands to affect the game world

When the DM is narrating, the action buttons lock until it finishes. You'll see tokens stream in as the DM "speaks."

---

## Debug Campaign Walkthrough

The debug campaign has 5 rooms arranged like a cross:

```
          [Combat: Rat]
               │
[Chest]──[Central Hub]──[Loot]
               │
          [Combat: Zombie]
```

1. **Central Hub** — Starting room. Four doorways. Explore.
2. **Chest Room (East)** — Two chests: one locked, one unlocked. Open the unlocked one for VALUABLES. Try picking the locked one.
3. **Loot Room (West)** — 5 valuable items on the floor. Walk around and pick them all up.
4. **Combat 1 (North)** — A single Debug Rat. Kill it for a health potion.
5. **Combat 2 (South)** — A single Debug Zombie. Kill it for a valuable item.

After clearing everything, return to the chest room and try lockpicking the locked chest.

---

## Tips

- **Move first, then act**. Walk near loot or chests before trying to interact.
- **Keep health potions handy**. Use them mid-combat if you're low on HP.
- **Study enemies you haven't seen before**. It reveals their abilities.
- **Search rooms after combat**. There might be hidden loot.
- **Equip better gear**. If you find a weapon with a quality bonus, equip it.
- **Talk to the DM**. The free text input lets you do things the buttons don't cover.
- **Watch the fog of war**. Dark tiles might hide enemies or treasure.
- **Lockpicking can fail**. The more you fail, the higher the risk of breaking your tools. Consider saving before attempting high-DC chests.

---

## Quick Reference

### All Actions

| Action | Where Available | What It Does |
|---|---|---|
| MOVE_N/S/E/W | Always (exploration) | Move to new room or 1 tile |
| TAKE_ITEM | Near loot on floor | Pick up item |
| OPEN_CHEST | Adjacent to unlocked chest | Take all chest contents |
| PICK_LOCK | Adjacent to locked chest | Attempt to unlock |
| SEARCH_AREA | Any room | Perception check for hidden things |
| ATTACK | Combat, enemy visible | Fight a target |
| DASH | Combat | Double movement |
| DODGE | Combat | +5 effective AC |
| DISENGAGE | Combat | Safe movement away |
| HIDE | Combat | Try to conceal |
| STUDY | Combat, enemy visible | Learn enemy stats |
| FLEE | Combat | Attempt escape |
| OFF-HAND ATTACK | Combat, dual-wielding | Bonus action attack |
| USE_ITEM | CONSUMABLE in inventory | Drink/use item |
| EQUIP_ITEM | Weapon in inventory | Wield weapon |
| EQUIP_ARMOUR / UNEQUIP_ARMOUR | Armour in inventory | Wear/remove armour |
| Free text | Always | Talk to the DM |

### Keyboard Shortcuts

- **Arrow keys**: Move N/S/E/W
- **Enter**: Submit typed text
- **Escape**: Close popups

### What Each Stat Modifier Gives

| Score | Mod |
|---|---|
| 1 | -5 |
| 8-9 | -1 |
| 10-11 | 0 |
| 12-13 | +1 |
| 14-15 | +2 |
| 16-17 | +3 |
| 18-19 | +4 |
| 20 | +5 |

---

*Happy adventuring! If you get stuck, try talking to the DM — they're surprisingly helpful.*
