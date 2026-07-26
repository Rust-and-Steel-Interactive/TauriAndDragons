use rand::Rng;
use crate::engine::state::{DamageType, DamageProfile, WeaponRange, Tile, has_line_of_sight, chebyshev_distance, tiles_between, Player};
use crate::campaign::schema::{BaseSpell, AoEShape};

/// Why an attack could not proceed to the dice roll at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttackBlockReason {
    OutOfRange,
    NoLineOfSight,
    OutOfAmmo(String),
}

/// The result of a fully-resolved attack roll (range/LOS/ammo all passed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackRollResult {
    pub roll: i32,
    pub bonus: i32,
    pub total: i32,
    pub is_crit: bool,
}

/// Everything needed to attempt an attack roll, gathered up-front by the caller
/// so this function stays pure (no access to SessionState/Player/Enemy directly).
pub struct AttackRollInputs<'a> {
    pub attacker_pos: (i32, i32),
    pub target_pos: (i32, i32),
    pub tiles: &'a [Vec<Tile>],
    pub weapon_range: WeaponRange,
    pub base_atk_bonus: i32,
    pub apply_adjacent_enemy_penalty: bool,
    pub enemy_adjacent_to_attacker: bool,
    pub required_ammo_type: Option<String>,
    pub has_required_ammo: bool,
}

pub fn resolve_attack_roll(inputs: AttackRollInputs) -> Result<AttackRollResult, AttackBlockReason> {
    let dist = chebyshev_distance(inputs.attacker_pos.0, inputs.attacker_pos.1, inputs.target_pos.0, inputs.target_pos.1);
    let range_band = classify_range(dist, &inputs.weapon_range);

    if range_band == RangeBand::OutOfRange {
        return Err(AttackBlockReason::OutOfRange);
    }

    if let Some(ref ammo_type) = inputs.required_ammo_type {
        if !inputs.has_required_ammo {
            return Err(AttackBlockReason::OutOfAmmo(ammo_type.clone()));
        }
    }

    if !has_line_of_sight(inputs.tiles, inputs.attacker_pos.0, inputs.attacker_pos.1, inputs.target_pos.0, inputs.target_pos.1) {
        return Err(AttackBlockReason::NoLineOfSight);
    }

    let mut rng = rand::thread_rng();
    let roll = rng.gen_range(1..=20);
    let is_crit = roll == 20;

    let mut bonus = inputs.base_atk_bonus;
    if range_band == RangeBand::LongRange {
        bonus -= 5;
    }
    if inputs.apply_adjacent_enemy_penalty && inputs.enemy_adjacent_to_attacker {
        bonus -= 5;
    }

    Ok(AttackRollResult { roll, bonus, total: roll + bonus, is_crit })
}

/// Rolls weapon/spell damage dice and doubles on crit. Deliberately does NOT
/// apply resistance — `SessionState::apply_damage` (state.rs) is the single
/// place resistance gets applied, since it's the only call site that actually
/// knows which target is being hit and can look up the right DamageProfile.
/// An earlier version of this function applied resistance itself AND relied
/// on callers piping the result through apply_damage, which double-applied
/// resistance on every hit — fixed here.
pub fn resolve_damage_roll(dmg_dice: &str, dmg_bonus: i32, is_crit: bool) -> i32 {
    let (dice_roll, embedded_bonus) = crate::engine::validator::roll_dice_expr(dmg_dice);
    let dice_roll = if is_crit { dice_roll * 2 } else { dice_roll };
    dice_roll + embedded_bonus + dmg_bonus
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum RangeBand {
    InRange,
    LongRange,
    OutOfRange,
}

/// Classify a tile distance against a weapon's range profile.
///
/// - `dist <= range.normal` → InRange (no penalty).
/// - `range.normal < dist <= range.long` (if the weapon has a long range) → LongRange
///   (later steps apply a penalty for this band).
/// - Anything beyond that (or beyond `normal` for a weapon with no `long` at all,
///   e.g. melee) → OutOfRange (attack is blocked).
#[allow(dead_code)]
pub fn classify_range(dist: i32, range: &WeaponRange) -> RangeBand {
    if dist <= range.normal {
        RangeBand::InRange
    } else if let Some(long) = range.long {
        if dist <= long {
            RangeBand::LongRange
        } else {
            RangeBand::OutOfRange
        }
    } else {
        RangeBand::OutOfRange
    }
}

/// Apply a target's resistance/vulnerability/immunity profile to an incoming
/// damage amount for a specific damage type.
///
/// Rules (standard 5e-style stacking):
/// - Immune: damage becomes 0, regardless of anything else.
/// - Resistant AND Vulnerable to the same type: they cancel out, full damage.
/// - Resistant only: half damage, rounded down.
/// - Vulnerable only: double damage.
/// - Neither: unchanged.
/// Why a spell could not be cast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastBlockReason {
    NoResonance,
    SpellNotKnown,
    InsufficientMana,
    WrongWeapon,
}

