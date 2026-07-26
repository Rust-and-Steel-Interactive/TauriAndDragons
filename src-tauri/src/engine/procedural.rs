use crate::campaign::schema::{CampaignData, SpawnTemplate, BaseItem};
use crate::engine::state::{self, ItemInstance, Enemy, Chest, TileType, Tile, AwarenessState};
use rand::Rng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

type SeededRng = ChaCha8Rng;

fn seeded_rng(seed: i32) -> SeededRng {
    let seed_bytes: [u8; 32] = {
        let mut bytes = [0u8; 32];
        let seed_str = format!("tnd_tactical_{}", seed);
        let seed_hash = seed_str.bytes().collect::<Vec<_>>();
        for i in 0..32.min(seed_hash.len()) {
            bytes[i] = seed_hash[i];
        }
        bytes
    };
    SeededRng::from_seed(seed_bytes)
}

pub fn generate_room_layout(seed: i32, width: i32, height: i32, wall_thickness: i32) -> Vec<Vec<Tile>> {
    let mut rng = seeded_rng(seed);
    let mut grid: Vec<Vec<Tile>> = Vec::new();

    for y in 0..height {
        let mut row: Vec<Tile> = Vec::new();
        for x in 0..width {
            let tile_type = if x < wall_thickness || y < wall_thickness
                || x >= width - wall_thickness || y >= height - wall_thickness
            {
                TileType::Wall
            } else {
                let roll: f64 = rng.gen();
                if roll < 0.15 {
                    TileType::Rubble
                } else {
                    TileType::Floor
                }
            };
            row.push(Tile {
                x,
                y,
                tile_type,
                visibility: state::TileVisibility::Unknown,
                ground_light_source: None,
            });
        }
        grid.push(row);
    }

    // Clear entrances at interior corners (inside the walls)
    let ex = wall_thickness;
    let ey = wall_thickness;
    grid[ey as usize][ex as usize].tile_type = TileType::Floor;
    grid[(height - wall_thickness - 1) as usize][(width - wall_thickness - 1) as usize].tile_type = TileType::Floor;

    grid
}

pub fn get_floor_tiles(tiles: &[Vec<Tile>]) -> Vec<(i32, i32)> {
    let mut floors = Vec::new();
    for row in tiles {
        for tile in row {
            if tile.tile_type == TileType::Floor {
                floors.push((tile.x, tile.y));
            }
        }
    }
    floors
}

pub fn get_floor_tiles_excluding(
    tiles: &[Vec<Tile>],
    exclude_points: &[(i32, i32)],
    radius: i32,
) -> Vec<(i32, i32)> {
    let mut floors = Vec::new();
    for row in tiles {
        for tile in row {
            if tile.tile_type != TileType::Floor { continue; }
            let mut excluded = false;
            for &(ex, ey) in exclude_points {
                if (tile.x - ex).abs() <= radius && (tile.y - ey).abs() <= radius {
                    excluded = true;
                    break;
                }
            }
            if !excluded {
                floors.push((tile.x, tile.y));
            }
        }
    }
    floors
}

fn get_door_positions(tile_width: i32, tile_height: i32, connections: &[String]) -> Vec<(i32, i32)> {
    let mut doors = Vec::new();
    for (conn_idx, conn_id) in connections.iter().enumerate() {
        if conn_id.is_empty() { continue; }
        let (dx, dy) = match conn_idx % 4 {
            0 => (tile_width / 2, 0),                     // north
            1 => (tile_width - 1, tile_height / 2),       // east
            2 => (tile_width / 2, tile_height - 1),       // south
            3 => (0, tile_height / 2),                    // west
            _ => continue,
        };
        if dy >= 0 && dy < tile_height && dx >= 0 && dx < tile_width {
            doors.push((dx, dy));
        }
    }
    doors
}

