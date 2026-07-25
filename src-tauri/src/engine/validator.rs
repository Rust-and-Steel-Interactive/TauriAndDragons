use crate::engine::commands::Command;
use crate::engine::state::{ability_modifier, compute_enemy_ac, DamageType, SessionState};
use rand::Rng;

pub enum CommandRejection {
    Critical(String),
    NonCritical(String),
}

pub fn roll_dice(expression: &str) -> i32 {
    let (dice_roll, bonus) = roll_dice_expr(expression);
    dice_roll + bonus
}

pub fn roll_dice_expr(expression: &str) -> (i32, i32) {
    let parts: Vec<&str> = expression.split('d').collect();
    if parts.len() != 2 { return (0, 0); }
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
    let dice_total = (1..=count).map(|_| rng.gen_range(1..=sides)).sum::<i32>();
    (dice_total, bonus)
}

pub fn validate_and_execute(cmd: &Command, state: &mut SessionState) -> Result<(), CommandRejection> {
    match cmd {
        Command::Damage { target, amount, damage_type } => {
            if target != "player" {
                return Err(CommandRejection::NonCritical(format!("Target {} not found.", target)));
            }
            state.apply_damage(target, *amount, *damage_type).map_err(CommandRejection::Critical)
        }

        Command::Heal { target, amount, .. } => {
            if target != "player" {
                return Err(CommandRejection::NonCritical(format!("Target {} not found.", target)));
            }
            state.apply_heal(target, *amount).map_err(CommandRejection::Critical)
        }

        Command::RollCheck { stat: _, dc, .. } => {
            let mut rng = rand::thread_rng();
            let roll = rng.gen_range(1..=20);
            let bonus = 2;
            let total = roll + bonus;
            
            let outcome = if let Some(dc_val) = dc {
                if total >= *dc_val { "SUCCESS" } else { "FAILURE" }
            } else {
                "INFORMATIONAL"
            };

            let dc_str = dc.map_or("None".to_string(), |d| d.to_string());
            state.last_roll = format!("d20+{} = {} ({}) vs DC {}", bonus, total, outcome, dc_str);
            Ok(())
        }

        Command::UseItem { target, item_id } => {
            if target != "player" {
                return Err(CommandRejection::NonCritical("Can only target player.".to_string()));
            }
            
            let item_index = state.player.inventory.iter().position(|i| i.instance_id == *item_id);
            if let Some(idx) = item_index {
                let template_id = state.player.inventory[idx].template_id.clone();
                let display_name = state.player.inventory[idx].display_name.clone();

                let item = &mut state.player.inventory[idx];
                if item.quantity > 0 {
                    let effect = item.effect.clone();
                    let display_name = item.display_name.clone();
                    let template_id = item.template_id.clone();
                    let light_radius = item.light_radius;
                    let duration_turns = item.duration_turns;
                    item.quantity -= 1;
                    
                    if template_id == "torch" {
                        let radius = light_radius.unwrap_or(3);
                        let remaining_turns = duration_turns.unwrap_or(60);
                        // If wielding a two-handed weapon, stow it first
                        if let Some(ph) = &state.player.primary_hand {
                            if state.player.inventory.iter()
                                .any(|i| i.instance_id == *ph && i.handedness.as_deref() == Some("TWO_HANDED"))
                            {
                                state.player.primary_hand = None;
                            }
                        }
                        // Torch always goes to secondary_hand when lit
                        state.player.secondary_hand = Some(item_id.clone());
                        state.player.active_light_source = Some(crate::engine::state::ActiveLightSource {
                            item_id: "torch".to_string(),
                            radius,
                            remaining_turns,
                            is_belt_mounted: false,
                        });
                        state.last_combat_event = format!("You light the {}. (Radius {}, {} turns)", display_name, radius, remaining_turns);
                    } else if let Some(eff) = effect {
                        if eff.starts_with("HEAL") {
                            let dice_str = eff.trim_start_matches("HEAL_");
                            let heal_amount = roll_dice(dice_str);
                            
                            state.player.hp += heal_amount;
                            if state.player.hp > state.player.max_hp {
                                state.player.hp = state.player.max_hp;
                            }
                            
                            state.last_roll = format!("{} = {} (Healing)", dice_str, heal_amount);
                            state.last_combat_event = format!("Player used {} and healed {} HP.", display_name, heal_amount);
                        }
                    }
                    Ok(())
                } else {
                    Err(CommandRejection::NonCritical("No items left!".to_string()))
                }
            } else {
                Err(CommandRejection::NonCritical("Item not found.".to_string()))
            }
        }

        Command::EquipItem { target, item_id } => {
            if target != "player" {
                return Err(CommandRejection::NonCritical("Can only target player.".to_string()));
            }
            
            if state.player.inventory.iter().any(|i| {
                i.instance_id == *item_id && (
                    i.item_class == "WEAPON" || i.item_class == "MELEE" || i.item_class == "MAGIC" || i.item_class == "RANGED"
                    || i.handedness.is_some()
                )
            }) {
                if let Some(msg) = state.equip_to_slot(item_id) {
                    state.last_combat_event = msg;
                }
                state.generate_available_actions();
                Ok(())
            } else {
                Err(CommandRejection::NonCritical("Weapon not found in inventory.".to_string()))
            }
        }

        Command::Attack { target, .. } => {
            // ── Range check (all weapon classes) ──
            let target_dist = state.get_current_room()
                .and_then(|r| r.enemies.iter().find(|e| e.id == *target))
                .map(|e| crate::engine::state::chebyshev_distance(e.x, e.y, state.player.x, state.player.y));

            let target_dist = match target_dist {
                Some(d) => d,
                None => return Err(CommandRejection::NonCritical("Target enemy not found.".to_string())),
            };

            let weapon_range = crate::engine::state::get_weapon_range(&state.player);
            let range_band = crate::engine::combat::classify_range(target_dist, &weapon_range);

            if range_band == crate::engine::combat::RangeBand::OutOfRange {
                return Err(CommandRejection::NonCritical("Target is out of range for your weapon.".to_string()));
            }

            // ── "Ranged while adjacent" penalty setup ──
            let weapon_item_class = state.get_equipped_weapon().map(|w| w.item_class.clone());
            let is_ranged_style = matches!(weapon_item_class.as_deref(), Some("RANGED") | Some("MAGIC"));
            let enemy_adjacent_to_player = state.get_current_room()
                .map(|r| r.enemies.iter().any(|e| e.hp > 0 && crate::engine::state::is_adjacent(e.x, e.y, state.player.x, state.player.y)))
                .unwrap_or(false);

            // ── LOS check for ranged/magic attacks only ──
            if is_ranged_style {
                let target_pos = state.get_current_room()
                    .and_then(|r| r.enemies.iter().find(|e| e.id == *target))
                    .map(|e| (e.x, e.y));
                let has_los = match target_pos {
                    Some((tx, ty)) => state.get_current_room()
                        .map(|r| crate::engine::state::has_line_of_sight(&r.tiles, state.player.x, state.player.y, tx, ty))
                        .unwrap_or(false),
                    None => false,
                };
                if !has_los {
                    return Err(CommandRejection::NonCritical("You don't have a clear line of sight to that target.".to_string()));
                }
            }

            // ── Ammo check (RANGED weapons that require ammo only; MAGIC is exempt) ──
            if weapon_item_class.as_deref() == Some("RANGED") {
                if let Some(required_ammo) = state.get_equipped_weapon().and_then(|w| w.ammo_type.clone()) {
                    if !crate::engine::state::has_matching_ammo(&state.player, &required_ammo) {
                        return Err(CommandRejection::NonCritical(format!("You're out of {} for your weapon.", required_ammo)));
                    }
                }
            }

            let mut rng = rand::thread_rng();
            let atk_roll = rng.gen_range(1..=20);

            let (weapon_name, dmg_dice, dmg_bonus, atk_stat_mod, weapon_damage_type) = {
                if let Some(w) = state.get_equipped_weapon() {
                    let stat_mod = match w.item_class.as_str() {
                        "RANGED" => ability_modifier(state.player.dexterity),
                        "MAGIC" => ability_modifier(state.player.intelligence),
                        _ => ability_modifier(state.player.strength),
                    };
                    (w.display_name.clone(), w.damage_dice.clone().unwrap_or("1d3".to_string()), w.damage_bonus.unwrap_or(0), stat_mod, w.damage_type.unwrap_or_default())
                } else {
                    let stat_mod = ability_modifier(state.player.strength);
                    ("Unarmed Strike".to_string(), "1d4".to_string(), 0, stat_mod, DamageType::default())
                }
            };
            let proficiency = state.player.proficiency_bonus;
            let mut atk_bonus = atk_stat_mod + proficiency;
            if range_band == crate::engine::combat::RangeBand::LongRange {
                atk_bonus -= 5;
            }
            if is_ranged_style && enemy_adjacent_to_player {
                atk_bonus -= 5;
            }
            let atk_total = atk_roll + atk_bonus;

            let (hit, enemy_name, enemy_ac, enemy_hp_before) = {
                if let Some(room) = state.get_current_room() {
                    if let Some(enemy) = room.enemies.iter().find(|e| e.id == *target) {
                        let e_ac = compute_enemy_ac(enemy);
                        (atk_total >= e_ac, enemy.name.clone(), e_ac, enemy.hp)
                    } else {
                        return Err(CommandRejection::NonCritical("Attack target not found.".to_string()));
                    }
                } else {
                    return Err(CommandRejection::Critical("No current room.".to_string()));
                }
            };

            let (dmg_roll, enemy_hp) = if hit {
                let raw_dice = roll_dice(&dmg_dice);
                let total_dmg = raw_dice + dmg_bonus;
                state.apply_damage(target, total_dmg, weapon_damage_type).map_err(CommandRejection::Critical)?;
                let new_hp = state.get_current_room().and_then(|r| r.enemies.iter().find(|e| e.id == *target)).map(|e| e.hp).unwrap_or(enemy_hp_before);
                (total_dmg, new_hp)
            } else {
                (0, enemy_hp_before)
            };

            if hit {
                state.last_roll = format!("d20+{} = {} (HIT) vs AC {}. Dmg: {}{} = {}", atk_bonus, atk_total, enemy_ac, dmg_dice, if dmg_bonus >= 0 { format!("+{}", dmg_bonus) } else { format!("{}", dmg_bonus) }, dmg_roll);
                state.last_combat_event = format!("Player attacked {} with {}. Attack roll d20+{} = {} vs AC {}. HIT for {} damage. {} HP is now {}.", enemy_name, weapon_name, atk_bonus, atk_total, enemy_ac, dmg_roll, enemy_name, enemy_hp);
                if enemy_hp <= 0 {
                    state.last_combat_event.push_str(&format!(" {} has died.", enemy_name));
                }
            } else {
                state.last_roll = format!("d20+{} = {} (MISS) vs AC {}", atk_bonus, atk_total, enemy_ac);
                state.last_combat_event = format!("Player attacked {} with {}. Attack roll d20+{} = {} vs AC {}. MISS.", enemy_name, weapon_name, atk_bonus, atk_total, enemy_ac);
            }

            if weapon_item_class.as_deref() == Some("RANGED") {
                if let Some(required_ammo) = state.get_equipped_weapon().and_then(|w| w.ammo_type.clone()) {
                    if crate::engine::state::consume_matching_ammo(&mut state.player, &required_ammo) {
                        state.ammo_consumed_this_combat.entry(required_ammo).and_modify(|c| *c += 1).or_insert(1);
                    }
                }
            }

            Ok(())
        }

        Command::AddItem { container, item_id, quantity } => {
            if container != "player" {
                return Err(CommandRejection::NonCritical("Can only add to player inventory.".to_string()));
            }
            
            if let Some(room) = state.get_current_room_mut() {
                if let Some(pos) = room.loot.iter().position(|i| i.instance_id == *item_id) {
                    let mut room_item = room.loot.remove(pos);
                    room_item.quantity = *quantity;
                    
                    if let Some(existing) = state.player.inventory.iter_mut().find(|i| i.instance_id == room_item.instance_id) {
                        existing.quantity += room_item.quantity;
                    } else {
                        state.player.inventory.push(room_item);
                    }
                    
                    state.generate_available_actions();
                    Ok(())
                } else {
                    Err(CommandRejection::NonCritical(format!("Item '{}' is not in this room.", item_id)))
                }
            } else {
                Err(CommandRejection::Critical("No current room.".to_string()))
            }
        }

        Command::Narrate { .. } | Command::AudioCue { .. } | Command::VisualEffect { .. } | Command::Flee { .. } | Command::DmChoose { .. } => {
            Ok(())
        }

        _ => Err(CommandRejection::NonCritical(format!("Command type not yet implemented."))),
    }
}