/// The single gate through which all spellcasting eligibility flows.
/// Does NOT deduct mana, resolve damage/healing, or advance any turn state.
pub fn can_cast_spell(player: &Player, spell_id: &str, spell: &BaseSpell) -> Result<(), CastBlockReason> {
    if !player.has_resonance {
        return Err(CastBlockReason::NoResonance);
    }
    if !player.known_spell_ids.contains(spell_id) {
        return Err(CastBlockReason::SpellNotKnown);
    }
    if player.mana < spell.mana_cost {
        return Err(CastBlockReason::InsufficientMana);
    }
    if !spell.allowed_weapons.is_empty() {
        let equipped_template_id = player.primary_hand.as_deref()
            .and_then(|id| player.inventory.iter().find(|i| i.instance_id == id))
            .map(|i| i.template_id.as_str());
        let weapon_allowed = equipped_template_id
            .map(|tid| spell.allowed_weapons.iter().any(|w| w == tid))
            .unwrap_or(false);
        if !weapon_allowed {
            return Err(CastBlockReason::WrongWeapon);
        }
    }
    Ok(())
}

/// Turn a spell's AoEShape into the concrete set of tiles it covers.
pub fn rasterize_aoe(shape: &AoEShape, origin: (i32, i32), target: (i32, i32)) -> Vec<(i32, i32)> {
    match shape {
        AoEShape::Circle { radius } => {
            let mut tiles = Vec::new();
            for dx in -*radius..=*radius {
                for dy in -*radius..=*radius {
                    let (tx, ty) = (target.0 + dx, target.1 + dy);
                    if chebyshev_distance(target.0, target.1, tx, ty) <= *radius {
                        tiles.push((tx, ty));
                    }
                }
            }
            tiles
        }
        AoEShape::Line { length } => {
            let dx = (target.0 - origin.0) as f32;
            let dy = (target.1 - origin.1) as f32;
            let mag = (dx * dx + dy * dy).sqrt().max(0.001);
            let (fx, fy) = (dx / mag, dy / mag);
            let end_x = origin.0 + (fx * *length as f32).round() as i32;
            let end_y = origin.1 + (fy * *length as f32).round() as i32;
            let mut tiles = tiles_between(origin.0, origin.1, end_x, end_y);
            tiles.push((end_x, end_y));
            tiles
        }
    }
}