pub fn place_enemies_on_tiles(
    seed: i32,
    campaign: &CampaignData,
    enemy_specs: &[(String, f32)],
    tiles: &[Vec<Tile>],
    entrance_x: i32,
    entrance_y: i32,
    tile_width: i32,
    tile_height: i32,
    connections: &[String],
) -> Vec<Enemy> {
    let mut rng = seeded_rng(seed);
    let door_positions = get_door_positions(tile_width, tile_height, connections);
    let mut exclude_points = Vec::new();
    exclude_points.push((entrance_x, entrance_y));
    for dp in &door_positions {
        exclude_points.push(*dp);
    }
    let available = get_floor_tiles_excluding(tiles, &exclude_points, 3);
    let mut enemies = Vec::new();

    for (enemy_id, scale) in enemy_specs {
        if let Some(mut enemy) = generate_enemy_instance(campaign, enemy_id, *scale) {
            if let Some(&(ex, ey)) = available.choose(&mut rng) {
                enemy.x = ex;
                enemy.y = ey;
                enemy.awareness = AwarenessState::Unaware;
            } else {
                enemy.x = entrance_x + 2;
                enemy.y = entrance_y + 2;
            }

            // Assign a simple patrol route: two waypoints flanking the enemy's spawn point
            let wx = enemy.x;
            let wy = enemy.y;
            // Pick two nearby floor tiles as waypoints (W-E or N-S pair, 2-3 tiles apart)
            let waypoints = vec![
                (wx.max(2) - 1, wy),
                (wx.min(tile_width - 3) + 1, wy),
            ];
            enemy.behaviour = state::NpcBehaviour::Patrol {
                waypoints,
                current_index: 0,
            };
            enemy.detection_range = 5;

            enemies.push(enemy);
        }
    }
    enemies
}

pub fn place_loot_on_tiles(
    seed: i32,
    campaign: &CampaignData,
    items: Vec<state::ItemInstance>,
    tiles: &[Vec<Tile>],
) -> Vec<(i32, i32, state::ItemInstance)> {
    let mut rng = seeded_rng(seed);
    let available = get_floor_tiles(tiles);
    let mut positioned = Vec::new();

    for item in items {
        if let Some(&(px, py)) = available.choose(&mut rng) {
            positioned.push((px, py, item));
        }
    }
    positioned
}

pub fn place_chests_on_tiles(
    seed: i32,
    chests_in: Vec<Chest>,
    tiles: &[Vec<Tile>],
    entrance_x: i32,
    entrance_y: i32,
) -> Vec<(i32, i32, Chest)> {
    let mut rng = seeded_rng(seed);
    let available = get_floor_tiles_excluding(tiles, &[(entrance_x, entrance_y)], 2);
    let mut positioned = Vec::new();

    for chest in chests_in {
        // Place chests near walls
        let wall_adjacent: Vec<(i32, i32)> = available.iter()
            .filter(|&&(x, y)| {
                x == 1 || y == 1 || x == (tiles[0].len() as i32 - 2) || y == (tiles.len() as i32 - 2)
            })
            .copied()
            .collect();

        if let Some(&(px, py)) = wall_adjacent.choose(&mut rng).or_else(|| available.choose(&mut rng)) {
            positioned.push((px, py, chest));
        }
    }
    positioned
}

pub fn roll_dice(expression: &str) -> i32 {
    let parts: Vec<&str> = expression.split('d').collect();
    if parts.len() != 2 { return 0; }
    let count: i32 = parts[0].parse().unwrap_or(1);
    let rest = parts[1];
    
    let (sides_str, bonus_str) = match rest.find('+') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => match rest.find('-') {
            Some(pos) => (&rest[..pos], &rest[pos..]),
            None => (rest, ""),
        }
    };
    
    let sides: i32 = sides_str.parse().unwrap_or(1);
    let bonus: i32 = bonus_str.parse().unwrap_or(0);
    
    let mut rng = rand::thread_rng();
    (1..=count).map(|_| rng.gen_range(1..=sides)).sum::<i32>() + bonus
}

fn roll_matches_key(roll: i32, key: &str) -> bool {
    if key.contains('-') {
        let range: Vec<&str> = key.split('-').collect();
        if range.len() == 2 {
            let min: i32 = range[0].parse().unwrap_or(0);
            let max: i32 = range[1].parse().unwrap_or(0);
            return roll >= min && roll <= max;
        }
        false
    } else {
        roll == key.parse().unwrap_or(-1)
    }
}

