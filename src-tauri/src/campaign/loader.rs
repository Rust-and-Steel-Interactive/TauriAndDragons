use crate::campaign::schema::*;
use std::collections::HashMap;
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

    let spells_path = campaign_dir.join("spells.json");
    let spells: SpellRegistry = match std::fs::read_to_string(&spells_path) {
        Ok(spells_str) => match serde_json::from_str(&spells_str) {
            Ok(parsed) => parsed,
            Err(e) => {
                eprintln!("WARN: campaign '{}' has an invalid spells.json ({}), continuing with no spells", campaign_dir.display(), e);
                SpellRegistry { base_spells: HashMap::new() }
            }
        },
        Err(_) => {
            eprintln!("WARN: campaign '{}' has no spells.json, continuing with no spells", campaign_dir.display());
            SpellRegistry { base_spells: HashMap::new() }
        }
    };

    let map_path = campaign_dir.join("maps").join(format!("{}.json", main.starting_map));
    let map_str = std::fs::read_to_string(&map_path)?;
    let map: GameMap = serde_json::from_str(&map_str)?;

    let master_seed = main.campaign_seed;
    let (world_seed, region_seed, dungeon_seed, _) = crate::campaign::schema::derive_seed_hierarchy(master_seed);

    validate_spell_weapon_references(&spells, &items, campaign_dir);
    validate_item_spell_references(&items, &spells, campaign_dir);

    Ok(CampaignData {
        main,
        items,
        modifiers,
        lore,
        enemies,
        map,
        spells,
        campaign_seed: master_seed,
        world_seed,
        region_seed,
        dungeon_seed,
    })
}

fn validate_item_spell_references(items: &ItemRegistry, spells: &SpellRegistry, campaign_dir: &Path) {
    for (item_id, item) in &items.base_items {
        match item.item_class.as_str() {
            "SPELLBOOK" => {
                if item.known_spell_ids.is_empty() {
                    eprintln!(
                        "WARN: campaign '{}' SPELLBOOK item '{}' has an empty known_spell_ids list — learning it would grant nothing",
                        campaign_dir.display(), item_id
                    );
                }
                for spell_id in &item.known_spell_ids {
                    if !spells.base_spells.contains_key(spell_id) {
                        eprintln!(
                            "WARN: campaign '{}' SPELLBOOK item '{}' references unknown spell '{}' (not found in spells.json)",
                            campaign_dir.display(), item_id, spell_id
                        );
                    }
                }
            }
            "SPELL_SCROLL" => match &item.scroll_spell_id {
                None => eprintln!(
                    "WARN: campaign '{}' SPELL_SCROLL item '{}' has no scroll_spell_id set — reading it would teach nothing",
                    campaign_dir.display(), item_id
                ),
                Some(spell_id) if !spells.base_spells.contains_key(spell_id) => eprintln!(
                    "WARN: campaign '{}' SPELL_SCROLL item '{}' references unknown spell '{}' (not found in spells.json)",
                    campaign_dir.display(), item_id, spell_id
                ),
                Some(_) => {}
            },
            _ => {}
        }

        if let Some(innate_id) = &item.innate_spell_id {
            if !spells.base_spells.contains_key(innate_id) {
                eprintln!(
                    "WARN: campaign '{}' item '{}' has innate_spell_id '{}' that doesn't exist in spells.json",
                    campaign_dir.display(), item_id, innate_id
                );
            }
        }
    }
}

fn validate_spell_weapon_references(spells: &SpellRegistry, items: &ItemRegistry, campaign_dir: &Path) {
    for (spell_id, spell) in &spells.base_spells {
        for weapon_id in &spell.allowed_weapons {
            if !items.base_items.contains_key(weapon_id) {
                eprintln!(
                    "WARN: campaign '{}' spell '{}' references unknown allowed_weapon '{}' (not found in items.json)",
                    campaign_dir.display(), spell_id, weapon_id
                );
            }
        }
    }
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

    #[test]
    fn test_load_campaign_without_spells_json_gets_empty_registry() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // d3_debug_campaign has no spells.json — confirm this doesn't break loading.
        let campaign_dir = manifest_dir.join("campaigns").join("d3_debug_campaign");
        let data = load_campaign(&campaign_dir).expect("campaign without spells.json should still load");
        assert!(data.spells.base_spells.is_empty());
    }

    #[test]
    fn test_load_debug_campaign_spells_json_populates_registry() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign_dir = manifest_dir.join("campaigns").join("debug_campaign");
        let data = load_campaign(&campaign_dir).expect("debug_campaign should load");
        assert!(data.spells.base_spells.contains_key("fire_bolt"));
        assert!(data.spells.base_spells.contains_key("fireball"));
    }

    #[test]
    fn test_load_d4_magic_debug_campaign() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign_dir = manifest_dir.join("campaigns").join("d4_magic_debug");
        let data = load_campaign(&campaign_dir).expect("d4_magic_debug should load");

        assert_eq!(data.main.campaign_id, "d4_magic_debug");

        // Confirm SPELLBOOK item has all magic fields
        let spellbook = data.items.base_items.get("spellbook_of_fire").unwrap();
        assert_eq!(spellbook.item_class, "SPELLBOOK");
        assert_eq!(spellbook.discipline.as_deref(), Some("Fire"));
        assert_eq!(spellbook.tier, Some(2));
        assert!(spellbook.known_spell_ids.contains(&"fire_bolt".to_string()));

        // Confirm SPELL_SCROLL item
        let scroll = data.items.base_items.get("scroll_of_fireball").unwrap();
        assert_eq!(scroll.item_class, "SPELL_SCROLL");
        assert_eq!(scroll.scroll_spell_id.as_deref(), Some("fireball"));

        // Confirm spells exist
        assert!(data.spells.base_spells.contains_key("fire_bolt"));
        assert!(data.spells.base_spells.contains_key("heal_wounds"));
        assert!(data.spells.base_spells.contains_key("fireball"));

        // Confirm staff is a MAGIC item with allowed_weapon-compatible template_id
        let staff = data.items.base_items.get("staff").unwrap();
        assert_eq!(staff.item_class, "MAGIC");

        // Confirm enemy exists
        assert!(data.enemies.base_enemies.contains_key("test_dummy"));

        // Confirm map has 2 rooms
        assert_eq!(data.map.rooms.len(), 2);
    }

    #[test]
    fn test_validate_item_spell_references_does_not_panic_on_dangling_or_empty() {
        use std::collections::HashMap;
        use std::path::Path;

        let mut items = HashMap::new();
        items.insert("empty_spellbook".to_string(), BaseItem {
            item_class: "SPELLBOOK".to_string(),
            known_spell_ids: vec![],
            ..BaseItem::default_for_test()
        });
        items.insert("dangling_scroll".to_string(), BaseItem {
            item_class: "SPELL_SCROLL".to_string(),
            scroll_spell_id: Some("nonexistent_spell".to_string()),
            ..BaseItem::default_for_test()
        });
        items.insert("blank_scroll".to_string(), BaseItem {
            item_class: "SPELL_SCROLL".to_string(),
            scroll_spell_id: None,
            ..BaseItem::default_for_test()
        });
        items.insert("dangling_wand".to_string(), BaseItem {
            item_class: "MAGIC".to_string(),
            innate_spell_id: Some("nonexistent_spell".to_string()),
            ..BaseItem::default_for_test()
        });

        let item_registry = ItemRegistry { base_items: items };
        let spell_registry = SpellRegistry { base_spells: HashMap::new() };

        validate_item_spell_references(&item_registry, &spell_registry, Path::new("test_campaign"));
    }
}