pub fn apply_resistance(dmg: i32, dtype: DamageType, profile: &DamageProfile) -> i32 {
    if profile.immunities.contains(&dtype) {
        return 0;
    }

    let resistant = profile.resistances.contains(&dtype);
    let vulnerable = profile.vulnerabilities.contains(&dtype);

    match (resistant, vulnerable) {
        (true, true) => dmg,
        (true, false) => (dmg as f32 / 2.0).floor() as i32,
        (false, true) => dmg * 2,
        (false, false) => dmg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::state::ItemInstance;
    use std::collections::HashSet;

    fn profile_with(resistances: &[DamageType], vulnerabilities: &[DamageType], immunities: &[DamageType]) -> DamageProfile {
        DamageProfile {
            resistances: resistances.to_vec(),
            vulnerabilities: vulnerabilities.to_vec(),
            immunities: immunities.to_vec(),
        }
    }

    #[test]
    fn test_immune_zeroes_damage() {
        let profile = profile_with(&[], &[], &[DamageType::Fire]);
        assert_eq!(apply_resistance(20, DamageType::Fire, &profile), 0);
    }

    #[test]
    fn test_resistant_halves_and_floors() {
        let profile = profile_with(&[DamageType::Bludgeoning], &[], &[]);
        assert_eq!(apply_resistance(7, DamageType::Bludgeoning, &profile), 3);
    }

    #[test]
    fn test_vulnerable_doubles() {
        let profile = profile_with(&[], &[DamageType::Radiant], &[]);
        assert_eq!(apply_resistance(6, DamageType::Radiant, &profile), 12);
    }

    #[test]
    fn test_resistant_and_vulnerable_cancel_out() {
        let profile = profile_with(&[DamageType::Cold], &[DamageType::Cold], &[]);
        assert_eq!(apply_resistance(10, DamageType::Cold, &profile), 10);
    }

    #[test]
    fn test_neither_unchanged() {
        let profile = DamageProfile::default();
        assert_eq!(apply_resistance(9, DamageType::Poison, &profile), 9);
    }

    #[test]
    fn test_classify_range_in_range() {
        let range = WeaponRange { normal: 6, long: Some(20) };
        assert_eq!(classify_range(4, &range), RangeBand::InRange);
        assert_eq!(classify_range(6, &range), RangeBand::InRange);
    }

    #[test]
    fn test_classify_range_long_range() {
        let range = WeaponRange { normal: 6, long: Some(20) };
        assert_eq!(classify_range(10, &range), RangeBand::LongRange);
        assert_eq!(classify_range(20, &range), RangeBand::LongRange);
    }

    #[test]
    fn test_classify_range_out_of_range_beyond_long() {
        let range = WeaponRange { normal: 6, long: Some(20) };
        assert_eq!(classify_range(21, &range), RangeBand::OutOfRange);
    }

    #[test]
    fn test_classify_range_melee_no_long_range() {
        let range = WeaponRange { normal: 1, long: None };
        assert_eq!(classify_range(1, &range), RangeBand::InRange);
        assert_eq!(classify_range(2, &range), RangeBand::OutOfRange);
    }

    // ── Scenario tests for resolve_attack_roll / resolve_damage_roll ──

    fn floor_tiles(width: i32, height: i32) -> Vec<Vec<Tile>> {
        (0..height).map(|y| {
            (0..width).map(|x| Tile {
                x, y,
                tile_type: crate::engine::state::TileType::Floor,
                visibility: crate::engine::state::TileVisibility::Visible,
                ground_light_source: None,
            }).collect()
        }).collect()
    }

    #[test]
    fn test_scenario_in_range_hit() {
        let tiles = floor_tiles(5, 5);
        let inputs = AttackRollInputs {
            attacker_pos: (0, 0),
            target_pos: (1, 0),
            tiles: &tiles,
            weapon_range: WeaponRange { normal: 1, long: None },
            base_atk_bonus: 5,
            apply_adjacent_enemy_penalty: false,
            enemy_adjacent_to_attacker: false,
            required_ammo_type: None,
            has_required_ammo: true,
        };
        let result = resolve_attack_roll(inputs).expect("should resolve, target is in range");
        assert_eq!(result.bonus, 5, "no penalties should apply at normal range");
    }

    #[test]
    fn test_scenario_long_range_penalty() {
        let tiles = floor_tiles(20, 20);
        let inputs = AttackRollInputs {
            attacker_pos: (0, 0),
            target_pos: (10, 0),
            tiles: &tiles,
            weapon_range: WeaponRange { normal: 6, long: Some(20) },
            base_atk_bonus: 5,
            apply_adjacent_enemy_penalty: false,
            enemy_adjacent_to_attacker: false,
            required_ammo_type: None,
            has_required_ammo: true,
        };
        let result = resolve_attack_roll(inputs).expect("distance 10 is within long range (6..20)");
        assert_eq!(result.bonus, 0, "long-range penalty of -5 should reduce bonus from 5 to 0");
    }

    #[test]
    fn test_scenario_out_of_range_block() {
        let tiles = floor_tiles(20, 20);
        let inputs = AttackRollInputs {
            attacker_pos: (0, 0),
            target_pos: (10, 0),
            tiles: &tiles,
            weapon_range: WeaponRange { normal: 1, long: None },
            base_atk_bonus: 5,
            apply_adjacent_enemy_penalty: false,
            enemy_adjacent_to_attacker: false,
            required_ammo_type: None,
            has_required_ammo: true,
        };
        let result = resolve_attack_roll(inputs);
        assert_eq!(result, Err(AttackBlockReason::OutOfRange));
    }

    #[test]
    fn test_scenario_no_ammo_block() {
        let tiles = floor_tiles(5, 5);
        let inputs = AttackRollInputs {
            attacker_pos: (0, 0),
            target_pos: (1, 0),
            tiles: &tiles,
            weapon_range: WeaponRange { normal: 16, long: Some(64) },
            base_atk_bonus: 5,
            apply_adjacent_enemy_penalty: false,
            enemy_adjacent_to_attacker: false,
            required_ammo_type: Some("arrow".to_string()),
            has_required_ammo: false,
        };
        let result = resolve_attack_roll(inputs);
        assert_eq!(result, Err(AttackBlockReason::OutOfAmmo("arrow".to_string())));
    }

    #[test]
    fn test_scenario_adjacent_hostile_penalty() {
        let tiles = floor_tiles(5, 5);
        let inputs = AttackRollInputs {
            attacker_pos: (0, 0),
            target_pos: (3, 0),
            tiles: &tiles,
            weapon_range: WeaponRange { normal: 16, long: Some(64) },
            base_atk_bonus: 5,
            apply_adjacent_enemy_penalty: true,
            enemy_adjacent_to_attacker: true,
            required_ammo_type: None,
            has_required_ammo: true,
        };
        let result = resolve_attack_roll(inputs).expect("in range, should resolve");
        assert_eq!(result.bonus, 0, "adjacent-hostile penalty of -5 should reduce bonus from 5 to 0, independent of range band");
    }

    #[test]
    fn test_resolve_damage_roll_crit_doubles_dice_only() {
        let normal = resolve_damage_roll("1d1", 5, false);
        let crit = resolve_damage_roll("1d1", 5, true);
        assert_eq!(normal, 6);
        assert_eq!(crit, 7);
    }

    fn test_spell(mana_cost: i32, allowed_weapons: &[&str]) -> BaseSpell {
        BaseSpell {
            name: "Test Bolt".to_string(),
            school: "Evocation".to_string(),
            tier: 1,
            mana_cost,
            damage_dice: Some("2d6".to_string()),
            damage_type: Some(DamageType::Fire),
            heal_dice: None,
            range: None,
            area_of_effect: None,
            allowed_weapons: allowed_weapons.iter().map(|s| s.to_string()).collect(),
            status_effects: vec![],
            ai_description: None,
        }
    }

    #[test]
    fn test_can_cast_spell_blocks_no_resonance() {
        let player = Player { has_resonance: false, mana: 100, known_spell_ids: HashSet::from(["bolt".to_string()]), ..Player::default_for_test() };
        let spell = test_spell(5, &[]);
        assert_eq!(can_cast_spell(&player, "bolt", &spell), Err(CastBlockReason::NoResonance));
    }

    #[test]
    fn test_can_cast_spell_blocks_unknown_spell() {
        let player = Player { has_resonance: true, mana: 100, known_spell_ids: HashSet::new(), ..Player::default_for_test() };
        let spell = test_spell(5, &[]);
        assert_eq!(can_cast_spell(&player, "bolt", &spell), Err(CastBlockReason::SpellNotKnown));
    }

    #[test]
    fn test_can_cast_spell_blocks_insufficient_mana() {
        let player = Player { has_resonance: true, mana: 2, known_spell_ids: HashSet::from(["bolt".to_string()]), ..Player::default_for_test() };
        let spell = test_spell(5, &[]);
        assert_eq!(can_cast_spell(&player, "bolt", &spell), Err(CastBlockReason::InsufficientMana));
    }

    #[test]
    fn test_can_cast_spell_blocks_wrong_weapon() {
        let sword = ItemInstance { instance_id: "w1".to_string(), template_id: "shortsword".to_string(), item_class: "MELEE".to_string(), ..ItemInstance::default_for_test() };
        let player = Player {
            has_resonance: true, mana: 100,
            known_spell_ids: HashSet::from(["bolt".to_string()]),
            primary_hand: Some("w1".to_string()),
            inventory: vec![sword],
            ..Player::default_for_test()
        };
        let spell = test_spell(5, &["staff", "wand"]);
        assert_eq!(can_cast_spell(&player, "bolt", &spell), Err(CastBlockReason::WrongWeapon));
    }

    #[test]
    fn test_can_cast_spell_succeeds_with_matching_weapon() {
        let staff = ItemInstance { instance_id: "w1".to_string(), template_id: "staff".to_string(), item_class: "MAGIC".to_string(), ..ItemInstance::default_for_test() };
        let player = Player {
            has_resonance: true, mana: 100,
            known_spell_ids: HashSet::from(["bolt".to_string()]),
            primary_hand: Some("w1".to_string()),
            inventory: vec![staff],
            ..Player::default_for_test()
        };
        let spell = test_spell(5, &["staff", "wand"]);
        assert_eq!(can_cast_spell(&player, "bolt", &spell), Ok(()));
    }

    #[test]
    fn test_can_cast_spell_succeeds_when_no_weapon_restriction() {
        let player = Player { has_resonance: true, mana: 100, known_spell_ids: HashSet::from(["bolt".to_string()]), ..Player::default_for_test() };
        let spell = test_spell(5, &[]);
        assert_eq!(can_cast_spell(&player, "bolt", &spell), Ok(()));
    }

    #[test]
    fn test_rasterize_aoe_circle_includes_center_and_radius_edge() {
        let shape = AoEShape::Circle { radius: 1 };
        let tiles = rasterize_aoe(&shape, (0, 0), (5, 5));
        assert!(tiles.contains(&(5, 5)));
        assert!(tiles.contains(&(6, 5)));
        assert!(!tiles.contains(&(7, 5)));
    }

    #[test]
    fn test_rasterize_aoe_line_extends_from_origin_toward_target() {
        let shape = AoEShape::Line { length: 3 };
        let tiles = rasterize_aoe(&shape, (0, 0), (10, 0));
        assert!(tiles.contains(&(3, 0)));
        assert!(!tiles.contains(&(0, 0)));
    }
}
