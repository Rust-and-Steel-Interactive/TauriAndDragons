use crate::engine::state::{DamageType, WeaponRange};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ==========================================
// Master Campaign Data (Loaded by loader.rs)
// ==========================================
#[derive(Debug, Clone)]
pub struct CampaignData {
    pub main: CampaignMain,
    pub items: ItemRegistry,
    pub modifiers: ModifierConfig,
    pub enemies: EnemyRegistry,
    pub lore: LoreTemplate,
    pub map: GameMap,
    /// Master seed for deterministic campaign generation
    pub campaign_seed: i32,
    /// Derived seeds for each level of the hierarchy
    pub world_seed: i32,
    pub region_seed: i32,
    pub dungeon_seed: i32,
}

/// Derive a child seed from a parent seed using a namespace string,
/// producing independent but deterministic child seeds.
pub fn derive_seed(parent: i32, namespace: &str) -> i32 {
    let s = format!("{}_{}", parent, namespace);
    s.bytes().fold(0i32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as i32))
}

/// Generate the full seed hierarchy from a master campaign seed.
pub fn derive_seed_hierarchy(master_seed: i32) -> (i32, i32, i32, i32) {
    let world = derive_seed(master_seed, "world");
    let region = derive_seed(world, "region");
    let dungeon = derive_seed(region, "dungeon");
    (world, region, dungeon, master_seed)
}

// ==========================================
// 1. main.json
// ==========================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignMain {
    pub campaign_id: String,
    pub campaign_name: String,
    pub description: String,
    pub starting_map: String,
    pub player_template: PlayerTemplate,
    #[serde(default = "default_campaign_seed")]
    pub campaign_seed: i32,
    #[serde(default)]
    pub container_parts: Option<ContainerPartsTable>,
}