// FIX: Removed unused `campaign` parameter
pub fn evaluate_slot(slot: &crate::campaign::schema::ProceduralSlot) -> Vec<SpawnTemplate> {
    let roll = roll_dice(&slot.roll);
    println!("🎲 Procedural roll for slot: {} = {}", slot.roll, roll);
    
    for (key, spawns) in &slot.results {
        if roll_matches_key(roll, key) {
            return spawns.clone();
        }
    }
    vec![]
}

fn pick_tier(rarity_min: i32, rarity_max: i32) -> i32 {
    let mut rng = rand::thread_rng();
    let min = rarity_min.max(1).min(4);
    let max = (rarity_max.max(1).min(4)).max(min).min(min + 2);
    rng.gen_range(min..=max)
}

// Update the function signature to accept specific_item_id
fn generate_tool_name(tier_val: i32, base_name: &str) -> String {
    match tier_val {
        1 => format!("Crude {}", base_name),
        2 => base_name.to_string(),
        3 => format!("Superior {}", base_name),
        4 => format!("Masterwork {}", base_name),
        _ => base_name.to_string(),
    }
}

/// Returns the quality bonus for a TOOL item based on its rarity tier.
pub fn get_tool_quality_bonus(rarity: i32) -> i32 {
    match rarity {
        1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        _ => 0,
    }
}

