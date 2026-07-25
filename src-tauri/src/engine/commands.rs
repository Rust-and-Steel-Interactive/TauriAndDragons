use crate::engine::state::DamageType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Command {
    Damage {
        target: String,
        amount: i32,
        #[serde(default, deserialize_with = "deserialize_damage_type_loose")]
        damage_type: DamageType,
    },
    Heal { target: String, amount: i32 },
    AlterStat { target: String, key: String, operation: String, value: i32 },
    MoveEntity { target: String, to_location: String },
    AddItem { container: String, item_id: String, quantity: i32 },
    RemoveItem { container: String, item_id: String, quantity: i32 },
    GrantCondition { target: String, condition: String, duration: i32 },
    RemoveCondition { target: String, condition: String },
    RollCheck { stat: String, dc: Option<i32>, advantage: String },
    Narrate { text: String },
    AudioCue { cue: String },
    VisualEffect { effect: String },
    SetFlag { key: String, value: String },
    UseItem { target: String, item_id: String },
    Flee { target: String },
    Attack { target: String, weapon_id: Option<String> },
    EquipItem { target: String, item_id: String },
    DmChoose { action_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub narration: String,
    #[serde(default)]
    pub commands: Vec<serde_json::Value>,
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
    fn test_parse_damage_canonical_lowercase() {
        let json = r#"{"type":"DAMAGE","target":"player","amount":5,"damage_type":"poison"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::Damage { target, amount, damage_type } => {
                assert_eq!(target, "player");
                assert_eq!(amount, 5);
                assert_eq!(damage_type, DamageType::Poison);
            }
            _ => panic!("Expected Damage variant"),
        }
    }

    #[test]
    fn test_parse_damage_typo_fallback() {
        let json = r#"{"type":"DAMAGE","target":"player","amount":5,"damage_type":"posion"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::Damage { damage_type, .. } => {
                assert_eq!(damage_type, DamageType::Bludgeoning);
            }
            _ => panic!("Expected Damage variant"),
        }
    }

    #[test]
    fn test_parse_damage_missing_field_defaults() {
        let json = r#"{"type":"DAMAGE","target":"player","amount":5}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::Damage { damage_type, .. } => {
                assert_eq!(damage_type, DamageType::Bludgeoning);
            }
            _ => panic!("Expected Damage variant"),
        }
    }
}