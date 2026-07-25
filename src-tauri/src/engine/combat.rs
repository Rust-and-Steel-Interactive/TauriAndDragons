use rand::Rng;
use crate::engine::state::{DamageType, DamageProfile, WeaponRange};

/// Everything resolve_attack_roll needs to decide if an attack is even legal,
/// and if so, whether it hits — deliberately decoupled from `Player`/`Enemy`/
/// `SessionState` so this module stays free of engine-state dependencies,
/// matching the existing style of `apply_resistance`/`classify_range`.
#[derive(Debug, Clone)]
pub struct AttackContext {
    pub distance: i32,
    pub weapon_range: WeaponRange,
    pub has_line_of_sight: bool,
    /// True for RANGED/MAGIC weapons — gates both the long-range penalty's
    /// sibling adjacency penalty and (for the ammo path specifically) is left
    /// to the caller to check separately, since ammo requires inventory access
    /// this module intentionally doesn't have.
    pub is_ranged_style: bool,
    pub hostile_adjacent_to_attacker: bool,
    pub attack_stat_mod: i32,
    pub proficiency_bonus: i32,
    pub target_ac: i32,
    pub target_dodging: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackOutcome {
    OutOfRange,
    NoLineOfSight,
    Hit { atk_bonus: i32, atk_roll: i32, atk_total: i32, is_crit: bool },
    Miss { atk_bonus: i32, atk_roll: i32, atk_total: i32 },
}

/// Roll a d20 attack against `ctx`, applying range/LOS gating and both the
/// long-range and ranged-while-adjacent penalties before comparing to AC.
/// Ammo is NOT checked here — callers must gate on ammo availability
/// themselves before calling this, exactly as Step 21 already does, since
/// this module has no access to inventory state.
pub fn resolve_attack_roll(ctx: &AttackContext) -> AttackOutcome {
    let range_band = classify_range(ctx.distance, &ctx.weapon_range);
    if range_band == RangeBand::OutOfRange {
        return AttackOutcome::OutOfRange;
    }
    if ctx.is_ranged_style && !ctx.has_line_of_sight {
        return AttackOutcome::NoLineOfSight;
    }

    let mut rng = rand::thread_rng();
    let atk_roll = rng.gen_range(1..=20);
    let is_crit = atk_roll == 20;

    let mut atk_bonus = ctx.attack_stat_mod + ctx.proficiency_bonus;
    if range_band == RangeBand::LongRange {
        atk_bonus -= 5;
    }
    if ctx.is_ranged_style && ctx.hostile_adjacent_to_attacker {
        atk_bonus -= 5;
    }

    let effective_ac = if ctx.target_dodging { ctx.target_ac + 5 } else { ctx.target_ac };
    let atk_total = atk_roll + atk_bonus;

    if is_crit || atk_total >= effective_ac {
        AttackOutcome::Hit { atk_bonus, atk_roll, atk_total, is_crit }
    } else {
        AttackOutcome::Miss { atk_bonus, atk_roll, atk_total }
    }
}

/// Roll weapon damage dice + stat/flat bonuses, doubling dice on a crit.
/// Resistance is NOT applied here — callers should pass the result to
/// `apply_damage` (which calls `apply_resistance` internally), matching
/// Step 10's single-source-of-truth design.
pub fn resolve_damage_roll(dice_total: i32, stat_bonus: i32, flat_bonus: i32, is_crit: bool) -> i32 {
    let base = if is_crit { dice_total * 2 } else { dice_total };
    base + stat_bonus + flat_bonus
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
}