pub fn generate_item_instance(
    campaign: &CampaignData, 
    item_class: &str, 
    rarity_min: i32, 
    rarity_max: i32, 
    specific_item_id: Option<&str>
) -> Option<ItemInstance> {
    let mut rng = rand::thread_rng();
    
    // If we requested a specific item, use it. Otherwise, pick random from the class.
    let (base_id, base_item) = if let Some(id) = specific_item_id {
        let base = campaign.items.base_items.get(id)?;
        (id.to_string(), base)
    } else {
        let possible_items: Vec<_> = campaign.items.base_items.iter()
            .filter(|(_, item)| item.item_class == item_class)
            .collect();

        if possible_items.is_empty() {
            return None;
        }
        let (chosen_id, chosen_item) = possible_items.choose(&mut rng)?;
        (chosen_id.to_string(), *chosen_item)
    };

    // For specific items (like starting gear), force Tier 2 (Standard)
    let tier_val = if specific_item_id.is_some() { 2 } else { pick_tier(rarity_min, rarity_max) };
    
    // TOOL class: torches use modifier pool for flame/wood; other tools use simple quality naming
    if base_item.item_class == "TOOL" {
        if base_id == "torch" {
            let q_tier = if specific_item_id.is_some() { 2 } else { pick_tier(rarity_min, rarity_max) };
            let m_tier = if specific_item_id.is_some() { 2 } else { pick_tier(rarity_min, rarity_max) };
            let c_tier = if specific_item_id.is_some() { 2 } else { pick_tier(rarity_min, rarity_max) };

            let class_pool = campaign.modifiers.classes.get("TOOL");
            let q_mod = class_pool.and_then(|p| p.quality.get(&q_tier.to_string()));
            let m_mod = class_pool.and_then(|p| p.material.get(&m_tier.to_string()));
            let c_mod = class_pool.and_then(|p| p.component.get(&c_tier.to_string()));

            let flame_name = c_mod.map(|c| c.name.as_str()).unwrap_or("Standard");
            let wood_name = m_mod.map(|m| m.name.as_str()).unwrap_or("Wooden");
            let quality_prefix = q_mod.map(|q| q.name.as_str()).unwrap_or("Standard");

            let display_name = format!("{} {} Torch", flame_name, wood_name);
            let value_mult = q_mod.map(|q| q.value_mult).unwrap_or(1.0)
                * m_mod.map(|m| m.value_mult).unwrap_or(1.0)
                * c_mod.map(|c| c.value_mult).unwrap_or(1.0);

            let base_radius = base_item.light_radius.unwrap_or(3);
            let base_duration = base_item.duration_turns.unwrap_or(60);
            let radius_bonus = c_mod.and_then(|c| c.stat_bonus).unwrap_or(0);
            let duration_mult = m_mod.and_then(|m| m.effect_mult).unwrap_or(1.0);

            let light_radius = (base_radius as i32 + radius_bonus).max(1) as u32;
            let duration_turns = (base_duration as f32 * duration_mult).round().max(1.0) as u32;

            let gp_value = (base_item.base_value as f32 * value_mult) as i32;
            let description = Some(format!(
                "A {} torch made of {}. Flame: {}, Wood: {}. Burns for {} turns with a light radius of {}.",
                quality_prefix.to_lowercase(), wood_name.to_lowercase(), flame_name, wood_name, duration_turns, light_radius
            ));

            let instance_id = format!("proc_{}", &uuid::Uuid::new_v4().to_string()[..8]);
            let v_id = display_name.clone();

            return Some(ItemInstance {
                instance_id,
                template_id: base_id.to_string(),
                display_name,
                description,
                item_class: base_item.item_class.clone(),
                rarity: q_tier,
                weight: base_item.weight,
                gp_value,
                damage_dice: None,
                damage_bonus: None,
                weapon_type: None,
                armor_slot: None,
                armour_category: None,
                dex_cap: None,
                ac_bonus: None,
                effect: None,
                is_quest_item: false,
                quantity: 1,
                placed_x: None,
                placed_y: None,
                variant_id: v_id,
                light_radius: Some(light_radius),
                duration_turns: Some(duration_turns),
                handedness: base_item.handedness.clone(),
                current_fuel: None,
                is_lit: None,
                max_duration: base_item.max_duration,
                fuel_restore: base_item.fuel_restore,
                tier: base_item.tier,
                damage_type: base_item.damage_type,
                weapon_range: base_item.weapon_range.clone(),
                ammo_type: base_item.ammo_type.clone(),
                discipline: base_item.discipline.clone(),
                known_spell_ids: base_item.known_spell_ids.clone(),
                scroll_spell_id: base_item.scroll_spell_id.clone(),
                innate_spell_id: base_item.innate_spell_id.clone(),
            });
        } else {
            let display_name = generate_tool_name(tier_val, &base_item.name);

            let gp_value = (base_item.base_value as f32 * (tier_val as f32 * 0.5).max(1.0)) as i32;
            let description = Some(format!("{} with a quality bonus of +{}.",
                display_name, get_tool_quality_bonus(tier_val)));
            let instance_id = format!("proc_{}", &uuid::Uuid::new_v4().to_string()[..8]);
            let v_id = display_name.clone();
            
            return Some(ItemInstance {
                instance_id,
                template_id: base_id.to_string(),
                display_name,
                description,
                item_class: base_item.item_class.clone(),
                rarity: tier_val,
                weight: base_item.weight,
                gp_value,
                damage_dice: None,
                damage_bonus: None,
                weapon_type: None,
                armor_slot: None,
                armour_category: None,
                dex_cap: None,
                ac_bonus: None,
                effect: None,
                is_quest_item: false,
                quantity: 1,
                placed_x: None,
                placed_y: None,
                variant_id: v_id,
                light_radius: base_item.light_radius,
                duration_turns: None,
                handedness: base_item.handedness.clone(),
                current_fuel: if base_item.max_duration.is_some() { Some(base_item.max_duration.unwrap_or(0) as i32) } else { None },
                is_lit: if base_item.max_duration.is_some() { Some(false) } else { None },
                max_duration: base_item.max_duration,
                fuel_restore: base_item.fuel_restore,
                tier: base_item.tier,
                damage_type: base_item.damage_type,
                weapon_range: base_item.weapon_range.clone(),
                ammo_type: base_item.ammo_type.clone(),
                discipline: base_item.discipline.clone(),
                known_spell_ids: base_item.known_spell_ids.clone(),
                scroll_spell_id: base_item.scroll_spell_id.clone(),
                innate_spell_id: base_item.innate_spell_id.clone(),
            });
        }
    }
    
    let class_pool = campaign.modifiers.classes.get(item_class)?;
    
    // For specific items (like starting gear), force Tier 2 (Standard)
    let q_tier = if specific_item_id.is_some() { "2".to_string() } else { pick_tier(rarity_min, rarity_max).to_string() };
    let m_tier = if specific_item_id.is_some() { "2".to_string() } else { pick_tier(rarity_min, rarity_max).to_string() };
    let c_tier = if specific_item_id.is_some() { "2".to_string() } else { pick_tier(rarity_min, rarity_max).to_string() };
    
    let q_mod = class_pool.quality.get(&q_tier);
    let m_mod = class_pool.material.get(&m_tier);
    let c_mod = class_pool.component.get(&c_tier);
    
    let mut stat_bonus = 0;
    let mut value_mult = 1.0;
    let mut display_name = String::new();
    
    if let Some(q) = q_mod {
        stat_bonus += q.stat_bonus.unwrap_or(0);
        value_mult *= q.value_mult;
        display_name = format!("{} {}", q.name, display_name);
    }
    if let Some(c) = c_mod {
        stat_bonus += c.stat_bonus.unwrap_or(0);
        value_mult *= c.value_mult;
        display_name = format!("{} {}", display_name, c.name);
    }
    if let Some(m) = m_mod {
        stat_bonus += m.stat_bonus.unwrap_or(0);
        value_mult *= m.value_mult;
        display_name = format!("{} {}", display_name, m.name);
    }
    
    display_name = format!("{} {}", display_name.trim(), base_item.name);
    
    let gp_value = (base_item.base_value as f32 * value_mult) as i32;
    
    // Generate description
    let description = Some(generate_item_description(&base_item, q_mod, m_mod, c_mod, tier_val, base_item.damage_type));

    let instance_id = format!("proc_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let v_id = display_name.clone();
    
    Some(ItemInstance {
        instance_id,
        template_id: base_id.to_string(),
        display_name,
        description,
        item_class: base_item.item_class.clone(),
        rarity: tier_val,
        weight: base_item.weight,
        gp_value,
        damage_dice: base_item.base_damage_dice.clone(),
        damage_bonus: Some(stat_bonus),
        weapon_type: Some(base_item.item_class.clone()),
        armor_slot: base_item.armor_slot.clone(),
        armour_category: base_item.armour_category.clone(),
        dex_cap: base_item.dex_cap,
        ac_bonus: base_item.base_ac_bonus.map(|ac| ac + stat_bonus),
        effect: base_item.base_effect.clone(),
        is_quest_item: false,
        quantity: 1,
        placed_x: None,
        placed_y: None,
        variant_id: v_id,
        light_radius: None,
        duration_turns: None,
        handedness: base_item.handedness.clone(),
        current_fuel: if base_item.max_duration.is_some() { Some(base_item.max_duration.unwrap_or(0) as i32) } else { None },
        is_lit: if base_item.max_duration.is_some() { Some(false) } else { None },
        max_duration: base_item.max_duration,
        fuel_restore: base_item.fuel_restore,
        tier: base_item.tier,
        damage_type: base_item.damage_type,
        weapon_range: base_item.weapon_range.clone(),
        ammo_type: base_item.ammo_type.clone(),
        discipline: base_item.discipline.clone(),
        known_spell_ids: base_item.known_spell_ids.clone(),
        scroll_spell_id: base_item.scroll_spell_id.clone(),
        innate_spell_id: base_item.innate_spell_id.clone(),
    })
}

pub fn generate_enemy_armour(campaign: &CampaignData, config: &crate::campaign::schema::EnemyArmourConfig) -> Vec<ItemInstance> {
    let mut rng = rand::thread_rng();
    let mut armour = Vec::new();
    for slot in &config.slots {
        if rng.gen_range(1..=100) > config.drop_chance {
            continue;
        }
        let armour_items: Vec<(&String, &BaseItem)> = campaign.items.base_items.iter()
            .filter(|(_, item)| item.item_class == "ARMOR" && item.armor_slot.as_deref() == Some(slot.as_str()))
            .collect();
        if armour_items.is_empty() {
            continue;
        }
        if let Some((id, _)) = armour_items.choose(&mut rng) {
            if let Some(item) = generate_item_instance(campaign, "ARMOR", config.rarity_min, config.rarity_max, Some(id)) {
                armour.push(item);
            }
        }
    }
    armour
}

const RARITY_NAMES: [&str; 5] = ["", "Crude", "Standard", "Fine", "Masterwork"];
const RARITY_DESC: [&str; 5] = [
    "",
    "A hastily made, barely functional piece.",
    "Serviceable and well-maintained.",
    "Expertly crafted with care and precision.",
    "A masterpiece of the craft, fit for a hero.",
];

pub fn generate_item_description(
    base_item: &BaseItem,
    q_mod: Option<&crate::campaign::schema::ModifierTier>,
    m_mod: Option<&crate::campaign::schema::ModifierTier>,
    c_mod: Option<&crate::campaign::schema::ModifierTier>,
    rarity: i32,
    damage_type: Option<crate::engine::state::DamageType>,
) -> String {
    let item_type = match base_item.item_class.as_str() {
        "MELEE" => "weapon",
        "MAGIC" => "arcane implement",
        "RANGED" => "ranged weapon",
        "ARMOR" => {
            match base_item.armor_slot.as_deref() {
                Some("CHEST") => "chest piece",
                Some("HEAD") => "helm",
                Some("HANDS") => "gloves",
                Some("FEET") => "boots",
                Some("SHIELD") => "shield",
                _ => "armour piece",
            }
        }
        "CONSUMABLE" => "consumable",
        "VALUABLE" => "valuable",
        _ => "item",
    };

    let rarity_label = RARITY_NAMES.get(rarity as usize).copied().unwrap_or("Strange");
    let rarity_flavor = RARITY_DESC.get(rarity as usize).copied().unwrap_or("Unknown origin.");
    let quality_name = q_mod.map(|q| q.name.as_str()).unwrap_or("Standard");
    let material_name = m_mod.map(|m| m.name.as_str()).unwrap_or("Common");
    let component_name = c_mod.map(|c| c.name.as_str()).unwrap_or("");

    let dmg_bonus = q_mod.and_then(|q| q.stat_bonus).unwrap_or(0)
        + c_mod.and_then(|c| c.stat_bonus).unwrap_or(0)
        + m_mod.and_then(|m| m.stat_bonus).unwrap_or(0);

    let ac_bonus = base_item.base_ac_bonus.unwrap_or(0) + dmg_bonus;

    let subtitle = match base_item.item_class.as_str() {
        "MELEE" | "MAGIC" | "RANGED" => {
            format!("A {} {} {}, {}.",
                quality_name.to_lowercase(),
                material_name.to_lowercase(),
                item_type,
                if item_type == "arcane implement" { "humming with latent energy" } else { "solid and balanced" }
            )
        }
        "ARMOR" => {
            let slot_desc = match base_item.armor_slot.as_deref() {
                Some("CHEST") => "protecting the torso",
                Some("HEAD") => "guarding the head",
                Some("HANDS") => "covering the hands",
                Some("FEET") => "reinforcing the feet",
                Some("SHIELD") => "ready to block",
                _ => "providing protection",
            };
            format!("A {} {} armour piece, {}.",
                quality_name.to_lowercase(),
                material_name.to_lowercase(),
                slot_desc
            )
        }
        "CONSUMABLE" => {
            format!("A {} potion in a small glass vial.", rarity_label.to_lowercase())
        }
        "VALUABLE" => {
            format!("A {} trinket of modest worth.", rarity_label.to_lowercase())
        }
        _ => {
            format!("A {} {}.", rarity_label.to_lowercase(), item_type)
        }
    };

    let mut lines: Vec<String> = Vec::new();
    lines.push(base_item.name.clone());
    lines.push(String::new());
    lines.push(subtitle);
    lines.push(String::new());

    if let Some(dice) = &base_item.base_damage_dice {
        let dmg_str = if dmg_bonus > 0 {
            format!("{} + {}", dice, dmg_bonus)
        } else {
            dice.clone()
        };
        let dtype = damage_type.unwrap_or_default();
        lines.push(format!("Damage: {} {}", dmg_str, dtype));
    }
    if base_item.base_ac_bonus.is_some() {
        lines.push(format!("Armour Class: +{}", ac_bonus));
    }
    lines.push(format!("Quality: {}", quality_name));
    if !material_name.is_empty() && material_name != "Common" {
        lines.push(format!("Material: {}", material_name));
    }
    if !component_name.is_empty() {
        if let Some(c) = c_mod {
            if let Some(effect) = &c.effect {
                lines.push(format!("Special: {}", effect.replace("_", " ")));
            }
        }
    }
    if let Some(effect) = &base_item.base_effect {
        if effect.starts_with("HEAL") {
            let dice_str = effect.trim_start_matches("HEAL_");
            lines.push(format!("Effect: Restores {} HP when consumed.", dice_str));
        } else {
            lines.push(format!("Effect: {}", effect));
        }
    }
    lines.push(format!("Weight: {:.1} lb", base_item.weight));
    lines.push(format!("Value: {} gp", (base_item.base_value as f32) as i32));

    if rarity >= 4 {
        lines.push(String::new());
        lines.push("This item radiates exceptional power. Its true history remains untold.".to_string());
    } else {
        lines.push(String::new());
        lines.push(rarity_flavor.to_string());
    }

    lines.join("\n")
}

pub fn generate_chest(
    campaign: &CampaignData,
    locked_chance: i32,
    dc: i32,
    item_class: &str,
    rarity_min: i32,
    rarity_max: i32,
    tier_bias: f64,
) -> Option<Chest> {
    let mut rng = rand::thread_rng();
    let locked = rng.gen_range(1..=100) <= locked_chance;
    let mut loot = Vec::new();

    let item_count = rng.gen_range(1..=3);
    for _ in 0..item_count {
        if let Some(item) = generate_item_instance(campaign, item_class, rarity_min, rarity_max, None) {
            loot.push(item);
        }
    }

    let chest_id = format!("chest_{}", &uuid::Uuid::new_v4().to_string()[..6]);
    let (parts, name) = generate_container_name(&mut rng, &campaign.main.container_parts, tier_bias);

    let break_chance = match parts.lock_status.as_str() {
        "Rusty Lock" => 40,
        "Heavy Bolted Lock" => 15,
        _ => 50,
    };

    Some(Chest {
        id: chest_id,
        name,
        locked,
        dc,
        break_chance,
        broken: false,
        loot,
        parts,
        is_revealed: false,
    })
}

// ==========================================
// Container Parts System
// ==========================================

fn select_weighted_part<'a, R: Rng>(rng: &mut R, parts: &'a [crate::campaign::schema::ContainerPartEntry], bias: f64) -> &'a str {
    let total: f64 = parts.iter().map(|p| p.weight * (p.tier as f64).powf(bias)).sum();
    if total == 0.0 { return &parts[0].name; }
    let roll: f64 = rng.gen_range(0.0..total);
    let mut cumulative = 0.0;
    for p in parts {
        let effective = p.weight * (p.tier as f64).powf(bias);
        cumulative += effective;
        if roll < cumulative {
            return &p.name;
        }
    }
    parts.last().unwrap().name.as_str()
}

