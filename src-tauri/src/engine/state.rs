use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

/// Tile types for the tactical grid.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TileType {
    Floor,
    Wall,
    Rubble,
    Water,
    Door,
    Stairs,
    Empty,
}

/// Visibility state for fog of war.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TileVisibility {
    Unknown,
    Explored,
    Visible,
}

/// Damage types for attacks, spells, and environmental effects.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(dead_code)]
pub enum DamageType {
    Bludgeoning,
    Piercing,
    Slashing,
    Acid,
    Cold,
    Fire,
    Force,
    Lightning,
    Necrotic,
    Poison,
    Psychic,
    Radiant,
    Thunder,
}

/// A single tile on the tactical grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
    pub tile_type: TileType,
    pub visibility: TileVisibility,
    #[serde(default)]
    pub ground_light_source: Option<ActiveLightSource>,
}

/// The three independent d100 rolls that seed a room.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSeed {
    pub layout_roll: i32,  // z-seed: room layout / structure
    pub loot_roll: i32,    // x-seed: loot placement
    pub threat_roll: i32,  // y-seed: enemies / traps / chests
}

/// Awareness state for NPCs (enemies).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AwarenessState {
    Unaware,
    Suspicious,
    Alert,
    Searching,
}

/// Behaviour pattern for NPC movement when not in combat.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NpcBehaviour {
    Patrol {
        waypoints: Vec<(i32, i32)>,
        current_index: usize,
    },
    Guard {
        guard_x: i32,
        guard_y: i32,
        patrol_radius: i32,
    },
    Wander {
        anchor_x: i32,
        anchor_y: i32,
        wander_radius: i32,
    },
    Investigate {
        target_x: i32,
        target_y: i32,
        remaining_turns: i32,
    },
    Idle,
}

impl Default for NpcBehaviour {
    fn default() -> Self {
        NpcBehaviour::Idle
    }
}

impl Default for DamageType {
    fn default() -> Self {
        DamageType::Bludgeoning
    }
}

impl DamageType {
    pub fn from_loose_str(s: &str) -> DamageType {
        let normalized = s.trim().to_uppercase().replace(' ', "_").replace('-', "_");
        match normalized.as_str() {
            "BLUDGEONING" | "BLUNT" | "CRUSHING" => DamageType::Bludgeoning,
            "PIERCING" | "PIERCE" => DamageType::Piercing,
            "SLASHING" | "SLASH" | "CUTTING" => DamageType::Slashing,
            "ACID" | "CORROSIVE" => DamageType::Acid,
            "COLD" | "ICE" | "FROST" => DamageType::Cold,
            "FIRE" | "FLAME" | "BURNING" => DamageType::Fire,
            "FORCE" => DamageType::Force,
            "LIGHTNING" | "ELECTRIC" | "ELECTRICITY" | "SHOCK" => DamageType::Lightning,
            "NECROTIC" | "DARK" | "SHADOW" | "DEATH" => DamageType::Necrotic,
            "POISON" | "TOXIC" | "VENOM" => DamageType::Poison,
            "PSYCHIC" | "PSIONIC" | "MENTAL" => DamageType::Psychic,
            "RADIANT" | "HOLY" | "LIGHT" => DamageType::Radiant,
            "THUNDER" | "SONIC" => DamageType::Thunder,
            other => {
                eprintln!("⚠️ Unknown damage_type '{}' from LLM/campaign data, defaulting to Bludgeoning", other);
                DamageType::Bludgeoning
            }
        }
    }
}

impl std::fmt::Display for DamageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            DamageType::Bludgeoning => "Bludgeoning",
            DamageType::Piercing => "Piercing",
            DamageType::Slashing => "Slashing",
            DamageType::Acid => "Acid",
            DamageType::Cold => "Cold",
            DamageType::Fire => "Fire",
            DamageType::Force => "Force",
            DamageType::Lightning => "Lightning",
            DamageType::Necrotic => "Necrotic",
            DamageType::Poison => "Poison",
            DamageType::Psychic => "Psychic",
            DamageType::Radiant => "Radiant",
            DamageType::Thunder => "Thunder",
        };
        write!(f, "{}", label)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeaponRange {
    pub normal: i32,
    #[serde(default)]
    pub long: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[allow(dead_code)]
pub struct DamageProfile {
    #[serde(default)]
    pub resistances: Vec<DamageType>,
    #[serde(default)]
    pub vulnerabilities: Vec<DamageType>,
    #[serde(default)]
    pub immunities: Vec<DamageType>,
}

fn deserialize_damage_type_loose<'de, D>(deserializer: D) -> Result<DamageType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(DamageType::from_loose_str(&s))
}

pub const TILE_SIZE_FEET: i32 = 5;

pub fn manhattan_distance(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    (x1 - x2).abs() + (y1 - y2).abs()
}

pub fn chebyshev_distance(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    (x1 - x2).abs().max((y1 - y2).abs())
}

pub fn ability_modifier(score: i32) -> i32 {
    (score - 10) / 2
}

pub fn parse_chest_position(name: &str) -> (i32, i32) {
    if let Some(pos_part) = name.split('[').nth(1) {
        if let Some(coords) = pos_part.split(']').next() {
            let parts: Vec<&str> = coords.split(':').collect();
            if parts.len() == 2 {
                if let (Ok(x), Ok(y)) = (parts[0].trim().parse(), parts[1].trim().parse()) {
                    return (x, y);
                }
            }
        }
    }
    (0, 0)
}

pub fn is_stackable(item_class: &str) -> bool {
    matches!(item_class, "CONSUMABLE" | "VALUABLE" | "AMMO" | "MATERIAL")
}

pub fn stacks_with(a: &ItemInstance, b: &ItemInstance) -> bool {
    !a.variant_id.is_empty() && a.variant_id == b.variant_id
}

pub fn add_to_inventory(inventory: &mut Vec<ItemInstance>, item: ItemInstance) {
    if is_stackable(&item.item_class) {
        if let Some(existing) = inventory.iter_mut().find(|i| stacks_with(i, &item)) {
            existing.quantity += item.quantity;
            return;
        }
    }
    inventory.push(item);
}

pub fn get_perk_description(perk: &str) -> &'static str {
    match perk {
        "UNDEAD_FORTITUDE" => "When reduced to 0 HP, this creature may make a Constitution saving throw (DC 5 + damage taken) to drop to 1 HP instead.",
        "PACK_TACTICS" => "This creature has advantage on attack rolls against a target if at least one of its allies is within 5 feet of the target.",
        "NIMBLE_ESCAPE" => "This creature can take the Disengage or Hide action as a bonus action on each of its turns.",
        "MAGIC_RESISTANCE" => "This creature has advantage on saving throws against spells and other magical effects.",
        "BERSERKER" => "When this creature is reduced to half its maximum HP or fewer, it gains +2 to attack and damage rolls.",
        "REGENERATION" => "This creature regains 5 HP at the start of each of its turns if it has at least 1 HP.",
        "POISON_IMMUNITY" => "This creature is immune to poison damage and the poisoned condition.",
        "DARKVISION" => "This creature can see in dim light as if it were bright light and in darkness as if it were dim light.",
        _ => "A special ability with unknown effects.",
    }
}

pub fn remove_from_inventory(inventory: &mut Vec<ItemInstance>, instance_id: &str, quantity: i32) -> bool {
    if let Some(idx) = inventory.iter().position(|i| i.instance_id == instance_id) {
        if inventory[idx].quantity > quantity {
            inventory[idx].quantity -= quantity;
        } else {
            inventory.remove(idx);
        }
        return true;
    }
    false
}

