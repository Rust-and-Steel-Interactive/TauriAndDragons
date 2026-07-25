use crate::campaign::schema::*;
use std::path::Path;

pub fn load_campaign(campaign_dir: &Path) -> anyhow::Result<CampaignData> {
    let main_path = campaign_dir.join("main.json");
    let items_path = campaign_dir.join("items.json");
    let modifiers_path = campaign_dir.join("modifiers.json");
    let lore_path = campaign_dir.join("lore_template.json");
    let enemies_path = campaign_dir.join("enemies.json");

    let main_str = std::fs::read_to_string(&main_path)?;
    let main: CampaignMain = serde_json::from_str(&main_str)?;

    let items_str = std::fs::read_to_string(&items_path)?;
    let items: ItemRegistry = serde_json::from_str(&items_str)?;

    let modifiers_str = std::fs::read_to_string(&modifiers_path)?;
    let modifiers: ModifierConfig = serde_json::from_str(&modifiers_str)?;

    let lore_str = std::fs::read_to_string(&lore_path)?;
    let lore: LoreTemplate = serde_json::from_str(&lore_str)?;

    let enemies_str = std::fs::read_to_string(&enemies_path)?;
    let enemies: EnemyRegistry = serde_json::from_str(&enemies_str)?;

    let map_path = campaign_dir.join("maps").join(format!("{}.json", main.starting_map));
    let map_str = std::fs::read_to_string(&map_path)?;
    let map: GameMap = serde_json::from_str(&map_str)?;

    let master_seed = main.campaign_seed;
    let (world_seed, region_seed, dungeon_seed, _) = crate::campaign::schema::derive_seed_hierarchy(master_seed);

    Ok(CampaignData {
        main,
        items,
        modifiers,
        lore,
        enemies,
        map,
        campaign_seed: master_seed,
        world_seed,
        region_seed,
        dungeon_seed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_d3_debug_campaign() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign_dir = manifest_dir.join("campaigns").join("d3_debug_campaign");
        let data = load_campaign(&campaign_dir).expect("Failed to load d3_debug_campaign");

        assert_eq!(data.main.campaign_id, "d3_debug_campaign");

        // Items with weapon_range
        let sword = data.items.base_items.get("shortsword").unwrap();
        assert_eq!(sword.weapon_range.as_ref().unwrap().normal, 1);
        assert_eq!(sword.weapon_range.as_ref().unwrap().long, None);

        let bow = data.items.base_items.get("longbow").unwrap();
        assert_eq!(bow.weapon_range.as_ref().unwrap().normal, 16);
        assert_eq!(bow.weapon_range.as_ref().unwrap().long, Some(64));

        // Items with damage_type
        assert_eq!(sword.damage_type, Some(crate::engine::state::DamageType::Slashing));
        assert_eq!(bow.damage_type, Some(crate::engine::state::DamageType::Piercing));

        // Item without weapon_range (backward compat)
        let potion = data.items.base_items.get("health_potion").unwrap();
        assert!(potion.weapon_range.is_none());

        // Belt items present
        assert!(data.items.base_items.contains_key("leather_belt"));
        let belt_item = data.items.base_items.get("leather_belt").unwrap();
        assert_eq!(belt_item.item_class, "BELT");

        // Enemies with damage_profile
        let zombie = data.enemies.base_enemies.get("zombie").unwrap();
        assert_eq!(zombie.damage_profile.resistances.len(), 2);
        assert!(zombie.damage_profile.resistances.contains(&crate::engine::state::DamageType::Necrotic));

        let elemental = data.enemies.base_enemies.get("fire_elemental").unwrap();
        assert!(elemental.damage_profile.immunities.contains(&crate::engine::state::DamageType::Fire));
        assert!(elemental.damage_profile.vulnerabilities.contains(&crate::engine::state::DamageType::Cold));

        // Enemy without damage_profile (backward compat)
        let rat = data.enemies.base_enemies.get("rat").unwrap();
        assert!(rat.damage_profile.resistances.is_empty());

        // Traps in map
        let map = &data.map;
        let typo_room = map.rooms.iter().find(|r| r.id == "typo_trap_test").unwrap();
        let trap_results = typo_room.procedural_slots.get("trap_fire").unwrap();
        let spawns = trap_results.results.get("1").unwrap();
        let trap_spawn = spawns.iter().find(|s| matches!(s, SpawnTemplate::Trap { .. })).unwrap();
        match trap_spawn {
            SpawnTemplate::Trap { damage_type, .. } => {
                // The JSON has "posion" — from_loose_str maps it to Bludgeoning
                assert_eq!(*damage_type, crate::engine::state::DamageType::Bludgeoning);
            }
            _ => panic!("Expected Trap spawn"),
        }
    }
}