fn get_container_table(table: &Option<crate::campaign::schema::ContainerPartsTable>) -> &crate::campaign::schema::ContainerPartsTable {
    use std::sync::OnceLock;
    static DEFAULT: OnceLock<crate::campaign::schema::ContainerPartsTable> = OnceLock::new();
    DEFAULT.get_or_init(|| {
        use crate::campaign::schema::ContainerPartEntry;
        crate::campaign::schema::ContainerPartsTable {
            lock_statuses: vec![
                ContainerPartEntry { name: "Simple Latch".into(), weight: 50.0, tier: 1 },
                ContainerPartEntry { name: "Rusty Lock".into(), weight: 35.0, tier: 2 },
                ContainerPartEntry { name: "Heavy Bolted Lock".into(), weight: 15.0, tier: 3 },
            ],
            conditions: vec![
                ContainerPartEntry { name: "Rotting".into(), weight: 15.0, tier: 1 },
                ContainerPartEntry { name: "Standard".into(), weight: 40.0, tier: 2 },
                ContainerPartEntry { name: "Excellent".into(), weight: 30.0, tier: 3 },
                ContainerPartEntry { name: "Pristine".into(), weight: 15.0, tier: 4 },
            ],
            accent_materials: vec![
                ContainerPartEntry { name: "Leather-Wrapped".into(), weight: 40.0, tier: 1 },
                ContainerPartEntry { name: "Iron-Banded".into(), weight: 30.0, tier: 2 },
                ContainerPartEntry { name: "Bronze-Studded".into(), weight: 20.0, tier: 3 },
                ContainerPartEntry { name: "Gem-Inlaid".into(), weight: 10.0, tier: 4 },
            ],
            core_materials: vec![
                ContainerPartEntry { name: "Oak".into(), weight: 35.0, tier: 1 },
                ContainerPartEntry { name: "Pine".into(), weight: 30.0, tier: 2 },
                ContainerPartEntry { name: "Ironwood".into(), weight: 20.0, tier: 3 },
                ContainerPartEntry { name: "Elderwood".into(), weight: 15.0, tier: 4 },
            ],
            container_types: vec![
                ContainerPartEntry { name: "Chest".into(), weight: 30.0, tier: 1 },
                ContainerPartEntry { name: "Crate".into(), weight: 25.0, tier: 1 },
                ContainerPartEntry { name: "Barrel".into(), weight: 20.0, tier: 1 },
                ContainerPartEntry { name: "Strongbox".into(), weight: 15.0, tier: 2 },
                ContainerPartEntry { name: "Trunk".into(), weight: 10.0, tier: 2 },
            ],
        }
    });
    table.as_ref().unwrap_or_else(|| DEFAULT.get().unwrap())
}