fn default_campaign_seed() -> i32 { 0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerPartEntry {
    pub name: String,
    pub weight: f64,
    pub tier: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerPartsTable {
    #[serde(default)]
    pub lock_statuses: Vec<ContainerPartEntry>,
    #[serde(default)]
    pub conditions: Vec<ContainerPartEntry>,
    #[serde(default)]
    pub accent_materials: Vec<ContainerPartEntry>,
    #[serde(default)]
    pub core_materials: Vec<ContainerPartEntry>,
    #[serde(default)]
    pub container_types: Vec<ContainerPartEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerTemplate {
    pub starting_hp: i32,
    pub starting_ac: i32,
    pub starting_gp: i32,
    pub starting_strength: i32,
    pub starting_dexterity: i32,
    pub starting_constitution: i32,
    pub starting_intelligence: i32,
    pub starting_wisdom: i32,
    pub starting_charisma: i32,
    pub starting_proficiency_bonus: i32,
    pub starting_inventory: Vec<StartingItem>,
    pub starting_equipped_weapon: String,
    #[serde(default)]
    pub starting_thieves_tools: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartingItem {
    pub id: String,
    pub quantity: i32,
}

// ==========================================
// 2. items.json
// ==========================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemRegistry {
    pub base_items: HashMap<String, BaseItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseItem {
    pub name: String,
    pub item_class: String,
    #[serde(default)]
    pub base_damage_dice: Option<String>,
    #[serde(default)]
    pub weight: f32,
    pub base_value: i32,
    #[serde(default)]
    pub armor_slot: Option<String>,
    #[serde(default)]
    pub base_ac_bonus: Option<i32>,
    #[serde(default)]
    pub armour_category: Option<String>,
    #[serde(default)]
    pub dex_cap: Option<i32>,
    #[serde(default)]
    pub base_effect: Option<String>,
    #[serde(default)]
    pub light_radius: Option<u32>,
    #[serde(default)]
    pub duration_turns: Option<u32>,
    #[serde(default)]
    pub handedness: Option<String>,
    #[serde(default)]
    pub max_duration: Option<u32>,
    #[serde(default)]
    pub fuel_restore: Option<u32>,
    #[serde(default)]
    pub tier: Option<i32>,
    #[serde(default)]
    pub utility_slots: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_optional_damage_type_loose")]
    pub damage_type: Option<DamageType>,
    #[serde(default)]
    pub weapon_range: Option<WeaponRange>,
    /// For a RANGED weapon: the ammo type it consumes (e.g. "arrow").
    /// For an AMMO-class item: its own ammo type identity (e.g. "arrow").
    /// None on everything else.
    #[serde(default)]
    pub ammo_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveLightSource {
    pub item_id: String,
    pub radius: u32,
    pub remaining_turns: u32,
}

// ==========================================
// 3. modifiers.json
// ==========================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifierConfig {
    #[serde(flatten)]
    pub classes: HashMap<String, ClassModifierPool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassModifierPool {
    #[serde(default)]
    pub quality: HashMap<String, ModifierTier>,
    #[serde(default)]
    pub material: HashMap<String, ModifierTier>,
    #[serde(default)]
    pub component: HashMap<String, ModifierTier>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifierTier {
    pub name: String,
    #[serde(default)]
    pub stat_bonus: Option<i32>,
    pub value_mult: f32,
    #[serde(default)]
    pub effect: Option<String>,
    #[serde(default)]
    pub effect_mult: Option<f32>,
    #[serde(default)]
    pub effect_target: Option<String>,
}

// ==========================================
// 4. lore_template.json
// ==========================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoreTemplate {
    pub world_name: String,
    pub tone: String,
    pub setting_lore: String,
    pub factions: Vec<Faction>,
    pub key_locations: Vec<LoreLocation>,
    pub narrative_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Faction {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoreLocation {
    pub name: String,
    pub description: String,
}

// ==========================================
// 5. enemies.json
// ==========================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyRegistry {
    pub base_enemies: HashMap<String, BaseEnemy>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyArmourConfig {
    pub slots: Vec<String>,
    #[serde(default)]
    pub rarity_min: i32,
    #[serde(default)]
    pub rarity_max: i32,
    #[serde(default = "default_drop_chance")]
    pub drop_chance: i32,
}

fn default_drop_chance() -> i32 { 50 }

fn default_speed() -> i32 { 30 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseEnemy {
    pub name: String,
    pub enemy_class: String,
    pub base_hp: i32,
    pub base_ac: i32,
    pub base_damage_dice: String,
    pub base_attack_bonus: i32,
    pub base_xp: i32,
    pub strength: i32,
    pub dexterity: i32,
    pub constitution: i32,
    pub intelligence: i32,
    pub wisdom: i32,
    pub charisma: i32,
    #[serde(default)]
    pub armour_config: Option<EnemyArmourConfig>,
    #[serde(default)]
    pub perks: Vec<String>,
    #[serde(default)]
    pub loot_table: Vec<LootDrop>,
    #[serde(default)]
    pub damage_profile: crate::engine::state::DamageProfile,
    #[serde(default = "default_speed")]
    pub speed: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LootDrop {
    pub drop_chance: i32, // 0 to 100
    pub item_class: String,
    pub rarity_min: i32,
    pub rarity_max: i32,
}

// ==========================================
// 6. maps/level_1.json
// ==========================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameMap {
    pub map_id: String,
    pub rooms: Vec<RoomTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub connections: Vec<String>,
    #[serde(default)]
    pub procedural_slots: HashMap<String, ProceduralSlot>,
    #[serde(default = "default_room_template")]
    pub room_template: String,
    #[serde(default = "default_interior_width")]
    pub interior_width: i32,
    #[serde(default = "default_interior_height")]
    pub interior_height: i32,
    #[serde(default = "default_wall_thickness")]
    pub wall_thickness: i32,
    #[serde(default = "default_tile_width")]
    pub tile_width: i32,
    #[serde(default = "default_tile_height")]
    pub tile_height: i32,
    #[serde(default)]
    pub room_seeds: Option<RoomSeedConfig>,
}

fn default_room_template() -> String { "medium_room".to_string() }
fn default_interior_width() -> i32 { 0 }
fn default_interior_height() -> i32 { 0 }
fn default_wall_thickness() -> i32 { 1 }
fn default_tile_width() -> i32 { 20 }
fn default_tile_height() -> i32 { 16 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSeedConfig {
    pub layout_seed: i32,
    pub loot_seed: i32,
    pub threat_seed: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralSlot {
    pub roll: String,
    pub results: HashMap<String, Vec<SpawnTemplate>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SpawnTemplate {
    Trap {
        id: String,
        name: String,
        dc: i32,
        damage: String,
        #[serde(default, deserialize_with = "deserialize_damage_type_loose")]
        damage_type: DamageType,
    },
    GenerateItem {
        item_class: String,
        rarity_min: i32,
        rarity_max: i32,
    },
    GenerateHiddenLoot {
        item_class: String,
        rarity_min: i32,
        rarity_max: i32,
    },
    SpawnEnemy {
        enemy_id: String,
        scale: f32,
    },
    SpawnChest {
        locked_chance: i32,
        dc: i32,
        item_class: String,
        rarity_min: i32,
        rarity_max: i32,
        #[serde(default)]
        tier_bias: f64,
    },
}

fn deserialize_optional_damage_type_loose<'de, D>(deserializer: D) -> Result<Option<DamageType>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.map(|s| DamageType::from_loose_str(&s)))
}

fn deserialize_damage_type_loose<'de, D>(deserializer: D) -> Result<DamageType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(DamageType::from_loose_str(&s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_template_trap_typo_damage_type_falls_back() {
        let json = r#"{"type":"TRAP","id":"t1","name":"Test Trap","dc":10,"damage":"1d6","damage_type":"peircing"}"#;
        let spawn: SpawnTemplate = serde_json::from_str(json).unwrap();
        match spawn {
            SpawnTemplate::Trap { damage_type, .. } => {
                assert_eq!(damage_type, DamageType::Bludgeoning);
            }
            _ => panic!("Expected Trap variant"),
        }
    }

    #[test]
    fn test_base_item_typo_damage_type_falls_back_to_some_bludgeoning() {
        let json = r#"{"name":"Test Weapon","item_class":"MELEE","base_damage_dice":"1d6","base_value":10,"damage_type":"peircing"}"#;
        let item: BaseItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.damage_type, Some(DamageType::Bludgeoning));
    }

    #[test]
    fn test_base_item_missing_damage_type_defaults_to_none() {
        let json = r#"{"name":"Test Weapon","item_class":"MELEE","base_damage_dice":"1d6","base_value":10}"#;
        let item: BaseItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.damage_type, None);
    }

    #[test]
    fn test_weapon_range_deserialize_canonical() {
        let json = r#"{"name":"Test Bow","item_class":"RANGED","base_value":10,"weight":1.0,"weapon_range":{"normal":16,"long":64}}"#;
        let item: BaseItem = serde_json::from_str(json).unwrap();
        let wr = item.weapon_range.expect("weapon_range should be Some");
        assert_eq!(wr.normal, 16);
        assert_eq!(wr.long, Some(64));
    }

    #[test]
    fn test_weapon_range_deserialize_normal_only() {
        let json = r#"{"name":"Test Dagger","item_class":"MELEE","base_value":5,"weight":1.0,"weapon_range":{"normal":1}}"#;
        let item: BaseItem = serde_json::from_str(json).unwrap();
        let wr = item.weapon_range.expect("weapon_range should be Some");
        assert_eq!(wr.normal, 1);
        assert_eq!(wr.long, None);
    }

    #[test]
    fn test_weapon_range_missing_key_defaults_to_none() {
        // Backward-compatibility: existing items without weapon_range parse with None
        let json = r#"{"name":"Test Sword","item_class":"MELEE","base_value":10,"weight":2.0}"#;
        let item: BaseItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.weapon_range, None);
    }
}