/// Get attack range in tiles based on equipped weapon
pub fn get_weapon_range(player: &Player) -> i32 {
    if let Some(w) = player.inventory.iter().find(|i| {
        Some(i.instance_id.as_str()) == player.primary_hand.as_deref()
            || Some(i.instance_id.as_str()) == player.secondary_hand.as_deref()
    }) {
        if let Some(wr) = &w.weapon_range {
            wr.normal
        } else {
            match w.item_class.as_str() {
                "RANGED" => 10,
                "MAGIC" => 6,
                _ => {
                    if w.weapon_type.as_deref() == Some("reach")
                        || w.template_id.contains("reach")
                        || w.template_id.contains("whip")
                        || w.template_id.contains("polearm")
                    {
                        2
                    } else {
                        1
                    }
                }
            }
        }
    } else {
        // Unarmed: adjacent only
        1
    }
}

pub fn get_enemy_attack_range(enemy: &Enemy) -> i32 {
    let has_ranged = enemy.perks.iter().any(|p| p == "RANGED_ATTACK");
    if has_ranged { 6 } else { 1 }
}

/// Check if a tile is blocked by a wall or obstacle
pub fn is_tile_blocked(tiles: &[Vec<Tile>], x: i32, y: i32) -> bool {
    if y < 0 || x < 0 || y >= tiles.len() as i32 || x >= tiles[0].len() as i32 {
        return true;
    }
    matches!(tiles[y as usize][x as usize].tile_type, TileType::Wall)
}

/// Bresenham's line algorithm: return all tiles between (x0,y0) and (x1,y1), exclusive of endpoints
pub fn tiles_between(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let mut tiles = Vec::new();
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut cx = x0;
    let mut cy = y0;

    loop {
        if cx == x1 && cy == y1 { break; }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            cx += sx;
        }
        if e2 <= dx {
            err += dx;
            cy += sy;
        }
        if cx == x1 && cy == y1 { break; }
        tiles.push((cx, cy));
    }
    tiles
}

/// Check if there is a clear line of sight between two tiles
pub fn has_line_of_sight(tiles: &[Vec<Tile>], x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
    let line = tiles_between(x0, y0, x1, y1);
    for (tx, ty) in &line {
        if is_tile_blocked(tiles, *tx, *ty) {
            return false;
        }
    }
    true
}

/// Check if two positions are adjacent (including diagonally)
pub fn is_adjacent(x1: i32, y1: i32, x2: i32, y2: i32) -> bool {
    (x1 - x2).abs() <= 1 && (y1 - y2).abs() <= 1 && !(x1 == x2 && y1 == y2)
}

/// Return enemies whose tile is currently marked as Visible (within player's FoW).
pub fn get_visible_enemies<'a>(room: &'a Room) -> Vec<&'a Enemy> {
    room.enemies.iter()
        .filter(|e| e.hp > 0)
        .filter(|e| {
            room.tiles.get(e.y as usize)
                .and_then(|row| row.get(e.x as usize))
                .map(|t| t.visibility == TileVisibility::Visible)
                .unwrap_or(false)
        })
        .collect()
}

/// Check whether the player is within an enemy's detection range and line of sight.
pub fn can_enemy_see_player(enemy: &Enemy, player_x: i32, player_y: i32, tiles: &[Vec<Tile>]) -> bool {
    let dist = manhattan_distance(enemy.x, enemy.y, player_x, player_y);
    if dist > enemy.detection_range {
        return false;
    }
    has_line_of_sight(tiles, enemy.x, enemy.y, player_x, player_y)
}

pub fn get_occupied_tiles(room: &Room, exclude_id: Option<&str>) -> Vec<(i32, i32)> {
    let mut occupied = Vec::new();
    for enemy in &room.enemies {
        if enemy.hp <= 0 { continue; }
        if let Some(eid) = exclude_id {
            if enemy.id == eid { continue; }
        }
        occupied.push((enemy.x, enemy.y));
    }
    occupied
}

pub fn compute_player_ac(player: &Player) -> i32 {
    let dex_mod = ability_modifier(player.dexterity);
    let mut ac = 10 + dex_mod;
    let mut dex_cap: Option<i32> = None;
    let mut shield_bonus = 0;

    for armour in &player.equipped_armour {
        if armour.armour_category.as_deref() == Some("SHIELD") {
            shield_bonus += armour.ac_bonus.unwrap_or(0);
        } else {
            ac += armour.ac_bonus.unwrap_or(0);
            if let Some(cap) = armour.dex_cap {
                dex_cap = Some(dex_cap.map(|c| c.min(cap)).unwrap_or(cap));
            }
        }
    }

    // Shield in secondary_hand
    if let Some(secondary_id) = &player.secondary_hand {
        if let Some(item) = player.inventory.iter().find(|i| i.instance_id == *secondary_id) {
            if item.handedness.as_deref() == Some("OFF_HAND_ONLY") {
                shield_bonus += item.ac_bonus.unwrap_or(2);
            }
        }
    }

    // Charm AC bonuses from belt utility slots
    if let Some(ref belt) = player.equipped_belt {
        for slot in &player.utility_slots {
            if let Some(ref item) = slot {
                if item.item_class == "CHARM" {
                    shield_bonus += item.ac_bonus.unwrap_or(0);
                }
            }
        }
    }

    let effective_dex = dex_cap.map(|cap| dex_mod.min(cap)).unwrap_or(dex_mod);
    ac = 10 + effective_dex + (ac - 10 - dex_mod) + shield_bonus;
    ac.max(10)
}

pub fn compute_enemy_ac(enemy: &Enemy) -> i32 {
    let dex_mod = ability_modifier(enemy.dexterity);
    let mut ac = 10 + dex_mod;
    let mut dex_cap: Option<i32> = None;
    let mut shield_bonus = 0;

    for armour in &enemy.equipped_armour {
        if armour.armour_category.as_deref() == Some("SHIELD") {
            shield_bonus += armour.ac_bonus.unwrap_or(0);
        } else {
            ac += armour.ac_bonus.unwrap_or(0);
            if let Some(cap) = armour.dex_cap {
                dex_cap = Some(dex_cap.map(|c| c.min(cap)).unwrap_or(cap));
            }
        }
    }

    let effective_dex = dex_cap.map(|cap| dex_mod.min(cap)).unwrap_or(dex_mod);
    ac = 10 + effective_dex + (ac - 10 - dex_mod) + shield_bonus;
    ac.max(10)
}

/// Tracks action economy resources per combatant per turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatResources {
    pub has_action: bool,
    pub has_bonus_action: bool,
    pub has_reaction: bool,
    pub remaining_movement_ft: u32,
    pub is_dodging: bool,
    pub is_disengaging: bool,
    pub has_readied_action: bool,
    pub readied_trigger: Option<String>,
    pub readied_action_type: Option<String>,
    pub readied_target_id: Option<String>,
}

impl CombatResources {
    pub fn new(speed: i32) -> Self {
        Self {
            has_action: true,
            has_bonus_action: true,
            has_reaction: true,
            remaining_movement_ft: speed as u32,
            is_dodging: false,
            is_disengaging: false,
            has_readied_action: false,
            readied_trigger: None,
            readied_action_type: None,
            readied_target_id: None,
        }
    }

    pub fn reset_turn(&mut self, speed: i32) {
        self.has_action = true;
        self.has_bonus_action = true;
        self.remaining_movement_ft = speed as u32;
        self.is_dodging = false;
        self.is_disengaging = false;
    }