pub fn generate_container_name<R: Rng>(rng: &mut R, table: &Option<crate::campaign::schema::ContainerPartsTable>, bias: f64) -> (state::ContainerParts, String) {
    let t = get_container_table(table);
    let lock_status = select_weighted_part(rng, &t.lock_statuses, bias);
    let condition = select_weighted_part(rng, &t.conditions, bias);
    let accent_material = select_weighted_part(rng, &t.accent_materials, bias);
    let core_material = select_weighted_part(rng, &t.core_materials, bias);
    let container_type = select_weighted_part(rng, &t.container_types, bias);

    let name = format!("{}, {}, {}, {} {}", lock_status, condition, accent_material, core_material, container_type);

    let parts = state::ContainerParts {
        lock_status: lock_status.to_string(),
        condition: condition.to_string(),
        accent_material: accent_material.to_string(),
        core_material: core_material.to_string(),
        container_type: container_type.to_string(),
    };

    (parts, name)
}

pub fn generate_enemy_instance(campaign: &CampaignData, enemy_id: &str, scale: f32) -> Option<Enemy> {
    let base = campaign.enemies.base_enemies.get(enemy_id)?;
    let instance_id = format!("enemy_{}_{}", enemy_id, &uuid::Uuid::new_v4().to_string()[..4]);
    let scaled_hp = ((base.base_hp as f32) * scale) as i32;
    
    let equipped_armour = match &base.armour_config {
        Some(config) => generate_enemy_armour(campaign, config),
        None => vec![],
    };

    Some(Enemy {
        id: instance_id,
        template_id: enemy_id.to_string(),
        name: base.name.clone(),
        hp: scaled_hp,
        max_hp: scaled_hp,
        ac: ((base.base_ac as f32) * scale) as i32,
        strength: base.strength,
        dexterity: base.dexterity,
        constitution: base.constitution,
        intelligence: base.intelligence,
        wisdom: base.wisdom,
        charisma: base.charisma,
        damage_dice: base.base_damage_dice.clone(),
        attack_bonus: base.base_attack_bonus,
        xp: base.base_xp,
        studied: false,
        equipped_armour,
        perks: base.perks.clone(),
        loot_table: base.loot_table.clone(),
        damage_profile: base.damage_profile.clone(),
        range: base.range.clone(),
        damage_type: base.damage_type,
        known_spell_ids: base.known_spell_ids.clone(),
        mana: base.max_mana,
        max_mana: base.max_mana,
        speed: base.speed,
        x: 1,
        y: 1,
        awareness: state::AwarenessState::Unaware,
        behaviour: state::NpcBehaviour::Idle,
        detection_range: 5,
    })
}