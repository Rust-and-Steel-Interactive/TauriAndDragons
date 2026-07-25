use crate::engine::state::{DamageType, DamageProfile, WeaponRange};

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