    pub fn reset_reaction(&mut self) {
        self.has_reaction = true;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatLogEntry {
    pub round: i32,
    pub actor: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiativeEntry {
    pub id: String,
    pub roll: i32,
    pub bonus: i32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LootGroup {
    pub source_name: String,
    pub gp: i32,
    pub items: Vec<ItemInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub player: Player,
    pub current_room_id: String,
    pub game_mode: GameMode,
    pub last_roll: String,
    pub available_actions: Vec<String>,
    pub rooms: Vec<Room>,
    pub campaign_name: String,
    pub last_combat_event: String,
    pub lore_context: String,
    pub initiative_order: Vec<String>,
    pub current_turn_index: usize,
    pub last_loot: Vec<LootGroup>,
    pub combat_resources: HashMap<String, CombatResources>,
    pub combat_log: Vec<CombatLogEntry>,
    pub round_number: i32,
    pub initiative_entries: Vec<InitiativeEntry>,
    pub spotted_enemy_ids: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub ac: i32,
    pub gp: i32,
    pub strength: i32,
    pub dexterity: i32,
    pub constitution: i32,
    pub intelligence: i32,
    pub wisdom: i32,
    pub charisma: i32,
    pub proficiency_bonus: i32,
    pub inventory: Vec<ItemInstance>,
    pub primary_hand: Option<String>,
    pub secondary_hand: Option<String>,
    pub equipped_armour: Vec<ItemInstance>,
    pub thieves_tools_proficiency: bool,
    pub speed: i32,
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub active_light_source: Option<ActiveLightSource>,
    #[serde(default)]
    pub equipped_belt: Option<ItemInstance>,
    #[serde(default)]
    pub utility_slots: Vec<Option<ItemInstance>>,
    #[serde(default)]
    pub damage_profile: DamageProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemInstance {
    pub instance_id: String,
    pub template_id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub item_class: String,
    pub rarity: i32,
    pub weight: f32,
    pub gp_value: i32,

    #[serde(default)]
    pub damage_dice: Option<String>,
    #[serde(default)]
    pub damage_bonus: Option<i32>,
    #[serde(default)]
    pub weapon_type: Option<String>,

    #[serde(default)]
    pub armor_slot: Option<String>,
    #[serde(default)]
    pub armour_category: Option<String>,
    #[serde(default)]
    pub dex_cap: Option<i32>,
    #[serde(default)]
    pub ac_bonus: Option<i32>,

    #[serde(default)]
    pub effect: Option<String>,

    #[serde(default)]
    pub is_quest_item: bool,
    pub quantity: i32,

    #[serde(default)]
    pub placed_x: Option<i32>,
    #[serde(default)]
    pub placed_y: Option<i32>,
    #[serde(default)]
    pub variant_id: String,
    #[serde(default)]
    pub light_radius: Option<u32>,
    #[serde(default)]
    pub duration_turns: Option<u32>,
    #[serde(default)]
    pub handedness: Option<String>,
    #[serde(default)]
    pub current_fuel: Option<i32>,
    #[serde(default)]
    pub is_lit: Option<bool>,
    #[serde(default)]
    pub max_duration: Option<u32>,
    #[serde(default)]
    pub fuel_restore: Option<u32>,
    #[serde(default)]
    pub tier: Option<i32>,
    #[serde(default)]
    pub damage_type: Option<DamageType>,
    #[serde(default)]
    pub weapon_range: Option<WeaponRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveLightSource {
    pub item_id: String,
    pub radius: u32,
    pub remaining_turns: u32,
    #[serde(default)]
    pub is_belt_mounted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GameMode {
    Exploration,
    Combat,
    GameOver,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerParts {
    pub lock_status: String,
    pub condition: String,
    pub accent_material: String,
    pub core_material: String,
    pub container_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chest {
    pub id: String,
    pub name: String,
    pub locked: bool,
    pub dc: i32,
    pub break_chance: i32,
    pub broken: bool,
    pub loot: Vec<ItemInstance>,
    pub parts: ContainerParts,
    #[serde(default)]
    pub is_revealed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub description: String,
    pub connections: Vec<String>,
    pub traps: Vec<Trap>,
    pub enemies: Vec<Enemy>,
    pub loot: Vec<ItemInstance>,
    pub chests: Vec<Chest>,
    pub hidden_caches: Vec<ItemInstance>,
    pub is_looted: bool,
    pub loot_noticed: bool,
    pub is_trap_triggered: bool,
    pub visited: bool,
    pub tiles: Vec<Vec<Tile>>,
    pub tile_width: i32,
    pub tile_height: i32,
    pub entrance_x: i32,
    pub entrance_y: i32,
    pub room_seed: Option<RoomSeed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trap {
    pub id: String,
    pub name: String,
    pub dc: i32,
    pub damage: String,
    #[serde(default, deserialize_with = "deserialize_damage_type_loose")]
    pub damage_type: DamageType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Enemy {
    pub id: String,
    pub template_id: String,
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub ac: i32,
    pub strength: i32,
    pub dexterity: i32,
    pub constitution: i32,
    pub intelligence: i32,
    pub wisdom: i32,
    pub charisma: i32,
    pub damage_dice: String,
    pub attack_bonus: i32,
    pub xp: i32,
    pub studied: bool,
    pub equipped_armour: Vec<ItemInstance>,
    pub perks: Vec<String>,
    pub loot_table: Vec<crate::campaign::schema::LootDrop>,
    #[serde(default)]
    pub damage_profile: DamageProfile,
    pub speed: i32,
    pub x: i32,
    pub y: i32,
    pub awareness: AwarenessState,
    #[serde(default)]
    pub behaviour: NpcBehaviour,
    #[serde(default = "default_detection_range")]
    pub detection_range: i32,
}

fn default_detection_range() -> i32 { 5 }

impl Enemy {
    pub fn has_perk(&self, perk: &str) -> bool {
        self.perks.iter().any(|p| p == perk)
    }

    pub fn get_damage_bonus(&self) -> i32 {
        let str_mod = ability_modifier(self.strength);
        let dex_mod = ability_modifier(self.dexterity);
        let mut bonus = str_mod.max(dex_mod);
        if self.hp > 0 && self.has_perk("BERSERKER") && self.hp <= self.max_hp / 2 {
            bonus += 2;
        }
        bonus
    }

    pub fn get_effective_attack_bonus(&self) -> i32 {
        let mut bonus = self.attack_bonus;
        if self.hp > 0 && self.has_perk("BERSERKER") && self.hp <= self.max_hp / 2 {
            bonus += 2;
        }
        bonus
    }
}

impl SessionState {
    pub fn new_from_campaign(campaign: &crate::campaign::schema::CampaignData) -> Self {
        let mut rooms = Vec::new();
        
        for room_tmpl in &campaign.map.rooms {
            let mut traps = Vec::new();
            let mut enemies = Vec::new();
            let mut loot = Vec::new();
            let mut chests = Vec::new();
            let mut hidden_caches = Vec::new();
            let mut enemy_specs: Vec<(String, f32)> = Vec::new();
            
            for (_slot_name, slot) in &room_tmpl.procedural_slots {
                let spawns = crate::engine::procedural::evaluate_slot(slot);
                for spawn in spawns {
                    match spawn {
                        crate::campaign::schema::SpawnTemplate::Trap { id, name, dc, damage, damage_type } => {
                            traps.push(Trap { id, name, dc, damage, damage_type });
                        }
                        crate::campaign::schema::SpawnTemplate::GenerateItem { item_class, rarity_min, rarity_max } => {
                            if let Some(item) = crate::engine::procedural::generate_item_instance(campaign, &item_class, rarity_min, rarity_max, None) {
                                loot.push(item);
                            }
                        }
                        crate::campaign::schema::SpawnTemplate::GenerateHiddenLoot { item_class, rarity_min, rarity_max } => {
                            if let Some(item) = crate::engine::procedural::generate_item_instance(campaign, &item_class, rarity_min, rarity_max, None) {
                                hidden_caches.push(item);
                            }
                        }
                        crate::campaign::schema::SpawnTemplate::SpawnEnemy { enemy_id, scale } => {
                            enemy_specs.push((enemy_id, scale));
                        }
                        crate::campaign::schema::SpawnTemplate::SpawnChest { locked_chance, dc, item_class, rarity_min, rarity_max, tier_bias } => {
                            if let Some(chest) = crate::engine::procedural::generate_chest(campaign, locked_chance, dc, &item_class, rarity_min, rarity_max, tier_bias) {
                                chests.push(chest);
                            }
                        }
                    }
                }
            }
            
            // Compute dimensions: use interior + walls when interior fields are set (> 0), else fall back to tile_width/tile_height
            let (tile_width, tile_height, entrance_x, entrance_y) = if room_tmpl.interior_width > 0 && room_tmpl.interior_height > 0 {
                let wt = room_tmpl.wall_thickness;
                (room_tmpl.interior_width + 2 * wt, room_tmpl.interior_height + 2 * wt, wt, wt)
            } else {
                (room_tmpl.tile_width, room_tmpl.tile_height, 1i32, 1i32)
            };

            // Determine seeds: use configured seeds, derive from campaign dungeon seed, or hash room id
            let dungeon_seed = campaign.dungeon_seed;
            let (layout_seed, loot_seed, threat_seed) = if let Some(seeds) = &room_tmpl.room_seeds {
                (seeds.layout_seed, seeds.loot_seed, seeds.threat_seed)
            } else if dungeon_seed != 0 {
                // Derive room seeds from the dungeon seed hierarchy
                let room_namespace = format!("room_{}", room_tmpl.id);
                let room_base = crate::campaign::schema::derive_seed(dungeon_seed, &room_namespace);
                (
                    crate::campaign::schema::derive_seed(room_base, "layout"),
                    crate::campaign::schema::derive_seed(room_base, "loot"),
                    crate::campaign::schema::derive_seed(room_base, "threat"),
                )
            } else {
                let hash = |offset: i32| -> i32 {
                    let s = format!("{}_{}", room_tmpl.id, offset);
                    s.bytes().fold(0i32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as i32))
                };
                (hash(0), hash(1), hash(2))
            };

            let room_seed = RoomSeed {
                layout_roll: layout_seed,
                loot_roll: loot_seed,
                threat_roll: threat_seed,
            };

            let wt = room_tmpl.wall_thickness;
            if room_tmpl.connections.len() > 4 {
                eprintln!("WARN: Room '{}' has {} connections — only the first 4 will have unique door positions, extras map to the same tiles", room_tmpl.id, room_tmpl.connections.len());
            }
            // Generate tile grid with wall thickness
            let mut tiles = crate::engine::procedural::generate_room_layout(layout_seed, tile_width, tile_height, wt);

            // Place Door tiles at connection edge positions and carve a path through the wall thickness
            for (conn_idx, conn_id) in room_tmpl.connections.iter().enumerate() {
                if conn_id.is_empty() { continue; }
                let (dx, dy, carve_axis, carve_dir) = match conn_idx % 4 {
                    0 => (tile_width / 2, 0, 1, 1),            // north: carve south (increase y)
                    1 => (tile_width - 1, tile_height / 2, 0, -1), // east: carve west (decrease x)
                    2 => (tile_width / 2, tile_height - 1, 1, -1), // south: carve north (decrease y)
                    3 => (0, tile_height / 2, 0, 1),            // west: carve east (increase x)
                    _ => continue,
                };
                let carve_depth = wt.max(1);
                for i in 0..carve_depth {
                    let (cx, cy) = if carve_axis == 0 {
                        (dx + carve_dir * i, dy)
                    } else {
                        (dx, dy + carve_dir * i)
                    };
                    if cy >= 0 && cy < tile_height && cx >= 0 && cx < tile_width {
                        tiles[cy as usize][cx as usize].tile_type = TileType::Door;
                    }
                }
            }

            // Place enemies on floor tiles using threat seed
            enemies = crate::engine::procedural::place_enemies_on_tiles(
                threat_seed, campaign, &enemy_specs, &tiles, entrance_x, entrance_y,
                tile_width, tile_height, &room_tmpl.connections,
            );

            // Place loot items on floor tiles
            if !loot.is_empty() {
                let positioned_loot = crate::engine::procedural::place_loot_on_tiles(
                    loot_seed, &campaign, loot, &tiles,
                );
                loot = positioned_loot.into_iter().map(|(lx, ly, mut item)| {
                    item.placed_x = Some(lx);
                    item.placed_y = Some(ly);
                    item
                }).collect();
            }

            // Place hidden caches on floor tiles
            if !hidden_caches.is_empty() {
                let positioned_hidden = crate::engine::procedural::place_loot_on_tiles(
                    loot_seed.wrapping_add(1), &campaign, hidden_caches, &tiles,
                );
                hidden_caches = positioned_hidden.into_iter().map(|(hx, hy, mut item)| {
                    item.placed_x = Some(hx);
                    item.placed_y = Some(hy);
                    item
                }).collect();
            }

            // Place chests on floor tiles using loot seed
            let positioned_chests = crate::engine::procedural::place_chests_on_tiles(
                threat_seed.wrapping_add(1), chests, &tiles, entrance_x, entrance_y,
            );

            // Convert positioned chests back to regular chests (store position)
            let mut placed_chests = Vec::new();
            for (cx, cy, mut ch) in positioned_chests {
                // Store position in chest name for now
                ch.name = format!("{} [{}:{}]", ch.name, cx, cy);
                placed_chests.push(ch);
            }
            
            let is_start = room_tmpl.id == campaign.map.rooms.first().map(|r| r.id.as_str()).unwrap_or("");
            rooms.push(Room {
                id: room_tmpl.id.clone(),
                name: room_tmpl.name.clone(),
                description: room_tmpl.description.clone(),
                connections: room_tmpl.connections.clone(),
                traps,
                enemies,
                loot,
                chests: placed_chests,
                hidden_caches,
                is_looted: false,
                loot_noticed: false,
                is_trap_triggered: false,
                visited: is_start,
                tiles,
                tile_width,
                tile_height,
                entrance_x,
                entrance_y,
                room_seed: Some(room_seed),
            });
        }
        
        let mut starting_inventory: Vec<ItemInstance> = Vec::new();
        let mut starting_armour: Vec<ItemInstance> = Vec::new();
        let mut primary_hand_id = None;
        
        for s_item in &campaign.main.player_template.starting_inventory {
            let item_class = match campaign.items.base_items.get(&s_item.id) {
                Some(bi) => bi.item_class.clone(),
                None => continue,
            };
            
            if let Some(mut inst) = crate::engine::procedural::generate_item_instance(campaign, &item_class, 2, 2, Some(&s_item.id)) {
                inst.quantity = s_item.quantity;
                if s_item.id == campaign.main.player_template.starting_equipped_weapon {
                    primary_hand_id = Some(inst.instance_id.clone());
                }
                if inst.item_class == "ARMOR" {
                    starting_armour.push(inst);
                } else {
                    add_to_inventory(&mut starting_inventory, inst);
                }
            }
        }
        
        // Auto-equip belt from starting inventory
        let (equipped_belt, utility_slots) = {
            let belt_idx = starting_inventory.iter().position(|i| i.item_class == "BELT");
            if let Some(idx) = belt_idx {
                let belt = starting_inventory.remove(idx);
                let slots = belt.tier.map(|t| match t { 4 => 3, 3 => 2, _ => 1 }).unwrap_or(0);
                let mut us = Vec::new();
                for _ in 0..slots {
                    us.push(None);
                }
                (Some(belt), us)
            } else {
                (None, vec![])
            }
        };

        let lore_context = format!(
            "World: {}. Tone: {}. Lore: {}. Factions: {:?}. Rules: {:?}",
            campaign.lore.world_name, campaign.lore.tone, campaign.lore.setting_lore, campaign.lore.factions, campaign.lore.narrative_rules
        );
        
        let player_speed = 30;
        let start_x = 1i32;
        let start_y = 1i32;
        
        let mut new_state = Self {
            player: Player {
                name: "Adventurer".to_string(),
                hp: campaign.main.player_template.starting_hp,
                max_hp: campaign.main.player_template.starting_hp,
                ac: campaign.main.player_template.starting_ac,
                gp: campaign.main.player_template.starting_gp,
                strength: campaign.main.player_template.starting_strength,
                dexterity: campaign.main.player_template.starting_dexterity,
                constitution: campaign.main.player_template.starting_constitution,
                intelligence: campaign.main.player_template.starting_intelligence,
                wisdom: campaign.main.player_template.starting_wisdom,
                charisma: campaign.main.player_template.starting_charisma,
                proficiency_bonus: campaign.main.player_template.starting_proficiency_bonus,
                inventory: starting_inventory,
                primary_hand: primary_hand_id,
                secondary_hand: None,
                equipped_armour: starting_armour,
                thieves_tools_proficiency: campaign.main.player_template.starting_thieves_tools,
                speed: player_speed,
                x: start_x,
                y: start_y,
                active_light_source: None,
                equipped_belt,
                utility_slots,
                damage_profile: DamageProfile::default(),
            },
            current_room_id: campaign.map.rooms.first().map(|r| r.id.clone()).unwrap_or_default(),
            game_mode: GameMode::Exploration,
            last_roll: "None".to_string(),
            available_actions: vec![],
            rooms,
            campaign_name: campaign.main.campaign_name.clone(),
            last_combat_event: "".to_string(),
            lore_context,
            initiative_order: vec![],
            current_turn_index: 0,
            last_loot: vec![],
            combat_resources: HashMap::new(),
            combat_log: vec![],
            round_number: 0,
            initiative_entries: vec![],
            spotted_enemy_ids: HashSet::new(),
        };
        
        new_state.generate_available_actions();
        new_state
    }

    pub fn log_combat(&mut self, text: String) {
        let entry = CombatLogEntry {
            round: self.round_number,
            actor: self.get_current_turn_id().cloned().unwrap_or_default(),
            text,
        };
        self.combat_log.push(entry);
    }

    pub fn generate_available_actions(&mut self) {
        let mut actions = vec![];
        
        if self.game_mode == GameMode::GameOver {
            self.available_actions = vec![];
            return;
        }

        let is_player_turn = if self.game_mode == GameMode::Combat {
            self.get_current_turn_id().map(|id| id == "player").unwrap_or(true)
        } else {
            true
        };

        if self.game_mode == GameMode::Combat && !is_player_turn {
            self.available_actions = vec![];
            return;
        }

        if let Some(room) = self.get_current_room() {
            // ── No enemies in room: free exploration ──
            if room.enemies.iter().all(|e| e.hp <= 0) {
                let px = self.player.x;
                let py = self.player.y;
                if room.tiles.is_empty() == false {
                    let tiles = &room.tiles;
                    if py > 0 && tiles.get((py - 1) as usize)
                        .and_then(|row| row.get(px as usize))
                        .is_some_and(|t| t.tile_type != TileType::Wall)
                    {
                        actions.push("MOVE_NORTH".to_string());
                    }
                    if py < room.tile_height as i32 - 1 && tiles.get((py + 1) as usize)
                        .and_then(|row| row.get(px as usize))
                        .is_some_and(|t| t.tile_type != TileType::Wall)
                    {
                        actions.push("MOVE_SOUTH".to_string());
                    }
                    if px > 0 && tiles.get(py as usize)
                        .and_then(|row| row.get((px - 1) as usize))
                        .is_some_and(|t| t.tile_type != TileType::Wall)
                    {
                        actions.push("MOVE_WEST".to_string());
                    }
                    if px < room.tile_width as i32 - 1 && tiles.get(py as usize)
                        .and_then(|row| row.get((px + 1) as usize))
                        .is_some_and(|t| t.tile_type != TileType::Wall)
                    {
                        actions.push("MOVE_EAST".to_string());
                    }
                }

                if !room.loot.is_empty() {
                    for item in &room.loot {
                        let in_range = item.placed_x.is_some() && item.placed_y.is_some()
                            && (item.placed_x.unwrap() - self.player.x).abs() <= 1
                            && (item.placed_y.unwrap() - self.player.y).abs() <= 1;
                        let is_visible = in_range && item.placed_x.is_some() && item.placed_y.is_some()
                            && room.tiles.get(item.placed_y.unwrap() as usize)
                                .and_then(|row| row.get(item.placed_x.unwrap() as usize))
                                .map(|t| t.visibility == TileVisibility::Visible)
                                .unwrap_or(false);
                        if is_visible {
                            actions.push(format!("TAKE_ITEM_{}", item.instance_id));
                        }
                    }
                }

                for chest in &room.chests {
                    if !chest.broken && chest.is_revealed {
                        let (cx, cy) = parse_chest_position(&chest.name);
                        let in_range = (cx - self.player.x).abs() <= 1 && (cy - self.player.y).abs() <= 1;
                        if !in_range { continue; }
                        if chest.locked {
                            actions.push(format!("PICK_LOCK_{}", chest.id));
                        } else {
                            actions.push(format!("OPEN_CHEST_{}", chest.id));
                        }
                    }
                }
            } else {
                let visible_enemies: Vec<&Enemy> = get_visible_enemies(room);

                if self.game_mode == GameMode::Combat {
                    let player_res = self.combat_resources.get("player");
                    let can_do_action = player_res.map(|r| r.has_action).unwrap_or(true);
                    let can_do_bonus = player_res.map(|r| r.has_bonus_action).unwrap_or(true);
                    let has_movement = player_res.map(|r| r.remaining_movement_ft >= TILE_SIZE_FEET as u32).unwrap_or(false);

                    if has_movement {
                        let px = self.player.x;
                        let py = self.player.y;
                        if py > 0 && room.tiles.get((py - 1) as usize)
                            .and_then(|row| row.get(px as usize))
                            .is_some_and(|t| t.tile_type != TileType::Wall)
                        {
                            actions.push("MOVE_NORTH".to_string());
                        }
                        if py < room.tile_height as i32 - 1 && room.tiles.get((py + 1) as usize)
                            .and_then(|row| row.get(px as usize))
                            .is_some_and(|t| t.tile_type != TileType::Wall)
                        {
                            actions.push("MOVE_SOUTH".to_string());
                        }
                        if px > 0 && room.tiles.get(py as usize)
                            .and_then(|row| row.get((px - 1) as usize))
                            .is_some_and(|t| t.tile_type != TileType::Wall)
                        {
                            actions.push("MOVE_WEST".to_string());
                        }
                        if px < room.tile_width as i32 - 1 && room.tiles.get(py as usize)
                            .and_then(|row| row.get((px + 1) as usize))
                            .is_some_and(|t| t.tile_type != TileType::Wall)
                        {
                            actions.push("MOVE_EAST".to_string());
                        }
                    }

                    if can_do_action {
                        for enemy in &visible_enemies {
                            actions.push(format!("ACTION_ATTACK_{}", enemy.id));
                        }
                        actions.push("ACTION_DASH".to_string());
                        actions.push("ACTION_DODGE".to_string());
                        actions.push("ACTION_DISENGAGE".to_string());
                        actions.push("ACTION_HIDE".to_string());
                        actions.push("ACTION_READY".to_string());
                        for enemy in &visible_enemies {
                            if !enemy.studied {
                                actions.push(format!("ACTION_STUDY_{}", enemy.id));
                            }
                        }
                    }

                    if can_do_bonus {
                        if let Some(w) = self.get_equipped_weapon() {
                            if w.weapon_type.as_deref() == Some("light")
                                || w.item_class == "WEAPON" && w.template_id.contains("light")
                            {
                                let has_offhand = self.player.inventory.iter().any(|i| {
                                    i.instance_id != self.player.primary_hand.as_deref().unwrap_or("")
                                        && i.instance_id != self.player.secondary_hand.as_deref().unwrap_or("")
                                        && (i.weapon_type.as_deref() == Some("light")
                                            || i.item_class == "WEAPON" && i.template_id.contains("light"))
                                });
                                if has_offhand {
                                    for enemy in &visible_enemies {
                                        actions.push(format!("BONUS_OFFHAND_ATTACK_{}", enemy.id));
                                    }
                                }
                            }
                        }
                    }

                    actions.push("ACTION_END_TURN".to_string());
                    actions.push("ACTION_FLEE".to_string());
                } else {
                    // Exploration mode with enemies present (pre-combat)
                    // Player can move freely until they walk into an enemy's visible area
                    let px = self.player.x;
                    let py = self.player.y;
                    if room.tiles.is_empty() == false {
                        let tiles = &room.tiles;
                        if py > 0 && tiles.get((py - 1) as usize)
                            .and_then(|row| row.get(px as usize))
                            .is_some_and(|t| t.tile_type != TileType::Wall)
                        {
                            actions.push("MOVE_NORTH".to_string());
                        }
                        if py < room.tile_height as i32 - 1 && tiles.get((py + 1) as usize)
                            .and_then(|row| row.get(px as usize))
                            .is_some_and(|t| t.tile_type != TileType::Wall)
                        {
                            actions.push("MOVE_SOUTH".to_string());
                        }
                        if px > 0 && tiles.get(py as usize)
                            .and_then(|row| row.get((px - 1) as usize))
                            .is_some_and(|t| t.tile_type != TileType::Wall)
                        {
                            actions.push("MOVE_WEST".to_string());
                        }
                        if px < room.tile_width as i32 - 1 && tiles.get(py as usize)
                            .and_then(|row| row.get((px + 1) as usize))
                            .is_some_and(|t| t.tile_type != TileType::Wall)
                        {
                            actions.push("MOVE_EAST".to_string());
                        }
                    }

                    for enemy in &visible_enemies {
                        actions.push(format!("ATTACK_{}", enemy.id));
                        if !enemy.studied {
                            actions.push(format!("STUDY_{}", enemy.id));
                        }
                    }
                }
            }
        }
        
        for item in &self.player.inventory {
            if item.quantity > 0 {
                let in_hand = self.player.primary_hand.as_deref() == Some(&item.instance_id)
                    || self.player.secondary_hand.as_deref() == Some(&item.instance_id);
                if !in_hand && ((item.item_class == "CONSUMABLE" && item.effect.is_some()) || item.template_id == "torch" || (item.template_id == "lantern" && item.is_lit != Some(true))) {
                    actions.push(format!("USE_ITEM_{}", item.instance_id));
                }
                if item.item_class == "WEAPON" || item.item_class == "MELEE" || item.item_class == "MAGIC" || item.item_class == "RANGED" || item.handedness.is_some() {
                    let already_equipped = self.player.primary_hand.as_deref() == Some(&item.instance_id)
                        || self.player.secondary_hand.as_deref() == Some(&item.instance_id);
                    if !already_equipped {
                        actions.push(format!("EQUIP_ITEM_{}", item.instance_id));
                        actions.push(format!("EQUIP_ITEM_PRIMARY_{}", item.instance_id));
                        actions.push(format!("EQUIP_ITEM_SECONDARY_{}", item.instance_id));
                    }
                }
                if item.item_class == "ARMOR" {
                    let already_equipped = self.player.equipped_armour.iter()
                        .any(|a| a.armor_slot == item.armor_slot);
                    if !already_equipped {
                        actions.push(format!("EQUIP_ARMOUR_{}", item.instance_id));
                    }
                }
            }
        }
        
        if is_player_turn {
            for armour in &self.player.equipped_armour {
                actions.push(format!("UNEQUIP_ARMOUR_{}", armour.instance_id));
            }
            if self.player.primary_hand.is_some() {
                actions.push("UNEQUIP_HAND_PRIMARY".to_string());
            }
            if self.player.secondary_hand.is_some() {
                actions.push("UNEQUIP_HAND_SECONDARY".to_string());
            }
        }

        // Pick up ground torch if on or adjacent to one
        if let Some(room) = self.get_current_room() {
            let px = self.player.x;
            let py = self.player.y;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let tx = px + dx;
                    let ty = py + dy;
                    if tx >= 0 && tx < room.tile_width && ty >= 0 && ty < room.tile_height {
                        if room.tiles[ty as usize][tx as usize].ground_light_source.is_some() {
                            actions.push(format!("PICK_UP_TORCH_{}_{}", tx, ty));
                        }
                    }
                }
            }
        }

        // Refill lantern action if player has a lantern and an oil flask
        let has_oil_flask = self.player.inventory.iter().any(|i| i.template_id == "oil_flask" && i.quantity > 0);
        if has_oil_flask {
            for item in &self.player.inventory {
                if item.template_id == "lantern" {
                    let in_hand = self.player.primary_hand.as_deref() == Some(&item.instance_id)
                        || self.player.secondary_hand.as_deref() == Some(&item.instance_id);
                    if in_hand || item.quantity > 0 {
                        actions.push(format!("REFILL_LANTERN_{}", item.instance_id));
                    }
                }
            }
        }

        // Belt equip/unequip actions
        if self.player.equipped_belt.is_some() {
            actions.push("UNEQUIP_BELT".to_string());
        }
        for item in &self.player.inventory {
            if item.item_class == "BELT" && item.quantity > 0 {
                let already_equipped = self.player.equipped_belt.as_ref()
                    .map(|b| b.instance_id == item.instance_id)
                    .unwrap_or(false);
                if !already_equipped {
                    actions.push(format!("EQUIP_BELT_{}", item.instance_id));
                }
            }
        }

        // Utility slot mount/unmount actions
        let slot_capacity = self.player.equipped_belt.as_ref()
            .and_then(|b| b.tier.map(|t| match t { 4 => 3, 3 => 2, _ => 1 }))
            .unwrap_or(0);
        let has_belt_in_inventory = self.player.inventory.iter().any(|i| i.item_class == "BELT" && i.quantity > 0);
        let effective_slots = if slot_capacity > 0 { slot_capacity } else if has_belt_in_inventory { 1 } else { 0 };
        if effective_slots > 0 {
            // Show mount actions for items that can go into utility slots
            for item in &self.player.inventory {
                if item.quantity > 0 {
                    let can_mount = item.template_id == "torch"
                        || item.template_id == "lantern"
                        || item.item_class == "CHARM";
                    if can_mount {
                        let already_mounted = self.player.utility_slots.iter()
                            .any(|s| s.as_ref().map(|i| i.instance_id == item.instance_id).unwrap_or(false));
                        let belt_item = self.player.equipped_belt.as_ref()
                            .map(|b| b.instance_id == item.instance_id)
                            .unwrap_or(false);
                        if !already_mounted && !belt_item {
                            for slot_idx in 0..effective_slots {
                                if slot_idx >= self.player.utility_slots.len() || self.player.utility_slots.get(slot_idx).and_then(|s| s.as_ref()).is_none() {
                                    actions.push(format!("MOUNT_UTILITY_{}_{}", slot_idx, item.instance_id));
                                }
                            }
                        }
                    }
                }
            }
            for hand_id in [&self.player.primary_hand, &self.player.secondary_hand].iter().filter_map(|h| h.as_ref()) {
                if let Some(item) = self.player.inventory.iter().find(|i| i.instance_id == *hand_id) {
                    let can_mount = item.template_id == "torch"
                        || item.template_id == "lantern"
                        || item.item_class == "CHARM";
                    if can_mount {
                        let already_mounted = self.player.utility_slots.iter()
                            .any(|s| s.as_ref().map(|i| i.instance_id == item.instance_id).unwrap_or(false));
                        if !already_mounted {
                            for slot_idx in 0..effective_slots {
                                if slot_idx >= self.player.utility_slots.len() || self.player.utility_slots.get(slot_idx).and_then(|s| s.as_ref()).is_none() {
                                    actions.push(format!("MOUNT_UTILITY_{}_{}", slot_idx, item.instance_id));
                                }
                            }
                        }
                    }
                }
            }
        }

        self.available_actions = actions;
    }

    pub fn apply_damage(&mut self, target: &str, amount: i32, damage_type: DamageType) -> Result<(), String> {
        if target == "player" {
            let adjusted = crate::engine::combat::apply_resistance(amount, damage_type, &self.player.damage_profile);
            self.player.hp -= adjusted;
            if self.player.hp <= 0 {
                self.player.hp = 0;
                self.game_mode = GameMode::GameOver;
            }
            Ok(())
        } else {
            if let Some(room) = self.get_current_room_mut() {
                if let Some(enemy) = room.enemies.iter_mut().find(|e| e.id == target) {
                    let adjusted = crate::engine::combat::apply_resistance(amount, damage_type, &enemy.damage_profile);
                    enemy.hp -= adjusted;
                    if enemy.hp <= 0 && enemy.has_perk("UNDEAD_FORTITUDE") {
                        let dc = 5 + amount.max(0);
                        let con_mod = ability_modifier(enemy.constitution);
                        let save = (std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .subsec_nanos() as i32 % 20).abs() + 1 + con_mod;
                        if save >= dc {
                            enemy.hp = 1;
                        }
                    }
                    return Ok(());
                }
            }
            Err(format!("Unknown target: {}", target))
        }
    }

    pub fn apply_heal(&mut self, target: &str, amount: i32) -> Result<(), String> {
        if target == "player" {
            self.player.hp += amount;
            if self.player.hp > self.player.max_hp {
                self.player.hp = self.player.max_hp;
            }
            Ok(())
        } else {
            Err(format!("Unknown target: {}", target))
        }
    }

    pub fn get_current_room(&self) -> Option<&Room> {
        self.rooms.iter().find(|r| r.id == self.current_room_id)
    }

    pub fn get_current_room_mut(&mut self) -> Option<&mut Room> {
        self.rooms.iter_mut().find(|r| r.id == self.current_room_id)
    }

    pub fn get_equipped_weapon(&self) -> Option<&ItemInstance> {
        self.player
            .primary_hand
            .as_ref()
            .and_then(|id| self.player.inventory.iter().find(|i| i.instance_id == *id))
    }

    pub fn get_offhand_weapon(&self) -> Option<&ItemInstance> {
        self.player
            .secondary_hand
            .as_ref()
            .and_then(|id| self.player.inventory.iter().find(|i| i.instance_id == *id))
    }

    pub fn get_current_turn_id(&self) -> Option<&String> {
        self.initiative_order.get(self.current_turn_index)
    }

    pub fn has_enemies_in_room(&self) -> bool {
        self.get_current_room()
            .map(|r| r.enemies.iter().any(|e| e.hp > 0))
            .unwrap_or(false)
    }

    /// Equip an inventory item to the correct hand slot based on handedness.
    /// For TWO_HANDED: clears secondary_hand (extinguishes if lit).
    /// For OFF_HAND_ONLY: if primary_hand has a 2H weapon, clears primary_hand.
    /// For ONE_HANDED: fills primary_hand first, then secondary_hand.
    fn handle_light_source_on_swap(&mut self, item_id: &str) -> Option<String> {
        let light = self.player.active_light_source.clone()?;
        let (px, py) = (self.player.x, self.player.y);
        let weapon_name = self.player.inventory.iter()
            .find(|i| i.instance_id == item_id)
            .map(|i| i.display_name.clone())
            .unwrap_or_else(|| "weapon".to_string());
        match light.item_id.as_str() {
            "torch" => {
                let room = self.get_current_room_mut()?;
                let target = if room.tiles[py as usize][px as usize].ground_light_source.is_none() {
                    (px, py)
                } else {
                    let offsets = [(0,-1),(0,1),(-1,0),(1,0),(-1,-1),(-1,1),(1,-1),(1,1)];
                    let mut found = None;
                    for (dx, dy) in &offsets {
                        let tx = px + dx;
                        let ty = py + dy;
                        if tx >= 0 && tx < room.tile_width && ty >= 0 && ty < room.tile_height {
                            if room.tiles[ty as usize][tx as usize].tile_type != TileType::Wall
                                && room.tiles[ty as usize][tx as usize].ground_light_source.is_none()
                            {
                                found = Some((tx, ty));
                                break;
                            }
                        }
                    }
                    found.unwrap_or((px, py))
                };
                room.tiles[target.1 as usize][target.0 as usize].ground_light_source = Some(light);
                println!("[DEBUG handle_light_source_on_swap] Dropping torch to ground, active_light_source = None");
                self.player.active_light_source = None;
                Some(format!("You drop your burning torch onto the ground to wield your {}, keeping the area illuminated!", weapon_name))
            }
            "lantern" => {
                println!("[DEBUG handle_light_source_on_swap] Stowing lantern, active_light_source = None");
                self.player.active_light_source = None;
                let item_name = self.player.inventory.iter()
                    .find(|i| i.instance_id == item_id)
                    .map(|i| i.display_name.clone())
                    .unwrap_or_else(|| "lantern".to_string());
                Some(format!("You stow your {} to wield your {}.", item_name, weapon_name))
            }
            _ => None,
        }
    }

    pub fn equip_to_slot(&mut self, item_id: &str) -> Option<String> {
        let handedness = self.player.inventory.iter()
            .find(|i| i.instance_id == item_id)
            .and_then(|i| i.handedness.clone())
            .unwrap_or_else(|| "ONE_HANDED".to_string());

        let mut log_message = None;

        match handedness.as_str() {
            "TWO_HANDED" => {
                let swap_msg = self.handle_light_source_on_swap(item_id);
                if swap_msg.is_some() {
                    log_message = swap_msg;
                }
                let old_secondary = self.player.secondary_hand.clone();
                self.player.secondary_hand = None;
                self.player.primary_hand = Some(item_id.to_string());
                if let Some(ref id) = old_secondary {
                    self.extinguish_light_source(id);
                }
            }
            "OFF_HAND_ONLY" => {
                let ph = self.player.primary_hand.clone();
                if let Some(ref primary_id) = ph {
                    if self.player.inventory.iter()
                        .any(|i| i.instance_id == *primary_id && i.handedness.as_deref() == Some("TWO_HANDED"))
                    {
                        self.player.primary_hand = None;
                    }
                }
                self.player.secondary_hand = Some(item_id.to_string());
            }
            _ => {
                if self.player.primary_hand.is_none() {
                    self.player.primary_hand = Some(item_id.to_string());
                } else {
                    self.player.secondary_hand = Some(item_id.to_string());
                }
            }
        }

        log_message
    }

    /// Extinguish any active light source associated with the given item instance.
    pub fn extinguish_light_source(&mut self, instance_id: &str) {
        if let Some(active) = &self.player.active_light_source {
            let is_active = active.item_id == "torch" || active.item_id == "lantern";
            if is_active {
                let in_hand = self.player.primary_hand.as_deref() == Some(instance_id)
                    || self.player.secondary_hand.as_deref() == Some(instance_id);
                let in_belt = self.player.utility_slots.iter()
                    .any(|s| s.as_ref().map(|i| i.instance_id == instance_id).unwrap_or(false));
                if in_hand || in_belt {
                    println!("[DEBUG extinguish_light_source] Extinguishing {} (instance={}, in_hand={}, in_belt={})", active.item_id, instance_id, in_hand, in_belt);
                    self.player.active_light_source = None;
                    // Mark the item as no longer lit
                    if let Some(item) = self.player.inventory.iter_mut().find(|i| i.instance_id == instance_id) {
                        item.is_lit = Some(false);
                    }
                    if let Some(item) = self.player.utility_slots.iter_mut().flatten().find(|i| i.instance_id == instance_id) {
                        item.is_lit = Some(false);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_loose_str_canonical() {
        assert_eq!(DamageType::from_loose_str("piercing"), DamageType::Piercing);
    }

    #[test]
    fn test_from_loose_str_synonym() {
        assert_eq!(DamageType::from_loose_str("FROST"), DamageType::Cold);
    }

    #[test]
    fn test_from_loose_str_fallback() {
        let result = DamageType::from_loose_str("glorbulon");
        assert_eq!(result, DamageType::Bludgeoning);
    }

    #[test]
    fn test_trap_deserialize_canonical_lowercase() {
        let json = r#"{"id":"pit_trap","name":"Pit Trap","dc":10,"damage":"1d6","damage_type":"piercing"}"#;
        let t: Trap = serde_json::from_str(json).unwrap();
        assert_eq!(t.damage_type, DamageType::Piercing);
    }

    #[test]
    fn test_trap_deserialize_typo_fallback() {
        let json = r#"{"id":"pit_trap","name":"Pit Trap","dc":10,"damage":"1d6","damage_type":"peircing"}"#;
        let t: Trap = serde_json::from_str(json).unwrap();
        assert_eq!(t.damage_type, DamageType::Bludgeoning);
    }

    #[test]
    fn test_trap_deserialize_missing_field_defaults() {
        let json = r#"{"id":"pit_trap","name":"Pit Trap","dc":10,"damage":"1d6"}"#;
        let t: Trap = serde_json::from_str(json).unwrap();
        assert_eq!(t.damage_type, DamageType::Bludgeoning);
    }

    #[test]
    fn test_item_instance_damage_type_some() {
        let item = ItemInstance {
            damage_type: Some(DamageType::Slashing),
            ..ItemInstance::default_for_test()
        };
        assert_eq!(item.damage_type, Some(DamageType::Slashing));
    }

    #[test]
    fn test_item_instance_damage_type_none() {
        let item = ItemInstance {
            damage_type: None,
            ..ItemInstance::default_for_test()
        };
        assert_eq!(item.damage_type, None);
    }

    #[test]
    fn test_item_instance_damage_type_deserialize_default() {
        // Simulate an old save with no damage_type field
        let json = r#"{"instance_id":"test","template_id":"leather_armor","display_name":"Leather Armor","item_class":"ARMOR","rarity":1,"weight":5.0,"gp_value":10,"quantity":1,"variant_id":""}"#;
        let item: ItemInstance = serde_json::from_str(json).unwrap();
        assert_eq!(item.damage_type, None);
    }

    #[test]
    fn test_damage_type_display() {
        assert_eq!(format!("{}", DamageType::Fire), "Fire");
        assert_eq!(format!("{}", DamageType::Slashing), "Slashing");
        assert_eq!(format!("{}", DamageType::Necrotic), "Necrotic");
        assert_eq!(DamageType::default().to_string(), "Bludgeoning");
    }

    #[test]
    fn test_damage_profile_empty_default() {
        let p: DamageProfile = serde_json::from_str("{}").unwrap();
        assert!(p.resistances.is_empty());
        assert!(p.vulnerabilities.is_empty());
        assert!(p.immunities.is_empty());
    }

    #[test]
    fn test_damage_profile_partial_deserialize() {
        let p: DamageProfile = serde_json::from_str(r#"{"resistances":["FIRE"],"immunities":["POISON"]}"#).unwrap();
        assert_eq!(p.resistances, vec![DamageType::Fire]);
        assert!(p.vulnerabilities.is_empty());
        assert_eq!(p.immunities, vec![DamageType::Poison]);
    }

    #[test]
    fn test_damage_profile_default_eq_empty_json() {
        let empty_json: DamageProfile = serde_json::from_str("{}").unwrap();
        assert_eq!(DamageProfile::default(), empty_json);
    }

    #[test]
    fn test_apply_damage_enemy_resistance_halves_damage() {
        let mut state = SessionState {
            player: Player {
                name: "Test".into(), hp: 20, max_hp: 20, ac: 10, gp: 0,
                strength: 10, dexterity: 10, constitution: 10,
                intelligence: 10, wisdom: 10, charisma: 10,
                proficiency_bonus: 2, speed: 30, x: 0, y: 0,
                primary_hand: None, secondary_hand: None,
                equipped_armour: vec![], thieves_tools_proficiency: false,
                active_light_source: None, equipped_belt: None,
                utility_slots: vec![], inventory: vec![],
                damage_profile: DamageProfile::default(),
            },
            current_room_id: "room_1".into(),
            game_mode: GameMode::Exploration,
            last_roll: String::new(), available_actions: vec![],
            campaign_name: String::new(), last_combat_event: String::new(),
            lore_context: String::new(),
            rooms: vec![Room {
                id: "room_1".into(), name: "Test".into(), description: String::new(),
                connections: vec![], traps: vec![],
                enemies: vec![Enemy {
                    id: "enemy_1".into(), template_id: "test".into(),
                    name: "Test Enemy".into(), hp: 20, max_hp: 20, ac: 10,
                    strength: 10, dexterity: 10, constitution: 10,
                    intelligence: 10, wisdom: 10, charisma: 10,
                    damage_dice: "1d4".into(), attack_bonus: 0, xp: 0,
                    studied: false, equipped_armour: vec![], perks: vec![],
                    loot_table: vec![],
                    damage_profile: DamageProfile {
                        resistances: vec![DamageType::Bludgeoning],
                        ..Default::default()
                    },
                    speed: 30, x: 0, y: 0, awareness: AwarenessState::Unaware,
                    behaviour: NpcBehaviour::Idle, detection_range: 5,
                }],
                loot: vec![], chests: vec![], hidden_caches: vec![],
                is_looted: false, loot_noticed: false,
                is_trap_triggered: false, visited: false,
                tile_width: 1, tile_height: 1, entrance_x: 0, entrance_y: 0,
                room_seed: None,
                tiles: vec![vec![Tile {
                    x: 0, y: 0, tile_type: TileType::Floor,
                    visibility: TileVisibility::Visible,
                    ground_light_source: None,
                }]],
            }],
            initiative_order: vec![], current_turn_index: 0,
            last_loot: vec![],
            combat_resources: HashMap::new(), combat_log: vec![],
            round_number: 0, initiative_entries: vec![],
            spotted_enemy_ids: HashSet::new(),
        };

        state.apply_damage("enemy_1", 10, DamageType::Bludgeoning).unwrap();

        let enemy = state.rooms[0].enemies.iter().find(|e| e.id == "enemy_1").unwrap();
        assert_eq!(enemy.hp, 15, "10 bludgeoning vs resistance should deal 5 damage");
    }
}

impl ItemInstance {
    pub(crate) fn default_for_test() -> Self {
        ItemInstance {
            instance_id: String::new(),
            template_id: String::new(),
            display_name: String::new(),
            description: None,
            item_class: String::new(),
            rarity: 0,
            weight: 0.0,
            gp_value: 0,
            damage_dice: None,
            damage_bonus: None,
            weapon_type: None,
            armor_slot: None,
            armour_category: None,
            dex_cap: None,
            ac_bonus: None,
            effect: None,
            is_quest_item: false,
            quantity: 0,
            placed_x: None,
            placed_y: None,
            variant_id: String::new(),
            light_radius: None,
            duration_turns: None,
            handedness: None,
            current_fuel: None,
            is_lit: None,
            max_duration: None,
            fuel_restore: None,
            tier: None,
            damage_type: None,
            weapon_range: None,
        }
    }
}
