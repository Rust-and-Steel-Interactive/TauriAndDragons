pub mod combat;
pub mod commands;
pub mod state;
pub mod validator;
pub mod procedural;
pub mod pathfinding;

use state::{SessionState, ability_modifier, compute_player_ac, compute_enemy_ac, CombatResources, Enemy};
use validator::{roll_dice, roll_dice_expr};
use rand::Rng;
use std::sync::Arc;
use crate::campaign::schema::CampaignData;

pub struct GameEngine {
    pub state: SessionState,
    pub campaign: Arc<CampaignData>,
}

impl GameEngine {
    pub fn new_with_state(state: SessionState, campaign: Arc<CampaignData>) -> Self {
        Self { state, campaign }
    }

    // ─── Combat State Machine ───────────────────────────────────────

    pub fn check_combat_state(&mut self) {
        if self.state.game_mode == state::GameMode::GameOver {
            return;
        }

        // Phase 1 — snapshot current FOV enemies and spotted set (no overlapping borrows)
        let (visible_ids, any_spotted_or_visible_alive) = {
            let spotted = &self.state.spotted_enemy_ids;
            let room = match self.state.get_current_room() {
                Some(r) => r,
                None => return,
            };

            let vis: Vec<String> = state::get_visible_enemies(room)
                .iter()
                .map(|e| e.id.clone())
                .collect();

            // Alive if either previously-spotted OR currently-visible enemy has hp > 0
            let alive = room.enemies.iter().any(|e| e.hp > 0 && (spotted.contains(&e.id) || vis.contains(&e.id)));

            (vis, alive)
        };

        // Phase 2 — mutate state (room borrow is dropped)
        for id in &visible_ids {
            self.state.spotted_enemy_ids.insert(id.clone());
        }

        if any_spotted_or_visible_alive {
            if self.state.game_mode != state::GameMode::Combat {
                println!("[DEBUG check_combat_state] Transition to Combat");
                self.state.game_mode = state::GameMode::Combat;
                self.roll_initiative();
            } else {
                self.add_new_visible_enemies_to_initiative();
            }
        } else {
            if self.state.game_mode == state::GameMode::Combat {
                println!("[DEBUG check_combat_state] Transition to Exploration (combat ended)");
                self.state.game_mode = state::GameMode::Exploration;
                self.state.initiative_order.clear();
                self.state.current_turn_index = 0;
                self.state.combat_resources.clear();
                self.state.combat_log.clear();
                self.state.round_number = 0;
                self.state.spotted_enemy_ids.clear();
                let room_id = self.state.current_room_id.clone();
                self.recover_ammo_from_combat(&room_id);
            } else if self.state.initiative_order.is_empty() {
                self.state.initiative_order = vec!["player".to_string()];
            }
        }
    }

    pub fn process_commands(&mut self, llm_response: &crate::engine::commands::LlmResponse) {
        for cmd_value in &llm_response.commands {
            match serde_json::from_value::<crate::engine::commands::Command>(cmd_value.clone()) {
                Ok(cmd) => {
                    match crate::engine::validator::validate_and_execute(&cmd, &mut self.state) {
                        Ok(_) => println!("✅ Command executed: {:?}", cmd),
                        Err(crate::engine::validator::CommandRejection::Critical(msg)) => {
                            println!("🚫 Command CRITICAL rejection: {:?} - {}", cmd, msg);
                            break;
                        }
                        Err(crate::engine::validator::CommandRejection::NonCritical(msg)) => {
                            println!("⚠️ Command non-critical rejection: {:?} - {}", cmd, msg);
                        }
                    }
                }
                Err(e) => {
                    println!("⚠️ Failed to parse LLM command JSON: {} | Raw: {}", e, cmd_value);
                }
            }
        }
    }

    // ─── Initiative ──────────────────────────────────────────────────

    fn roll_initiative(&mut self) {
        let mut rng = rand::thread_rng();
        let mut entries = Vec::new();
        let mut resources = std::collections::HashMap::new();

        // Player
        let dex_mod = ability_modifier(self.state.player.dexterity);
        let player_roll = rng.gen_range(1..=20) + dex_mod;
        entries.push(state::InitiativeEntry {
            id: "player".to_string(),
            roll: player_roll,
            bonus: dex_mod,
            name: self.state.player.name.clone(),
        });
        resources.insert("player".to_string(), CombatResources::new(self.state.player.speed));

        // Enemies — snapshot spotted IDs then iterate room (no overlapping borrows)
        let enemy_entries: Vec<(state::InitiativeEntry, i32)> = {
            let spotted = &self.state.spotted_enemy_ids;
            let mut out = Vec::new();
            if let Some(room) = self.state.get_current_room() {
                for enemy in &room.enemies {
                    if enemy.hp <= 0 { continue; }
                    if !spotted.contains(&enemy.id) { continue; }
                    let enemy_dex_mod = ability_modifier(enemy.dexterity);
                    let enemy_roll = rng.gen_range(1..=20) + enemy_dex_mod;
                    out.push((
                        state::InitiativeEntry {
                            id: enemy.id.clone(),
                            roll: enemy_roll,
                            bonus: enemy_dex_mod,
                            name: enemy.name.clone(),
                        },
                        enemy.speed,
                    ));
                }
            }
            out
        };

        for (entry, speed) in enemy_entries {
            resources.insert(entry.id.clone(), CombatResources::new(speed));
            entries.push(entry);
        }

        entries.sort_by(|a, b| b.roll.cmp(&a.roll).then_with(|| b.bonus.cmp(&a.bonus)));

        self.state.initiative_entries = entries.clone();
        self.state.initiative_order = entries.iter().map(|e| e.id.clone()).collect();
        self.state.combat_resources = resources;
        self.state.current_turn_index = 0;
        self.state.round_number = 1;
        self.state.combat_log.clear();

        let order_str = self.state.initiative_entries.iter()
            .map(|e| format!("{} ({})", e.name, e.roll))
            .collect::<Vec<_>>().join(" → ");
        self.state.last_roll = format!("Initiative: {}", order_str);
        self.state.log_combat(format!("⚔️ Combat begins! Initiative: {}", order_str));
    }

    /// Add any newly-spotted enemies not already in the initiative order.
    fn add_new_visible_enemies_to_initiative(&mut self) {
        let mut rng = rand::thread_rng();
        let mut new_entries: Vec<(state::InitiativeEntry, i32)> = Vec::new();

        // Snapshot spotted set + initiative order, then iterate room
        {
            let spotted = &self.state.spotted_enemy_ids;
            let order = &self.state.initiative_order;
            if let Some(room) = self.state.get_current_room() {
                for enemy in &room.enemies {
                    if enemy.hp <= 0 { continue; }
                    if !spotted.contains(&enemy.id) { continue; }
                    if order.contains(&enemy.id) { continue; }
                    let enemy_dex_mod = ability_modifier(enemy.dexterity);
                    let enemy_roll = rng.gen_range(1..=20) + enemy_dex_mod;
                    new_entries.push((
                        state::InitiativeEntry {
                            id: enemy.id.clone(),
                            roll: enemy_roll,
                            bonus: enemy_dex_mod,
                            name: enemy.name.clone(),
                        },
                        enemy.speed,
                    ));
                }
            }
        }

        for (entry, speed) in new_entries {
            let name = entry.name.clone();
            let eid = entry.id.clone();
            self.state.initiative_entries.push(entry);
            self.state.initiative_order.push(eid.clone());
            let mut res = CombatResources::new(speed);
            res.has_action = false; // cannot act until next round
            self.state.combat_resources.insert(eid.clone(), res);
            self.state.log_combat(format!("{} joins combat! Initiative: {}.", name, self.state.initiative_entries.last().map(|e| e.roll).unwrap_or(0)));
        }
    }

    // ─── Turn Flow ───────────────────────────────────────────────────

    pub fn start_turn(&mut self, combatant_id: &str) {
        let speed = if combatant_id == "player" {
            self.state.player.speed
        } else {
            self.state.get_current_room()
                .and_then(|r| state::get_visible_enemies(r).into_iter().find(|e| e.id == combatant_id))
                .map(|e| e.speed)
                .unwrap_or(30)
        };

        if let Some(res) = self.state.combat_resources.get_mut(combatant_id) {
            res.reset_turn(speed);
            res.is_dodging = false;
            res.is_disengaging = false;
        }

        // Start-of-turn effects: regeneration
        if combatant_id != "player" {
            let should_regen = self.state.get_current_room()
                .and_then(|r| r.enemies.iter().find(|e| e.id == combatant_id))
                .map(|e| e.has_perk("REGENERATION") && e.hp > 0 && e.hp < e.max_hp)
                .unwrap_or(false);

            if should_regen {
                if let Some(room) = self.state.get_current_room_mut() {
                    if let Some(e) = room.enemies.iter_mut().find(|e| e.id == combatant_id) {
                        let heal = 5.min(e.max_hp - e.hp);
                        e.hp += heal;
                        self.state.log_combat(format!("{} regenerates {} HP.", combatant_id, heal));
                    }
                }
            }
        }
    }

    pub fn end_turn(&mut self, combatant_id: &str) {
        // Tick light source when player completes a combat turn
        if combatant_id == "player" {
            println!("[DEBUG end_turn] player turn ending, ticking light source");
            self.tick_light_source();
        }

        // Check if readied action should persist (it doesn't — readied action uses the reaction)
        if let Some(res) = self.state.combat_resources.get_mut(combatant_id) {
            // Disengage ends at end of turn
            res.is_disengaging = false;

            // Readied action expires at end of turn if not triggered
            if res.has_readied_action {
                res.has_readied_action = false;
                res.readied_trigger = None;
                res.readied_action_type = None;
                res.readied_target_id = None;
            }
        }
    }

    pub fn advance_turn(&mut self) {
        // Remove dead enemies from initiative (combat persists regardless of visibility)
        let alive_enemy_ids: Vec<String> = self.state.get_current_room()
            .map(|r| {
                r.enemies.iter()
                    .filter(|e| e.hp > 0)
                    .map(|e| e.id.clone())
                    .collect()
            })
            .unwrap_or_default();

        self.state.initiative_order.retain(|id| id == "player" || alive_enemy_ids.contains(id));

        if self.state.initiative_order.is_empty() {
            return;
        }

        let prev_index = self.state.current_turn_index;
        self.state.current_turn_index = (self.state.current_turn_index + 1) % self.state.initiative_order.len();

        // If we wrapped around, new round
        if self.state.current_turn_index <= prev_index {
            self.state.round_number += 1;
            // Reset reactions for all combatants at start of new round
            for res in self.state.combat_resources.values_mut() {
                res.reset_reaction();
            }
            self.state.log_combat(format!("─── Round {} ───", self.state.round_number));
        }

        // Start the next combatant's turn
        if let Some(id) = self.state.get_current_turn_id().cloned() {
            self.start_turn(&id);
            let name = self.state.initiative_entries.iter()
                .find(|e| e.id == id)
                .map(|e| e.name.as_str())
                .unwrap_or(&id);
            self.state.log_combat(format!("▶ {}'s turn", name));
        }
    }

    /// End the turn loop: wrap up player turn, advance initiative, generate actions for next combatant.
    fn advance_turn_order(&mut self) {
        self.end_turn("player");
        self.advance_turn();
        if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
            self.state.generate_available_actions();
        } else {
            self.state.available_actions.clear();
        }
    }

    /// Check if player has exhausted all combat resources; if so, auto-advance turn.
    fn check_turn_completion(&mut self) {
        if self.state.game_mode != state::GameMode::Combat { return; }
        let is_player_turn = self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false);
        if !is_player_turn { return; }
        if let Some(res) = self.state.combat_resources.get("player") {
            if !res.has_action && res.remaining_movement_ft == 0 && !res.has_bonus_action {
                self.advance_turn_order();
            }
        }
    }

    /// Explicit END_TURN: forfeit remaining resources and pass the turn.
    fn handle_end_turn(&mut self) -> String {
        self.state.last_roll = "End Turn".to_string();
        self.state.log_combat("Player ends their turn.".to_string());
        self.advance_turn_order();
        "SYS_MSG:_SILENT".to_string()
    }

    // ─── Opportunity Attacks (position-based) ─────────────────────────

    fn trigger_opportunity_attack(&mut self, _target_id: &str, provoked_by: &str) -> String {
        let mut rng = rand::thread_rng();
        let mut log = String::new();

        let player_pos = (self.state.player.x, self.state.player.y);

        let enemy_data: Vec<(i32, i32, String, i32, bool)> = self.state.get_current_room()
            .map(|r| r.enemies.iter()
                .filter(|e| e.hp > 0)
                .map(|e| {
                    let has_reaction = self.state.combat_resources.get(&e.id)
                        .map(|r| r.has_reaction).unwrap_or(true);
                    (e.x, e.y, e.name.clone(), e.get_effective_attack_bonus(), has_reaction)
                })
                .collect())
            .unwrap_or_default();

        for (ex, ey, enemy_name, atk_bonus, has_reaction) in &enemy_data {
            if !has_reaction { continue; }

            // Only trigger OA if enemy is adjacent to the fleeing target
            if !state::is_adjacent(player_pos.0, player_pos.1, *ex, *ey) {
                continue;
            }

            // Use reaction
            // Lookup enemy_id by position + name
            let enemy_id = self.state.get_current_room()
                .and_then(|r| r.enemies.iter()
                    .find(|e| e.hp > 0 && e.x == *ex && e.y == *ey && e.name == *enemy_name)
                    .map(|e| e.id.clone()));
            
            if let Some(ref eid) = enemy_id {
                if let Some(res) = self.state.combat_resources.get_mut(eid) {
                    res.has_reaction = false;
                }
            }

            let atk_roll = rng.gen_range(1..=20);
            let is_crit = atk_roll == 20;
            let atk_total = atk_roll + atk_bonus;
            let player_ac = compute_player_ac(&self.state.player);

            let (dmg_dice, dmg_bonus) = self.state.get_current_room()
                .and_then(|r| r.enemies.iter().find(|e| e.x == *ex && e.y == *ey && e.name == *enemy_name))
                .map(|e| (e.damage_dice.clone(), e.get_damage_bonus()))
                .unwrap_or(("1d4".to_string(), 0));
            if is_crit || atk_total >= player_ac {
                let (dice_only, embedded) = roll_dice_expr(&dmg_dice);
                let dice_roll = if is_crit { dice_only * 2 } else { dice_only };
                let dmg_roll = dice_roll + embedded + dmg_bonus;
                self.state.apply_damage("player", dmg_roll, state::DamageType::default()).unwrap(); // TODO(Phase 7): use enemy's own damage_type once added
                log.push_str(&format!("[OA] {} strikes {} as they flee! d20+{}={} vs AC {}. {} damage. ", 
                    enemy_name, provoked_by, atk_bonus, atk_total, player_ac, dmg_roll));
            } else {
                log.push_str(&format!("[OA] {} swings at {} but misses. d20+{}={} vs AC {}. ", 
                    enemy_name, provoked_by, atk_bonus, atk_total, player_ac));
            }
        }
        log
    }

    // ─── Enemy AI with A* Pathfinding ────────────────────────────────

    fn enemy_move_along_path(&mut self, enemy_id: &str, path: &[(i32, i32)], movement: i32) -> (i32, String) {
        let steps = movement.min(path.len() as i32);
        if steps <= 0 || path.is_empty() {
            return (0, String::new());
        }

        let target = path[(steps - 1) as usize];
        let cost;
        let enemy_name;

        // Update enemy position (first borrow)
        if let Some(room) = self.state.get_current_room_mut() {
            if let Some(enemy) = room.enemies.iter_mut().find(|e| e.id == enemy_id) {
                enemy.x = target.0;
                enemy.y = target.1;
                enemy_name = enemy.name.clone();
                cost = movement;
            } else {
                return (0, String::new());
            }
        } else {
            return (0, String::new());
        }

        // Update movement (second borrow, after room borrow released)
        if let Some(res) = self.state.combat_resources.get_mut(enemy_id) {
            let deduction = (cost * state::TILE_SIZE_FEET) as u32;
            res.remaining_movement_ft = res.remaining_movement_ft.saturating_sub(deduction);
        }

        (steps, format!("{} moves {} tiles.", enemy_name, steps))
    }

    fn enemy_try_attack(&mut self, enemy_id: &str, enemy: &Enemy, target_id: &str, target_x: i32, target_y: i32) -> String {
        // Check action resource
        let has_action = self.state.combat_resources.get(enemy_id)
            .map(|r| r.has_action).unwrap_or(false);
        if !has_action {
            return String::new();
        }

        // Re-fetch live enemy position from room (enemy param may be stale after movement)
        let (ex, ey) = self.state.get_current_room()
            .and_then(|r| r.enemies.iter().find(|e| e.id == enemy_id))
            .map(|e| (e.x, e.y))
            .unwrap_or((enemy.x, enemy.y));

        let mut rng = rand::thread_rng();
        let attack_range = state::get_enemy_attack_range(enemy);
        let dist = state::chebyshev_distance(ex, ey, target_x, target_y);
        let range_band = crate::engine::combat::classify_range(dist, &attack_range);

        if range_band == crate::engine::combat::RangeBand::OutOfRange {
            return String::new();
        }

        // LOS check
        let room = self.state.get_current_room().unwrap();
        let tiles = &room.tiles;
        if !state::has_line_of_sight(tiles, ex, ey, target_x, target_y) {
            return String::new();
        }

        let target_ac = compute_player_ac(&self.state.player);
        let atk_roll = rng.gen_range(1..=20);
        let is_crit = atk_roll == 20;
        let mut atk_bonus = enemy.get_effective_attack_bonus();
        if range_band == crate::engine::combat::RangeBand::LongRange {
            atk_bonus -= 5;
        }
        let atk_total = atk_roll + atk_bonus;

        // Consume the action resource
        if let Some(res) = self.state.combat_resources.get_mut(enemy_id) {
            res.has_action = false;
        }

        if is_crit || atk_total >= target_ac {
            let (dice_only, embedded) = roll_dice_expr(&enemy.damage_dice);
            let dice_roll = if is_crit { dice_only * 2 } else { dice_only };
            let dmg_roll = dice_roll + embedded + enemy.get_damage_bonus();

            self.state.apply_damage(target_id, dmg_roll, state::DamageType::default()).unwrap(); // TODO(Phase 7): use enemy's own damage_type once added
            self.state.last_roll = format!("{} d20+{} = {} (HIT) Dmg={} ({}){}",
                enemy.name, atk_bonus, atk_total, dmg_roll, enemy.damage_dice,
                if is_crit { " [CRIT!]" } else { "" });
            self.state.log_combat(format!("{} attacks {}: d20+{}={} vs AC {}. Hits for {} damage.",
                enemy.name, target_id, atk_bonus, atk_total, target_ac, dmg_roll));

            format!(
                "You are the Dungeon Master. Narrate the enemy's attack turn. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                 --- ENGINE FACT PACKET ---\n\
                 EVENT_TYPE: EnemyCombatAction\n\
                 RESOLVED_ACTION:\n  actor: \"{}\"\n  action: \"ATTACK\"\n  target: \"{}\"\n  outcome: \"HIT\"\n\
                 DICE_ROLLS:\n  - Attack: d20+{} = {} vs AC {}\n  - Damage: {} = {}{}\n\
                 STATE_DELTAS:\n  - {} HP is now {}.\n",
                enemy.name, target_id, atk_bonus, atk_total, target_ac, enemy.damage_dice, dmg_roll,
                if is_crit { " (CRITICAL)" } else { "" }, target_id,
                if target_id == "player" { self.state.player.hp }
                else { self.state.get_current_room().and_then(|r| r.enemies.iter().find(|e| e.id == target_id)).map(|e| e.hp).unwrap_or(0) }
            )
        } else {
            self.state.last_roll = format!("{} d20+{} = {} (MISS)", enemy.name, atk_bonus, atk_total);
            self.state.log_combat(format!("{} attacks {}: d20+{}={} vs AC {}. Misses.",
                enemy.name, target_id, atk_bonus, atk_total, target_ac));
            format!(
                "You are the Dungeon Master. Narrate the enemy's attack turn. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                 --- ENGINE FACT PACKET ---\n\
                 EVENT_TYPE: EnemyCombatAction\n\
                 RESOLVED_ACTION:\n  actor: \"{}\"\n  action: \"ATTACK\"\n  target: \"{}\"\n  outcome: \"MISS\"\n\
                 DICE_ROLLS:\n  - Attack: d20+{} = {} vs AC {}\n\
                 STATE_DELTAS:\n  - (none)\n",
                enemy.name, target_id, atk_bonus, atk_total, target_ac
            )
        }
    }

    /// Execute a single step of patrol/wander/guard behaviour for an NPC that can't see the player.
    fn npc_do_patrol_move(&mut self, enemy_id: &str) -> String {
        // Read current enemy state (clone to avoid borrow issues)
        let info = self.state.get_current_room()
            .and_then(|r| r.enemies.iter().find(|e| e.id == enemy_id))
            .cloned();
        let (ex, ey, behaviour) = match info {
            Some(ref e) if e.hp > 0 => (e.x, e.y, e.behaviour.clone()),
            _ => return String::new(),
        };

        let movement_budget = self.state.combat_resources.get(enemy_id)
            .map(|r| r.remaining_movement_ft)
            .unwrap_or(0);

        let tiles_for_movement = (movement_budget / state::TILE_SIZE_FEET as u32) as i32;
        if tiles_for_movement <= 0 {
            return String::new();
        }

        // Determine goal for each behaviour type
        let goal = match &behaviour {
            state::NpcBehaviour::Patrol { waypoints, current_index } => {
                if waypoints.is_empty() { None }
                else { Some(waypoints[*current_index % waypoints.len()]) }
            }
            state::NpcBehaviour::Guard { guard_x, guard_y, patrol_radius } => {
                // Move toward guard position if far away, otherwise idle
                let dist = state::manhattan_distance(ex, ey, *guard_x, *guard_y);
                if dist > *patrol_radius { Some((*guard_x, *guard_y)) } else { None }
            }
            state::NpcBehaviour::Wander { anchor_x, anchor_y, wander_radius } => {
                let mut rng = rand::thread_rng();
                let tx = *anchor_x + rng.gen_range(-*wander_radius..=*wander_radius);
                let ty = *anchor_y + rng.gen_range(-*wander_radius..=*wander_radius);
                let (max_x, max_y) = self.state.get_current_room()
                    .map(|r| (r.tile_width - 1, r.tile_height - 1))
                    .unwrap_or((30, 30));
                Some((tx.clamp(1, max_x), ty.clamp(1, max_y)))
            }
            state::NpcBehaviour::Investigate { target_x, target_y, .. } => {
                Some((*target_x, *target_y))
            }
            state::NpcBehaviour::Idle => None,
        };

        let goal = match goal {
            Some(g) => g,
            None => return String::new(),
        };

        let occupied = state::get_occupied_tiles(
            self.state.get_current_room().unwrap(),
            Some(enemy_id),
        );

        let path = pathfinding::find_path(
            &self.state.get_current_room().unwrap().tiles,
            ex, ey, goal.0, goal.1,
            &occupied,
            tiles_for_movement as i32,
        );

        if let Some(p) = path {
            let (_steps, log) = self.enemy_move_along_path(enemy_id, &p, tiles_for_movement as i32);
            if !log.is_empty() {
                self.state.log_combat(log.clone());
                // Advance patrol waypoint
                if let state::NpcBehaviour::Patrol { waypoints, current_index: _ } = &behaviour {
                    if let Some(room) = self.state.get_current_room_mut() {
                        if let Some(enemy) = room.enemies.iter_mut().find(|e| e.id == enemy_id) {
                            if let state::NpcBehaviour::Patrol { ref mut current_index, .. } = &mut enemy.behaviour {
                                let last = goal;
                                if (enemy.x - last.0).abs() <= 1 && (enemy.y - last.1).abs() <= 1 {
                                    *current_index = (*current_index + 1) % waypoints.len();
                                }
                            }
                        }
                    }
                }
            }
        }
        String::new()
    }

    pub fn handle_enemy_turn(&mut self, enemy_id: &str) -> String {
        self.state.last_roll = "None".to_string();

        let mut rng = rand::thread_rng();

        let enemy_info = self.state.get_current_room()
            .and_then(|r| r.enemies.iter().find(|e| e.id == enemy_id))
            .cloned();

        let enemy = match enemy_info {
            Some(e) => e,
            None => return String::new(),
        };

        if enemy.hp <= 0 {
            return String::new();
        }

        // Check whether the player can currently see this enemy (for narration suppression)
        let is_enemy_visible = self.state.get_current_room()
            .and_then(|r| r.tiles.get(enemy.y as usize))
            .and_then(|row| row.get(enemy.x as usize))
            .map(|t| t.visibility == state::TileVisibility::Visible)
            .unwrap_or(false);

        // ── Detection check: can this enemy see the player? ──
        let tiles = self.state.get_current_room().map(|r| &r.tiles);
        let can_see_player = tiles.map(|t| {
            state::can_enemy_see_player(&enemy, self.state.player.x, self.state.player.y, t)
        }).unwrap_or(false);

        // ── Awareness state transition ──
        let enemy_name = enemy.name.clone();
        if can_see_player && enemy.awareness != state::AwarenessState::Alert {
            if let Some(room) = self.state.get_current_room_mut() {
                if let Some(e) = room.enemies.iter_mut().find(|e| e.id == enemy_id) {
                    e.awareness = state::AwarenessState::Alert;
                }
            }
            self.state.log_combat(format!("{} spots you and becomes hostile!", enemy_name));
        }

        let awareness = self.state.get_current_room()
            .and_then(|r| r.enemies.iter().find(|e| e.id == enemy_id))
            .map(|e| e.awareness.clone())
            .unwrap_or(state::AwarenessState::Unaware);

        // If enemy can't see the player, just do patrol/wander behaviour
        if !can_see_player || awareness != state::AwarenessState::Alert {
            let patrol_result = self.npc_do_patrol_move(enemy_id);

            // Mark action as used
            if let Some(res) = self.state.combat_resources.get_mut(enemy_id) {
                res.has_action = false;
            }
            self.end_turn(enemy_id);

            if patrol_result.is_empty() {
                // Truly idle — no fact packet needed for silent patrol
                return String::new();
            }
            return patrol_result;
        }

        // ── Enemy CAN see the player — combat AI ──
        let pct_hp = enemy.hp as f32 / enemy.max_hp as f32;
        let attack_range = state::get_enemy_attack_range(&enemy);
        let dist_to_player = state::chebyshev_distance(enemy.x, enemy.y, self.state.player.x, self.state.player.y);
        let movement_budget = self.state.combat_resources.get(enemy_id)
            .map(|r| r.remaining_movement_ft)
            .unwrap_or(0);
        let tiles_for_movement = (movement_budget / state::TILE_SIZE_FEET as u32) as i32;

        let mut fact_packet = String::new();

        // ── AI Decision ──
        enum EnemyAction {
            AttackAndMove,
            MoveAndAttack,
            MoveTowardPlayer,
            Dodge,
            Disengage,
            Hide,
            NoOp,
        }

        let can_attack_in_place = crate::engine::combat::classify_range(dist_to_player, &attack_range) != crate::engine::combat::RangeBand::OutOfRange
            && self.state.get_current_room()
                .map(|r| state::has_line_of_sight(&r.tiles, enemy.x, enemy.y, self.state.player.x, self.state.player.y))
                .unwrap_or(false);

        let action = if enemy.has_perk("NIMBLE_ESCAPE") && pct_hp < 0.5 {
            if rng.gen_bool(0.5) { EnemyAction::Disengage } else { EnemyAction::Hide }
        } else if pct_hp < 0.25 && rng.gen_bool(0.4) {
            EnemyAction::Dodge
        } else if !can_attack_in_place && tiles_for_movement > 0 {
            EnemyAction::MoveTowardPlayer
        } else if can_attack_in_place {
            EnemyAction::AttackAndMove
        } else {
            EnemyAction::NoOp
        };

        // ── Execute ──
        let mut is_attack = false;
        match action {
            EnemyAction::AttackAndMove | EnemyAction::MoveAndAttack => {
                let attack_result = self.enemy_try_attack(enemy_id, &enemy, "player",
                    self.state.player.x, self.state.player.y);
                is_attack = !attack_result.is_empty();
                fact_packet.push_str(&attack_result);
            }
            EnemyAction::MoveTowardPlayer => {
                let room_ref = self.state.get_current_room().unwrap();
                let tiles = &room_ref.tiles;
                let px = self.state.player.x;
                let py = self.state.player.y;

                // Block player's tile so enemies never path onto it
                let mut occupied = state::get_occupied_tiles(room_ref, Some(enemy_id));
                occupied.push((px, py));

                let attack_range = state::get_enemy_attack_range(&enemy);
                let is_ranged = attack_range.normal > 1;

                // Find goal tiles: preferred distance depends on weapon
                let goal = {
                    let pref_dist: i32 = if is_ranged { attack_range.normal.min(6) } else { 1 };
                    // Build a list of all unoccupied, in-bounds tiles at Chebyshev distance pref_dist from player
                    let adj_candidates = {
                        let mut c = Vec::new();
                        for dx in -(pref_dist)..=pref_dist {
                            for dy in -(pref_dist)..=pref_dist {
                                if dx.abs() != pref_dist && dy.abs() != pref_dist { continue; }
                                let gx = px + dx;
                                let gy = py + dy;
                                if gx >= 0 && gy >= 0 && gy < tiles.len() as i32 && gx < tiles[0].len() as i32
                                    && !state::is_tile_blocked(tiles, gx, gy)
                                    && !occupied.contains(&(gx, gy))
                                {
                                    c.push((gx, gy));
                                }
                            }
                        }
                        c
                    };
                    // Prefer closest candidate; if none at preferred distance, try distance 1 (melee)
                    let mut candidates = adj_candidates;
                    if candidates.is_empty() && pref_dist > 1 {
                        let fallback = [
                            (px - 1, py - 1), (px, py - 1), (px + 1, py - 1),
                            (px - 1, py),                     (px + 1, py),
                            (px - 1, py + 1), (px, py + 1), (px + 1, py + 1),
                        ];
                        candidates = fallback.iter()
                            .filter(|&&(gx, gy)| {
                                gx >= 0 && gy >= 0 && gy < tiles.len() as i32 && gx < tiles[0].len() as i32
                                    && !state::is_tile_blocked(tiles, gx, gy)
                                    && !occupied.contains(&(gx, gy))
                            })
                            .copied().collect();
                    }
                    candidates.iter()
                        .min_by_key(|&&(gx, gy)| state::manhattan_distance(enemy.x, enemy.y, gx, gy))
                        .copied()
                };

                let path = goal.and_then(|g| {
                    pathfinding::find_path(
                        tiles, enemy.x, enemy.y, g.0, g.1, &occupied, tiles_for_movement as i32,
                    )
                });

                let moved = if let Some(p) = path {
                    let (steps, log) = self.enemy_move_along_path(enemy_id, &p, tiles_for_movement as i32);
                    if steps > 0 {
                        let dist_left = state::manhattan_distance(
                            self.state.player.x, self.state.player.y,
                            self.state.get_current_room()
                                .and_then(|r| r.enemies.iter().find(|e| e.id == enemy_id))
                                .map(|e| e.x).unwrap_or(0),
                            self.state.get_current_room()
                                .and_then(|r| r.enemies.iter().find(|e| e.id == enemy_id))
                                .map(|e| e.y).unwrap_or(0),
                        );
                        self.state.log_combat(format!("{} moves {} tile(s) toward you ({} away).", enemy.name, steps, dist_left));
                        fact_packet.push_str(&format!(
                            "You are the Dungeon Master. {} moves through the room. \
                             Narrate the repositioning. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                             --- ENGINE FACT PACKET ---\n\
                             EVENT_TYPE: EnemyCombatAction\n\
                             RESOLVED_ACTION:\n  actor: \"{}\"\n  action: \"MOVE\"\n  outcome: \"SUCCESS\"\n",
                            enemy.name, enemy.name
                        ));
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };

                // Re-fetch enemy position after move, then check if now in attack range
                let now_adjacent = {
                    let room = self.state.get_current_room();
                    room.and_then(|r| {
                        r.enemies.iter().find(|e| e.id == enemy_id).map(|e| {
                            let dist = state::chebyshev_distance(e.x, e.y, self.state.player.x, self.state.player.y);
                            let e_range = state::get_enemy_attack_range(e);
                            crate::engine::combat::classify_range(dist, &e_range) != crate::engine::combat::RangeBand::OutOfRange
                            && state::has_line_of_sight(
                                &r.tiles, e.x, e.y,
                                self.state.player.x, self.state.player.y,
                            )
                        })
                    }).unwrap_or(false)
                };

                if now_adjacent {
                    let attack_result = self.enemy_try_attack(enemy_id, &enemy, "player",
                        self.state.player.x, self.state.player.y);
                    if !attack_result.is_empty() {
                        fact_packet.push_str(&attack_result);
                    }
                } else if !moved {
                    // Could not move into range — dodge instead of standing idle
                    if let Some(res) = self.state.combat_resources.get_mut(enemy_id) {
                        res.is_dodging = true;
                    }
                    self.state.log_combat(format!("{} takes the Dodge action.", enemy.name));
                    fact_packet = format!(
                        "You are the Dungeon Master. {} takes the Dodge action. Narrate. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                         --- ENGINE FACT PACKET ---\n\
                         EVENT_TYPE: EnemyCombatAction\n\
                         RESOLVED_ACTION:\n  actor: \"{}\"\n  action: \"DODGE\"\n  outcome: \"SUCCESS\"",
                        enemy.name, enemy.name
                    );
                }
            }
            EnemyAction::Dodge => {
                if let Some(res) = self.state.combat_resources.get_mut(enemy_id) {
                    res.is_dodging = true;
                }
                self.state.log_combat(format!("{} takes the Dodge action.", enemy.name));
                fact_packet = format!(
                    "You are the Dungeon Master. {} takes the Dodge action. Narrate. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: EnemyCombatAction\n\
                     RESOLVED_ACTION:\n  actor: \"{}\"\n  action: \"DODGE\"\n  outcome: \"SUCCESS\"",
                    enemy.name, enemy.name
                );
            }
            EnemyAction::Disengage => {
                if let Some(res) = self.state.combat_resources.get_mut(enemy_id) {
                    res.is_disengaging = true;
                }
                self.state.log_combat(format!("{} disengages.", enemy.name));
                fact_packet = format!(
                    "You are the Dungeon Master. {} disengages, backing away. Narrate. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: EnemyCombatAction\n\
                     RESOLVED_ACTION:\n  actor: \"{}\"\n  action: \"DISENGAGE\"\n  outcome: \"SUCCESS\"",
                    enemy.name, enemy.name
                );
            }
            EnemyAction::Hide => {
                self.state.log_combat(format!("{} attempts to hide.", enemy.name));
                fact_packet = format!(
                    "You are the Dungeon Master. {} attempts to hide. Narrate. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: EnemyCombatAction\n\
                     RESOLVED_ACTION:\n  actor: \"{}\"\n  action: \"HIDE\"\n  outcome: \"SUCCESS\"",
                    enemy.name, enemy.name
                );
            }
            EnemyAction::NoOp => {
                self.state.log_combat(format!("{} stands ready.", enemy.name));
                fact_packet = format!(
                    "You are the Dungeon Master. {} stands ready, watching. Narrate. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: EnemyCombatAction\n\
                     RESOLVED_ACTION:\n  actor: \"{}\"\n  action: \"READY\"\n  outcome: \"SUCCESS\"",
                    enemy.name, enemy.name
                );
            }
        }

        // Mark action and movement as consumed for enemy
        if let Some(res) = self.state.combat_resources.get_mut(enemy_id) {
            res.has_action = false;
            res.remaining_movement_ft = 0;
        }
        self.end_turn(enemy_id);

        if self.state.game_mode == state::GameMode::GameOver {
            if is_enemy_visible {
                fact_packet.push_str("\nTRIGGER: PLAYER_DEATH. The player has fallen in battle.");
            }
        }

        // Suppress fact packet for non-attack actions by enemies the player cannot see
        if !is_enemy_visible && !is_attack {
            String::new()
        } else {
            fact_packet
        }
    }

    // ─── Button Actions (new action economy) ─────────────────────────

    pub fn handle_button_action(&mut self, action_id: &str) -> String {
        // Clear last roll so only rolls set during THIS action are emitted
        self.state.last_roll = "None".to_string();

        println!("⚙️ Engine resolving action: {}", action_id);

        // ── 1. Movement (tile and room) ──
        if action_id.starts_with("MOVE_TO_") {
            return self.handle_move(action_id);
        }
        if action_id.starts_with("MOVE_") && (action_id == "MOVE_NORTH" || action_id == "MOVE_SOUTH"
            || action_id == "MOVE_EAST" || action_id == "MOVE_WEST")
        {
            return self.handle_tile_move(action_id);
        }

        // ── 2. Combat actions ──
        if self.state.game_mode == state::GameMode::Combat {
            // Check it's the player's turn
            if self.state.get_current_turn_id().map(|id| id != "player").unwrap_or(false) {
                return "SYS_MSG:It's not your turn.".to_string();
            }
        }

        if action_id.starts_with("ACTION_ATTACK_") {
            return self.handle_combat_attack(action_id);
        }
        if action_id == "ACTION_DASH" {
            return self.handle_dash();
        }
        if action_id == "ACTION_DODGE" {
            return self.handle_dodge();
        }
        if action_id == "ACTION_DISENGAGE" {
            return self.handle_disengage();
        }
        if action_id == "ACTION_HIDE" {
            return self.handle_hide();
        }
        if action_id == "ACTION_READY" {
            return self.handle_ready();
        }
        if action_id.starts_with("BONUS_OFFHAND_ATTACK_") {
            return self.handle_offhand_attack(action_id);
        }
        if action_id == "ACTION_FLEE" {
            return self.handle_flee();
        }
        if action_id == "ACTION_END_TURN" {
            return self.handle_end_turn();
        }
        if action_id.starts_with("ACTION_STUDY_") {
            return self.handle_study(action_id);
        }

        // ── Legacy compat: pre-combat attack → enters combat ──
        if action_id.starts_with("ATTACK_") {
            return self.handle_combat_attack(&format!("ACTION_{}", action_id));
        }

        // ── 3. Non-combat actions (work in both modes) ──
        if action_id.starts_with("USE_ITEM_") {
            return self.handle_use_item(action_id);
        }
        if action_id.starts_with("EQUIP_ITEM_PRIMARY_") {
            return self.handle_equip_item(action_id, "primary");
        }
        if action_id.starts_with("EQUIP_ITEM_SECONDARY_") {
            return self.handle_equip_item(action_id, "secondary");
        }
        if action_id.starts_with("EQUIP_ITEM_") {
            return self.handle_equip_item(action_id, "auto");
        }
        if action_id.starts_with("EQUIP_ARMOUR_") {
            return self.handle_equip_armour(action_id);
        }
        if action_id.starts_with("UNEQUIP_ARMOUR_") {
            return self.handle_unequip_armour(action_id);
        }
        if action_id.starts_with("TAKE_ITEM_") {
            return self.handle_take_item(action_id);
        }
        if action_id == "SEARCH_AREA" {
            return self.handle_search();
        }
        if action_id.starts_with("OPEN_CHEST_") {
            return self.handle_open_chest(action_id);
        }
        if action_id.starts_with("PICK_LOCK_") {
            return self.handle_pick_lock(action_id);
        }
        if action_id.starts_with("PICK_UP_TORCH_") {
            return self.handle_pick_up_torch(action_id);
        }
        if action_id.starts_with("REFILL_LANTERN_") {
            return self.handle_refill_lantern(action_id);
        }
        if action_id.starts_with("EQUIP_BELT_") {
            return self.handle_equip_belt(action_id);
        }
        if action_id == "UNEQUIP_BELT" {
            return self.handle_unequip_belt();
        }
        if action_id.starts_with("MOUNT_UTILITY_") {
            return self.handle_mount_utility(action_id);
        }
        if action_id.starts_with("UNMOUNT_UTILITY_") {
            return self.handle_unmount_utility(action_id);
        }
        if action_id == "UNEQUIP_HAND_PRIMARY" {
            return self.handle_unequip_hand("primary");
        }
        if action_id == "UNEQUIP_HAND_SECONDARY" {
            return self.handle_unequip_hand("secondary");
        }
        if action_id.starts_with("STUDY_") {
            // Legacy study outside combat
            return self.handle_study(&format!("ACTION_{}", action_id));
        }
        if action_id == "FLEE" {
            return self.handle_flee();
        }

        "You are the Dungeon Master. The player's action was not recognized. Narrate this. Respond ONLY with JSON: {\"narration\": \"...\", \"commands\": []}".to_string()
    }

    // ─── Action Implementations ──────────────────────────────────────

    fn consume_action(&mut self, combatant: &str) -> bool {
        if let Some(res) = self.state.combat_resources.get_mut(combatant) {
            if res.has_action {
                res.has_action = false;
                return true;
            }
        }
        false
    }

    fn consume_bonus_action(&mut self, combatant: &str) -> bool {
        if let Some(res) = self.state.combat_resources.get_mut(combatant) {
            if res.has_bonus_action {
                res.has_bonus_action = false;
                return true;
            }
        }
        false
    }

    // ─── Tile Movement & Visibility ──────────────────────────────────

    /// Parse chest position from name field "Name [x:y]"
    fn parse_chest_position(name: &str) -> (i32, i32) {
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

    fn light_source_status(&self) -> String {
        self.state.player.active_light_source
            .as_ref()
            .map(|light| format!("Lit (Radius {}, {} turns remaining)", light.radius, light.remaining_turns))
            .unwrap_or_else(|| "Unlit (Darkness)".to_string())
    }

    fn tick_light_source(&mut self) {
        if let Some(light) = &mut self.state.player.active_light_source {
            if light.remaining_turns > 0 {
                light.remaining_turns -= 1;
            }
            println!("[DEBUG tick_light_source] remaining_turns={}, will_extinguish={}", light.remaining_turns, light.remaining_turns == 0);
            if light.remaining_turns == 0 {
                let is_lantern = light.item_id == "lantern";
                println!("[DEBUG tick_light_source] EXTINGUISHING light (was {})", if is_lantern { "lantern" } else { "torch" });
                self.state.player.active_light_source = None;

                if is_lantern {
                    // Lantern goes out but stays in inventory or utility slot
                    if let Some(lantern) = self.state.player.inventory.iter_mut().find(|i| i.template_id == "lantern") {
                        lantern.is_lit = Some(false);
                        lantern.current_fuel = Some(0);
                    }
                    if let Some(lantern) = self.state.player.utility_slots.iter_mut().flatten().find(|i| i.template_id == "lantern") {
                        lantern.is_lit = Some(false);
                        lantern.current_fuel = Some(0);
                    }
                    self.state.log_combat("Your lantern sputters out of oil!".to_string());
                } else {
                    self.state.log_combat("Your torch flickers and dies, plunging you back into gloom.".to_string());
                }
                self.update_visibility();
            }
        }
        let extinguished: Vec<(i32, i32)> = {
            let mut v = Vec::new();
            if let Some(room) = self.state.get_current_room_mut() {
                for row in room.tiles.iter_mut() {
                    for tile in row.iter_mut() {
                        if let Some(light) = &mut tile.ground_light_source {
                            if light.remaining_turns > 0 {
                                light.remaining_turns -= 1;
                            }
                            if light.remaining_turns == 0 {
                                v.push((tile.x, tile.y));
                            }
                        }
                    }
                }
            }
            v
        };
        if !extinguished.is_empty() {
            for (gx, gy) in &extinguished {
                if let Some(room) = self.state.get_current_room_mut() {
                    room.tiles[*gy as usize][*gx as usize].ground_light_source = None;
                }
                self.state.log_combat("A torch on the ground sputters and dies.".to_string());
            }
            self.update_visibility();
        }
    }

    pub fn update_visibility(&mut self) {
        let px = self.state.player.x;
        let py = self.state.player.y;
        let base_radius: i32 = self.state.player.active_light_source
            .as_ref()
            .map(|light| light.radius as i32)
            .unwrap_or(1);

        let is_belt_mounted = self.state.player.active_light_source
            .as_ref()
            .map(|light| light.is_belt_mounted)
            .unwrap_or(false);
        let vision_radius = if is_belt_mounted { (base_radius - 1).max(1) } else { base_radius };
        println!("[DEBUG update_visibility] active_light_source={:?}, base_radius={}, is_belt_mounted={}, vision_radius={}",
            self.state.player.active_light_source.as_ref().map(|l| (l.remaining_turns, l.radius)),
            base_radius, is_belt_mounted, vision_radius);

        // Collect light-source positions (viewer coords + radius) using an immutable borrow
        let light_sources: Vec<(i32, i32, i32)> = {
            let room = match self.state.get_current_room() { Some(r) => r, None => return };
            let mut sources = vec![(px, py, vision_radius)];
            for row in &room.tiles {
                for tile in row {
                    if let Some(gl) = &tile.ground_light_source {
                        sources.push((tile.x, tile.y, gl.radius as i32));
                    }
                }
            }
            sources
        };

        // Compute which tiles are visible via line-of-sight from each light source
        let mut visible_set: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
        if let Some(room) = self.state.get_current_room() {
            let tiles = &room.tiles;
            for &(lx, ly, radius) in &light_sources {
                for row in tiles.iter() {
                    for tile in row.iter() {
                        let (tx, ty) = (tile.x, tile.y);
                        if (tx - lx).abs() > radius || (ty - ly).abs() > radius {
                            continue;
                        }
                        if tx == lx && ty == ly {
                            visible_set.insert((tx, ty));
                            continue;
                        }
                        if state::has_line_of_sight(tiles, lx, ly, tx, ty) {
                            visible_set.insert((tx, ty));
                        }
                    }
                }
            }
        }

        // Apply visibility with a mutable borrow
        if let Some(room) = self.state.get_current_room_mut() {
            for row in room.tiles.iter_mut() {
                for tile in row.iter_mut() {
                    if tile.visibility == state::TileVisibility::Visible {
                        tile.visibility = state::TileVisibility::Explored;
                    }
                    if visible_set.contains(&(tile.x, tile.y)) {
                        tile.visibility = state::TileVisibility::Visible;
                    }
                }
            }

            // Reveal chests using LOS from player
            for chest in room.chests.iter_mut() {
                if chest.is_revealed || chest.broken { continue; }
                let (cx, cy) = Self::parse_chest_position(&chest.name);
                if (cx - px).abs() <= vision_radius && (cy - py).abs() <= vision_radius {
                    if state::has_line_of_sight(&room.tiles, px, py, cx, cy) {
                        chest.is_revealed = true;
                    }
                }
            }
        }
    }

    fn check_discoveries(&mut self) -> Option<String> {
        let px = self.state.player.x;
        let py = self.state.player.y;
        let discovered_chest = {
            let room = self.state.get_current_room()?;
            let mut found: Option<String> = None;
            for chest in &room.chests {
                if chest.is_revealed || chest.broken { continue; }
                let (cx, cy) = Self::parse_chest_position(&chest.name);
                if (cx - px).abs() <= 1 && (cy - py).abs() <= 1 {
                    let name = chest.name.split('[').next().unwrap_or("chest").trim();
                    found = Some(format!("You spot {} nearby.", name));
                    break;
                }
            }
            if found.is_none() && !room.loot.is_empty() && !room.loot_noticed {
                for item in &room.loot {
                    if let (Some(lx), Some(ly)) = (item.placed_x, item.placed_y) {
                        if (lx - px).abs() <= 4 && (ly - py).abs() <= 4 {
                            found = Some("You notice valuables scattered nearby.".to_string());
                            break;
                        }
                    }
                }
            }
            found
        };
        if let Some(ref msg) = discovered_chest {
            self.state.log_combat(format!("[Discovery] {}", msg));
            if let Some(room) = self.state.get_current_room_mut() {
                room.loot_noticed = true;
            }
        }
        discovered_chest
    }

    fn handle_tile_move(&mut self, action_id: &str) -> String {
        let (dx, dy) = match action_id {
            "MOVE_NORTH" => (0, -1),
            "MOVE_SOUTH" => (0, 1),
            "MOVE_EAST" => (1, 0),
            "MOVE_WEST" => (-1, 0),
            _ => return "SYS_MSG:Invalid direction.".to_string(),
        };

        // Combat movement: consume 5 ft per tile from movement budget
        if self.state.game_mode == state::GameMode::Combat {
            let budget = self.state.combat_resources.get("player")
                .map(|r| r.remaining_movement_ft)
                .unwrap_or(0);
            if budget < state::TILE_SIZE_FEET as u32 {
                return "SYS_MSG:You have no movement remaining this turn.".to_string();
            }
            if let Some(res) = self.state.combat_resources.get_mut("player") {
                res.remaining_movement_ft = res.remaining_movement_ft.saturating_sub(state::TILE_SIZE_FEET as u32);
            }
        }

        let new_x = self.state.player.x + dx;
        let new_y = self.state.player.y + dy;

        if let Some(room) = self.state.get_current_room() {
            if new_x < 0 || new_y < 0 || new_y >= room.tile_height as i32 || new_x >= room.tile_width as i32 {
                return "SYS_MSG:Cannot move in that direction.".to_string();
            }
            if let Some(row) = room.tiles.get(new_y as usize) {
                if let Some(tile) = row.get(new_x as usize) {
                    if tile.tile_type == state::TileType::Wall {
                        return "SYS_MSG:A wall blocks your path.".to_string();
                    }
                }
            }
        } else {
            return "SYS_MSG:No current room.".to_string();
        }

        let was_not_combat = self.state.game_mode != state::GameMode::Combat;
        self.state.player.x = new_x;
        self.state.player.y = new_y;

        // Auto-transition if stepping onto a Door tile (extract target first to avoid borrow conflict)
        let door_target: Option<String> = self.state.get_current_room().and_then(|room| {
            let tile = room.tiles.get(new_y as usize)?.get(new_x as usize)?;
            if tile.tile_type != state::TileType::Door { return None; }
            let conn_idx = if new_y == 0 { 0 }
                else if new_x == room.tile_width as i32 - 1 { 1 }
                else if new_y == room.tile_height as i32 - 1 { 2 }
                else if new_x == 0 { 3 }
                else { return None; };
            room.connections.get(conn_idx).cloned()
        });
        if let Some(target_room) = door_target {
            return self.handle_move(&format!("MOVE_TO_{}", target_room));
        }

        println!("[DEBUG MOVE] calling tick_light_source (game_mode={:?})", self.state.game_mode);
        self.tick_light_source();
        self.update_visibility();
        println!("[DEBUG MOVE] after update_visibility, active_light_source={:?}", self.state.player.active_light_source.as_ref().map(|l| (l.remaining_turns, l.radius)));
        self.check_combat_state();
        if self.state.game_mode == state::GameMode::Combat {
            self.check_turn_completion();
        }
        let combat_started = was_not_combat && self.state.game_mode == state::GameMode::Combat;

        // Note: enemy turns are NOT processed here — main.rs combat loop
        // handles all enemy turns visibly so the player sees their actions and rolls

        let mut trap_triggered = false;
        let mut trap_spotted = false;
        let torch_bonus = if self.state.player.active_light_source.is_some() { 2 } else { 0 };
        let mut trap_info: Option<(String, i32, i32, i32, i32, i32, state::DamageType)> = None;

        if let Some(room) = self.state.get_current_room_mut() {
            if !room.traps.is_empty() && !room.is_trap_triggered {
                let trap = room.traps[0].clone();
                let mut rng = rand::thread_rng();
                let perc_roll = rng.gen_range(1..=20);
                let perc_bonus = state::ability_modifier(self.state.player.wisdom);
                let perc_total = perc_roll + perc_bonus + torch_bonus;

                if perc_total >= trap.dc {
                    trap_spotted = true;
                    trap_info = Some((trap.name.clone(), trap.dc, perc_total, 0, perc_bonus, torch_bonus, trap.damage_type));
                } else {
                    trap_triggered = true;
                    let dmg_roll = crate::engine::validator::roll_dice(&trap.damage);
                    trap_info = Some((trap.name.clone(), trap.dc, perc_total, dmg_roll, perc_bonus, torch_bonus, trap.damage_type));
                }
            }
        }

        self.state.generate_available_actions();

        let discovery = self.check_discoveries();

        // Only narrate when something meaningful happens
        let has_event = trap_info.is_some() || combat_started || discovery.is_some();
        if has_event {
            let mut fact_packet = format!(
                "You are the Dungeon Master. The player moved one tile in the current room. \
                 Narrate briefly. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                 --- ENGINE FACT PACKET ---\n\
                 EVENT_TYPE: PlayerMovement\n\
                 RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"TILE_MOVE\"\n  new_position: ({}, {})",
                self.state.player.x, self.state.player.y
            );
            if let Some((trap_name, trap_dc, perc_total, dmg_roll, perc_bonus, torch_bonus, trap_damage_type)) = trap_info {
                let roll_label = if torch_bonus > 0 {
                    format!("d20+{}+{} (Torch) = {}", perc_bonus, torch_bonus, perc_total)
                } else {
                    format!("d20+{} = {}", perc_bonus, perc_total)
                };
                if trap_spotted {
                    self.state.last_roll = format!("{} (SUCCESS) vs DC {}", roll_label, trap_dc);
                    fact_packet.push_str(&format!(
                        "\nTRIGGER: PLAYER_SPOTTED_TRAP ({})! Perception {} vs DC {}.",
                        trap_name, roll_label, trap_dc
                    ));
                } else if trap_triggered {
                    self.state.apply_damage("player", dmg_roll, trap_damage_type).unwrap();
                    if let Some(room) = self.state.get_current_room_mut() {
                        room.is_trap_triggered = true;
                    }
                    self.state.last_roll = format!("{} (FAILED) vs DC {} - {} damage", roll_label, trap_dc, dmg_roll);
                    fact_packet.push_str(&format!(
                        "\nTRIGGER: TRAP_TRIGGERED ({})! Failed perception ({} vs DC {}). Dealt {} damage. HP now {}.",
                        trap_name, roll_label, trap_dc, dmg_roll, self.state.player.hp
                    ));
                }
            }
            if combat_started {
                let visible_enemies = self.state.get_current_room()
                    .map(|r| state::get_visible_enemies(r))
                    .unwrap_or_default();
                let enemy_names = visible_enemies.iter()
                    .filter(|e| e.hp > 0).map(|e| e.name.clone())
                    .collect::<Vec<_>>().join(", ");
                let init_order: Vec<String> = self.state.initiative_entries.iter()
                    .map(|e| format!("{} ({})", e.name, e.roll)).collect();
                fact_packet.push_str(&format!(
                    "\nCOMBAT_INITIATED! Visible enemies: {enemy_names}.\n\
                     Describe the sudden clash — weapons drawn, enemies reacting with surprise or aggression, \
                     the first moments of combat exploding into action. Use vivid action verbs. Do NOT resolve or conclude the fight.\n\
                     Initiative order: {init_order:?}."
                ));
            }
            if let Some(ref disc) = discovery {
                fact_packet.push_str(&format!("\nDISCOVERY: {}. The player has noticed something of interest.", disc));
            }
            fact_packet
        } else {
            // Silent move: no LLM narration, just update state
            String::from("SYS_MSG:_SILENT")
        }
    }

    fn handle_move(&mut self, action_id: &str) -> String {
        self.state.last_loot.clear();

        let target_room_id = action_id.trim_start_matches("MOVE_TO_").to_string();
        let current_room_id = self.state.current_room_id.clone();

        // Pre-compute entry data from the current room (before mutable borrow on rooms)
        let entry_idx = {
            let current_room = self.state.rooms.iter().find(|r| r.id == current_room_id);
            let (px, py, cr_conns, cr_w) = match current_room {
                Some(r) => (self.state.player.x, self.state.player.y, &r.connections, r.tile_width),
                None => (0, 0, &vec![], 0),
            };

            let exit = if py == 0 { Some(0usize) }
                else if px == (cr_w - 1) { Some(1) }
                else if py == (current_room.map(|r| r.tile_height).unwrap_or(0) - 1) { Some(2) }
                else if px == 0 { Some(3) }
                else { None };

            let entry = exit.and_then(|ei| {
                // Verify the exit edge actually leads to the target room
                let _ = cr_conns.get(ei).filter(|c| *c == &target_room_id)?;
                // Find this same connection ID's index in the target room
                self.state.rooms.iter()
                    .find(|r| r.id == target_room_id)?
                    .connections.iter()
                    .position(|c| c == &current_room_id)
            });

            entry
        };

        self.recover_ammo_from_combat(&current_room_id);
        self.state.current_room_id = target_room_id.clone();

        if let Some(room) = self.state.rooms.iter_mut().find(|r| r.id == target_room_id) {
            room.visited = true;

            // Place player at the door corresponding to the entry index
            let spawn_pos = if let Some(dir) = entry_idx {
                let (nx, ny) = match dir % 4 {
                    0 => (room.tile_width / 2, 0),                       // north door
                    1 => (room.tile_width - 1, room.tile_height / 2),    // east door
                    2 => (room.tile_width / 2, room.tile_height - 1),    // south door
                    3 => (0, room.tile_height / 2),                      // west door
                    _ => (room.entrance_x, room.entrance_y),
                };
                (nx, ny)
            } else {
                (room.entrance_x, room.entrance_y)
            };
            // Verify the spawn tile is passable (Door or Floor); fall back to entrance if not
            let tile_is_passable = room.tiles.get(spawn_pos.1 as usize)
                .and_then(|row| row.get(spawn_pos.0 as usize))
                .map(|t| matches!(t.tile_type, state::TileType::Floor | state::TileType::Door | state::TileType::Stairs))
                .unwrap_or(false);
            if tile_is_passable {
                self.state.player.x = spawn_pos.0;
                self.state.player.y = spawn_pos.1;
            } else {
                self.state.player.x = room.entrance_x;
                self.state.player.y = room.entrance_y;
            }
        }
        // Exiting the current room ends combat
        if self.state.game_mode == state::GameMode::Combat {
            self.state.game_mode = state::GameMode::Exploration;
            self.state.initiative_order.clear();
            self.state.current_turn_index = 0;
            self.state.combat_resources.clear();
            self.state.combat_log.clear();
            self.state.round_number = 0;
            self.state.spotted_enemy_ids.clear();
        }
        let discovery = self.check_discoveries();
        self.update_visibility();
        self.check_combat_state();

        let room_name = self.state.get_current_room().map(|r| r.name.as_str()).unwrap_or("Unknown");
        let room_desc = self.state.get_current_room().map(|r| r.description.as_str()).unwrap_or("");

        let mut fact_packet = format!(
            "You are the Dungeon Master. The player just moved into a new room.\n\
             Describe the atmosphere and mood — the sights, sounds, and smells. Do NOT mention specific objects, loot, chests, \
             or interactables unless they are directly obvious (e.g., a roaring fire, an enormous statue). \
             Keep details vague: \"something glints in the shadows\", \"you sense this room may contain secrets\", \
             \"dusty shapes are barely visible\". Respond ONLY with a JSON object using this exact schema:\n\
             {{\n  \"narration\": \"Your atmospheric prose here...\",\n  \"commands\": [\n    {{ \"type\": \"AUDIO_CUE\", \"cue\": \"footsteps\" }}\n  ]\n}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: ModeTransition\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"MOVE_TO\"\n  target: \"{}\"\n\
              CURRENT_SITUATION:\n  Room Name: {}\n  Description: {}\n  Light Source: {}\n",
            target_room_id, room_name, room_desc, self.light_source_status()
        );

        // Trap logic
        let mut trap_triggered = false;
        let mut trap_spotted = false;
        let mut trap_info: Option<(String, i32, i32, i32, i32, state::DamageType)> = None;

        if let Some(room) = self.state.get_current_room_mut() {
            if !room.traps.is_empty() && !room.is_trap_triggered {
                let trap = room.traps[0].clone();
                let mut rng = rand::thread_rng();
                let perc_roll = rng.gen_range(1..=20);
                let perc_bonus = ability_modifier(self.state.player.wisdom);
                let perc_total = perc_roll + perc_bonus;

                if perc_total >= trap.dc {
                    trap_spotted = true;
                    trap_info = Some((trap.name.clone(), trap.dc, perc_total, 0, perc_bonus, trap.damage_type));
                } else {
                    trap_triggered = true;
                    let dmg_roll = roll_dice(&trap.damage);
                    trap_info = Some((trap.name.clone(), trap.dc, perc_total, dmg_roll, perc_bonus, trap.damage_type));
                }
            }
        }

        if let Some((trap_name, trap_dc, perc_total, dmg_roll, perc_bonus, trap_damage_type)) = trap_info {
            if trap_spotted {
                self.state.last_roll = format!("d20+{} = {} (SUCCESS) vs DC {}", perc_bonus, perc_total, trap_dc);
                fact_packet.push_str(&format!("\nTRIGGER: PLAYER_SPOTTED_TRAP ({})! Perception check d20+{} = {} vs DC {}.", trap_name, perc_bonus, perc_total, trap_dc));
            } else if trap_triggered {
                self.state.apply_damage("player", dmg_roll, trap_damage_type).unwrap();
                if let Some(room) = self.state.get_current_room_mut() {
                    room.is_trap_triggered = true;
                }
                fact_packet.push_str(&format!(
                    "\nTRIGGER: TRAP_TRIGGERED ({})! Player failed perception (d20+{} = {} vs DC {}). Trap dealt {} damage. Player HP is now {}.",
                    trap_name, perc_bonus, perc_total, trap_dc, dmg_roll, self.state.player.hp
                ));
            }
        }

        if let Some(ref disc) = discovery {
            fact_packet.push_str(&format!("\nDISCOVERY: {}. The player has noticed something of interest.", disc));
        }

        // Surprise attacks on entering combat
        if self.state.game_mode == state::GameMode::Combat {
            let visible_enemies = self.state.get_current_room()
                .map(|r| state::get_visible_enemies(r))
                .unwrap_or_default();
            let enemy_names = visible_enemies.iter()
                .filter(|e| e.hp > 0).map(|e| e.name.clone())
                .collect::<Vec<_>>().join(", ");
            let init_order: Vec<String> = self.state.initiative_entries.iter()
                .map(|e| format!("{} ({})", e.name, e.roll)).collect();
            fact_packet.push_str(&format!(
                "\nCOMBAT_INITIATED! The player steps into a room with enemies: {enemy_names}.\n\
                 Describe the sudden clash — weapons drawn, enemies reacting with surprise or aggression, \
                 the first moments of combat exploding into action. Use vivid action verbs. Do NOT resolve or conclude the fight.\n\
                 Initiative order: {init_order:?}."
            ));

            // Note: enemy turns are NOT processed here — main.rs combat loop
            // handles all enemy turns visibly so the player sees their actions and rolls
            self.state.generate_available_actions();
        } else {
            self.state.generate_available_actions();
        }

        fact_packet
    }

    fn handle_combat_attack(&mut self, action_id: &str) -> String {
        if !self.consume_action("player") {
            return "SYS_MSG:You have no action remaining this turn.".to_string();
        }

        let target_id = action_id
            .trim_start_matches("ACTION_ATTACK_")
            .trim_start_matches("ATTACK_")
            .to_string();

        // ── Visibility check: only visible enemies can be targeted ──
        let target_visible = self.state.get_current_room()
            .and_then(|r| {
                r.enemies.iter()
                    .find(|e| e.id == target_id)
                    .map(|e| {
                        r.tiles.get(e.y as usize)
                            .and_then(|row| row.get(e.x as usize))
                            .map(|t| t.visibility == state::TileVisibility::Visible)
                            .unwrap_or(false)
                    })
            })
            .unwrap_or(false);

        if !target_visible {
            return "SYS_MSG:That enemy is not visible. Move closer to reveal them.".to_string();
        }

        // ── Range check (all weapon classes) ──
        let target_dist = self.state.get_current_room()
            .and_then(|r| r.enemies.iter().find(|e| e.id == target_id))
            .map(|e| state::chebyshev_distance(e.x, e.y, self.state.player.x, self.state.player.y));

        let target_dist = match target_dist {
            Some(d) => d,
            None => return "SYS_MSG:Target enemy not found.".to_string(),
        };

        let weapon_range = state::get_weapon_range(&self.state.player);
        let weapon_item_class = self.state.get_equipped_weapon().map(|w| w.item_class.clone());
        let is_ranged_style = matches!(weapon_item_class.as_deref(), Some("RANGED") | Some("MAGIC"));

        // ── Ammo check (RANGED weapons that require ammo only; MAGIC is exempt) ──
        if weapon_item_class.as_deref() == Some("RANGED") {
            if let Some(required_ammo) = self.state.get_equipped_weapon().and_then(|w| w.ammo_type.clone()) {
                if !state::has_matching_ammo(&self.state.player, &required_ammo) {
                    return format!("SYS_MSG:You're out of {} for your weapon.", required_ammo);
                }
            }
        }

        let has_los = self.state.get_current_room()
            .and_then(|r| r.enemies.iter().find(|e| e.id == target_id).map(|e| (r, e)))
            .map(|(r, e)| state::has_line_of_sight(&r.tiles, self.state.player.x, self.state.player.y, e.x, e.y))
            .unwrap_or(false);

        let enemy_adjacent_to_player = self.state.get_current_room()
            .map(|r| r.enemies.iter().any(|e| e.hp > 0 && state::is_adjacent(e.x, e.y, self.state.player.x, self.state.player.y)))
            .unwrap_or(false);

        let (weapon_name, dmg_dice, dmg_bonus, atk_stat_mod, weapon_damage_type) = {
            if let Some(w) = self.state.get_equipped_weapon() {
                let stat_mod = match w.item_class.as_str() {
                    "RANGED" => ability_modifier(self.state.player.dexterity),
                    "MAGIC" => ability_modifier(self.state.player.intelligence),
                    _ => ability_modifier(self.state.player.strength),
                };
                (w.display_name.clone(), w.damage_dice.clone().unwrap_or("1d3".to_string()), w.damage_bonus.unwrap_or(0), stat_mod, w.damage_type.unwrap_or_default())
            } else {
                let stat_mod = ability_modifier(self.state.player.strength);
                ("Unarmed Strike".to_string(), "1d4".to_string(), 0, stat_mod, state::DamageType::default())
            }
        };

        let (enemy_ac, enemy_name, enemy_dodging) = {
            let room = self.state.get_current_room().unwrap();
            let enemy = room.enemies.iter().find(|e| e.id == target_id).unwrap();
            (compute_enemy_ac(enemy), enemy.name.clone(), self.state.combat_resources.get(&enemy.id).map(|r| r.is_dodging).unwrap_or(false))
        };

        let effective_ac = if enemy_dodging { enemy_ac + 5 } else { enemy_ac };
        let total_dmg_bonus = dmg_bonus + atk_stat_mod;

        let ctx = crate::engine::combat::AttackContext {
            distance: target_dist,
            weapon_range,
            has_line_of_sight: has_los,
            is_ranged_style,
            hostile_adjacent_to_attacker: enemy_adjacent_to_player,
            attack_stat_mod: atk_stat_mod,
            proficiency_bonus: self.state.player.proficiency_bonus,
            target_ac: enemy_ac,
            target_dodging: enemy_dodging,
        };

        let outcome = crate::engine::combat::resolve_attack_roll(&ctx);

        let mut fact_packet = String::new();
        let mut roll_str = String::new();

        match outcome {
            crate::engine::combat::AttackOutcome::OutOfRange => {
                return "SYS_MSG:Target is out of range for your weapon.".to_string();
            }
            crate::engine::combat::AttackOutcome::NoLineOfSight => {
                return "SYS_MSG:You don't have a clear line of sight to that target.".to_string();
            }
            crate::engine::combat::AttackOutcome::Hit { atk_bonus, atk_total, is_crit, .. } => {
                let (dice_only, embedded) = roll_dice_expr(&dmg_dice);
                let dmg_roll = crate::engine::combat::resolve_damage_roll(
                    dice_only, atk_stat_mod, dmg_bonus + embedded, is_crit,
                );

                self.state.apply_damage(&target_id, dmg_roll, weapon_damage_type).unwrap();

                roll_str = format!("d20+{} = {} (HIT) vs AC {}. Dmg: {}{} = {}{}",
                    atk_bonus, atk_total, effective_ac, dmg_dice,
                    if total_dmg_bonus >= 0 { format!("+{}", total_dmg_bonus) } else { format!("{}", total_dmg_bonus) },
                    dmg_roll, if is_crit { " [CRIT!]" } else { "" });

                self.state.log_combat(format!("{} attacks {}: d20+{}={} vs AC {}. Hits for {} damage{}.",
                    "player", enemy_name, atk_bonus, atk_total, effective_ac, dmg_roll,
                    if is_crit { " (CRITICAL!)" } else { "" }));

                fact_packet = format!(
                    "You are the Dungeon Master. Narrate this combat exchange. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: PlayerCombatAction\n\
                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"ATTACK\"\n  target: \"{}\"\n  outcome: \"HIT\"\n  weapon: \"{}\"\n\
                     DICE_ROLLS:\n  - Attack: d20+{} = {} vs AC {}\n  - Damage: {}{} = {}{}\n\
                     STATE_DELTAS:\n  - {} HP is now {}.\n",
                    enemy_name, weapon_name, atk_bonus, atk_total, effective_ac, dmg_dice,
                    if total_dmg_bonus >= 0 { format!("+{}", total_dmg_bonus) } else { format!("{}", total_dmg_bonus) },
                    dmg_roll, if is_crit { " (CRITICAL)" } else { "" }, enemy_name,
                    self.state.get_current_room().unwrap().enemies.iter().find(|e| e.id == target_id).map(|e| e.hp).unwrap_or(0)
                );

                // Check enemy death
                let is_dead = self.state.get_current_room()
                    .and_then(|r| r.enemies.iter().find(|e| e.id == target_id))
                    .map(|e| e.hp <= 0).unwrap_or(false);

                if is_dead {
                    fact_packet.push_str(&format!("\nTRIGGER: ENEMY_DEATH ({}). COMBAT_MAY_END.", enemy_name));
                    let (loot_items, xp_val) = {
                        let dead_enemy = self.state.get_current_room().unwrap().enemies.iter()
                            .find(|e| e.id == target_id).unwrap();
                        (self.generate_loot_for_enemy(dead_enemy), dead_enemy.xp)
                    };
                    for item in &loot_items {
                        state::add_to_inventory(&mut self.state.player.inventory, item.clone());
                    }
                    let gp = (xp_val / 5).max(1);
                    self.state.player.gp += gp;
                    self.state.last_loot.push(state::LootGroup {
                        source_name: enemy_name.clone(),
                        gp,
                        items: loot_items,
                    });
                }
            }
            crate::engine::combat::AttackOutcome::Miss { atk_bonus, atk_total, .. } => {
                roll_str = format!("d20+{} = {} (MISS) vs AC {}", atk_bonus, atk_total, effective_ac);
                self.state.log_combat(format!("{} attacks {}: d20+{}={} vs AC {}. Misses.",
                    "player", enemy_name, atk_bonus, atk_total, effective_ac));
                fact_packet = format!(
                    "You are the Dungeon Master. Narrate this combat exchange. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: PlayerCombatAction\n\
                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"ATTACK\"\n  target: \"{}\"\n  outcome: \"MISS\"\n  weapon: \"{}\"\n\
                     DICE_ROLLS:\n  - Attack: d20+{} = {} vs AC {}\n\
                     STATE_DELTAS:\n  - (none)\n",
                    enemy_name, weapon_name, atk_bonus, atk_total, effective_ac
                );
            }
        }

        self.state.last_roll = roll_str;

        // ── Ammo consumption (fires on hit or miss, once the ammo check in Step 21 has passed) ──
        if weapon_item_class.as_deref() == Some("RANGED") {
            if let Some(required_ammo) = self.state.get_equipped_weapon().and_then(|w| w.ammo_type.clone()) {
                if state::consume_matching_ammo(&mut self.state.player, &required_ammo) {
                    *self.state.ammo_consumed_this_combat.entry(required_ammo).or_insert(0) += 1;
                }
            }
        }

        if let Some(room) = self.state.get_current_room_mut() {
            room.enemies.retain(|e| e.hp > 0);
        }
        self.check_combat_state();

        self.end_turn("player");

        if self.state.game_mode != state::GameMode::Combat {
            fact_packet.push_str("\nTRIGGER: COMBAT_END (Victory).");
            self.state.generate_available_actions();
        } else {
            self.advance_turn();
            if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
                self.state.generate_available_actions();
            } else {
                self.state.available_actions.clear();
            }
        }
        fact_packet
    }

    fn handle_offhand_attack(&mut self, action_id: &str) -> String {
        if !self.consume_bonus_action("player") {
            return "SYS_MSG:You have no bonus action remaining this turn.".to_string();
        }

        let target_id = action_id
            .trim_start_matches("BONUS_OFFHAND_ATTACK_")
            .to_string();
        let mut rng = rand::thread_rng();

        // ── Visibility check ──
        let target_visible = self.state.get_current_room()
            .and_then(|r| {
                r.enemies.iter()
                    .find(|e| e.id == target_id)
                    .map(|e| {
                        r.tiles.get(e.y as usize)
                            .and_then(|row| row.get(e.x as usize))
                            .map(|t| t.visibility == state::TileVisibility::Visible)
                            .unwrap_or(false)
                    })
            })
            .unwrap_or(false);

        if !target_visible {
            return "SYS_MSG:That enemy is not visible.".to_string();
        }

        // ── Melee range check (off-hand is melee) ──
        let in_range = self.state.get_current_room()
            .and_then(|r| r.enemies.iter().find(|e| e.id == target_id))
            .map(|e| (e.x - self.state.player.x).abs() <= 1 && (e.y - self.state.player.y).abs() <= 1)
            .unwrap_or(false);
        if !in_range {
            return "SYS_MSG:Target is out of melee range. You must be adjacent to attack.".to_string();
        }

        let (weapon_name, dmg_dice, dmg_bonus, atk_stat_mod, weapon_damage_type) = {
            if let Some(w) = self.state.get_offhand_weapon() {
                let stat_mod = match w.item_class.as_str() {
                    "RANGED" => ability_modifier(self.state.player.dexterity),
                    "MAGIC" => ability_modifier(self.state.player.intelligence),
                    _ => ability_modifier(self.state.player.strength),
                };
                (w.display_name.clone(), w.damage_dice.clone().unwrap_or("1d3".to_string()), w.damage_bonus.unwrap_or(0), stat_mod, w.damage_type.unwrap_or_default())
            } else {
                let stat_mod = ability_modifier(self.state.player.strength);
                ("Unarmed Strike".to_string(), "1d4".to_string(), 0, stat_mod, state::DamageType::default())
            }
        };

        let atk_roll = rng.gen_range(1..=20);
        let is_crit = atk_roll == 20;
        let proficiency = self.state.player.proficiency_bonus;
        let atk_bonus = atk_stat_mod + proficiency;
        let atk_total = atk_roll + atk_bonus;

        let enemy_ac;
        let enemy_name;
        {
            let room = self.state.get_current_room().unwrap();
            let enemy = room.enemies.iter().find(|e| e.id == target_id).unwrap();
            enemy_ac = compute_enemy_ac(enemy);
            enemy_name = enemy.name.clone();
        }

        let mut fact_packet = String::new();

        if is_crit || atk_total >= enemy_ac {
            let (dice_only, embedded) = roll_dice_expr(&dmg_dice);
            let dice_roll = if is_crit { dice_only * 2 } else { dice_only };
            let dmg_roll = dice_roll + embedded + dmg_bonus;
            self.state.apply_damage(&target_id, dmg_roll, weapon_damage_type).unwrap();

            let crit_str = if is_crit { " (CRITICAL)" } else { "" };
            self.state.last_roll = format!("[Off-hand] d20+{} = {} vs AC {} (HIT) Dmg={}{}", atk_bonus, atk_total, enemy_ac, dmg_roll, crit_str);
            self.state.log_combat(format!("{} off-hand attack with {}: hits {} for {} damage{}.",
                "player", weapon_name, enemy_name, dmg_roll, crit_str));

            let enemy_hp_after = self.state.get_current_room()
                .and_then(|r| r.enemies.iter().find(|e| e.id == target_id))
                .map(|e| e.hp)
                .unwrap_or(0);

            fact_packet = format!(
                "You are the Dungeon Master. Narrate this quick off-hand attack. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                 --- ENGINE FACT PACKET ---\n\
                 EVENT_TYPE: PlayerCombatAction\n\
                 RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"OFFHAND_ATTACK\"\n  target: \"{}\"\n  outcome: \"HIT\"\n  weapon: \"{}\"\n\
                 DICE_ROLLS:\n  - Attack: d20+{} = {} vs AC {}\n  - Damage: {}{} = {}{}\n\
                 STATE_DELTAS:\n  - {} HP is now {}.\n",
                enemy_name, weapon_name, atk_bonus, atk_total, enemy_ac,
                dmg_dice, if dmg_bonus != 0 { format!("{:+}", dmg_bonus) } else { String::new() }, dmg_roll, crit_str,
                enemy_name, enemy_hp_after
            );

            let is_dead = self.state.get_current_room()
                .and_then(|r| r.enemies.iter().find(|e| e.id == target_id))
                .map(|e| e.hp <= 0).unwrap_or(false);
            if is_dead {
                fact_packet.push_str(&format!("\nTRIGGER: ENEMY_DEATH ({}). COMBAT_MAY_END.", enemy_name));
                let (loot_items, xp_val) = {
                    let dead_enemy = self.state.get_current_room().unwrap().enemies.iter()
                        .find(|e| e.id == target_id).unwrap();
                    (self.generate_loot_for_enemy(dead_enemy), dead_enemy.xp)
                };
                for item in &loot_items {
                    state::add_to_inventory(&mut self.state.player.inventory, item.clone());
                }
                let gp = (xp_val / 5).max(1);
                self.state.player.gp += gp;
                self.state.last_loot.push(state::LootGroup {
                    source_name: enemy_name.clone(),
                    gp,
                    items: loot_items,
                });
            }
        } else {
            self.state.last_roll = format!("[Off-hand] d20+{} = {} vs AC {} (MISS)", atk_bonus, atk_total, enemy_ac);
            self.state.log_combat(format!("{} off-hand attack misses {}.", "player", enemy_name));
            fact_packet = format!(
                "You are the Dungeon Master. Narrate this quick off-hand attack. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                 --- ENGINE FACT PACKET ---\n\
                 EVENT_TYPE: PlayerCombatAction\n\
                 RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"OFFHAND_ATTACK\"\n  target: \"{}\"\n  outcome: \"MISS\"\n  weapon: \"{}\"\n\
                 DICE_ROLLS:\n  - Attack: d20+{} = {} vs AC {}\n",
                enemy_name, weapon_name, atk_bonus, atk_total, enemy_ac
            );
        }

        if let Some(room) = self.state.get_current_room_mut() {
            room.enemies.retain(|e| e.hp > 0);
        }
        self.check_combat_state();

        self.end_turn("player");

        if self.state.game_mode != state::GameMode::Combat {
            fact_packet.push_str("\nTRIGGER: COMBAT_END (Victory).");
            self.state.generate_available_actions();
        } else {
            self.advance_turn();
            if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
                self.state.generate_available_actions();
            } else {
                self.state.available_actions.clear();
            }
        }

        fact_packet
    }

    fn handle_dash(&mut self) -> String {
        if !self.consume_action("player") {
            return "SYS_MSG:You have no action remaining this turn.".to_string();
        }

        let extra_speed = self.state.player.speed as u32;
        if let Some(res) = self.state.combat_resources.get_mut("player") {
            res.remaining_movement_ft = res.remaining_movement_ft.saturating_add(extra_speed);
        }

        self.state.last_roll = format!("Dash: +{} ft movement (total {} ft)", extra_speed,
            self.state.combat_resources.get("player").map(|r| r.remaining_movement_ft).unwrap_or(0));
        self.state.log_combat(format!("{} takes the Dash action. Movement increased by {}.", "player", extra_speed));

        self.end_turn("player");
        self.advance_turn();
        if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
            self.state.generate_available_actions();
        } else {
            self.state.available_actions.clear();
        }

        format!(
            "You are the Dungeon Master. The player dashes, moving with urgency. Narrate this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerCombatAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"DASH\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_dodge(&mut self) -> String {
        if !self.consume_action("player") {
            return "SYS_MSG:You have no action remaining this turn.".to_string();
        }

        if let Some(res) = self.state.combat_resources.get_mut("player") {
            res.is_dodging = true;
        }

        self.state.last_roll = "Dodge: imposing disadvantage on next attack against you (effective AC +5)".to_string();
        self.state.log_combat(format!("{} takes the Dodge action.", "player"));

        self.end_turn("player");
        self.advance_turn();
        if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
            self.state.generate_available_actions();
        } else {
            self.state.available_actions.clear();
        }

        format!(
            "You are the Dungeon Master. The player takes the Dodge action, focusing entirely on defense. Narrate this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerCombatAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"DODGE\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_disengage(&mut self) -> String {
        if !self.consume_action("player") {
            return "SYS_MSG:You have no action remaining this turn.".to_string();
        }

        if let Some(res) = self.state.combat_resources.get_mut("player") {
            res.is_disengaging = true;
        }

        self.state.last_roll = "Disengage: you can move without provoking opportunity attacks.".to_string();
        self.state.log_combat(format!("{} disengages.", "player"));

        self.end_turn("player");
        self.advance_turn();
        if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
            self.state.generate_available_actions();
        } else {
            self.state.available_actions.clear();
        }

        format!(
            "You are the Dungeon Master. The player disengages, carefully backing away from threats. Narrate this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerCombatAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"DISENGAGE\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_hide(&mut self) -> String {
        if !self.consume_action("player") {
            return "SYS_MSG:You have no action remaining this turn.".to_string();
        }

        self.state.last_roll = "Hide: you attempt to conceal yourself.".to_string();
        self.state.log_combat(format!("{} attempts to hide.", "player"));

        self.end_turn("player");
        self.advance_turn();
        if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
            self.state.generate_available_actions();
        } else {
            self.state.available_actions.clear();
        }

        format!(
            "You are the Dungeon Master. The player attempts to hide. Narrate this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerCombatAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"HIDE\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_ready(&mut self) -> String {
        if !self.consume_action("player") {
            return "SYS_MSG:You have no action remaining this turn.".to_string();
        }

        // For simplicity, prompt the player to describe their ready trigger via SYS_MSG
        self.state.last_roll = "Ready: describe your trigger and action via free text.".to_string();

        // The LLM will handle the details; we mark the state so future prompts can be processed
        // For now, this is a placeholder that ends the turn
        self.end_turn("player");
        self.advance_turn();
        if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
            self.state.generate_available_actions();
        } else {
            self.state.available_actions.clear();
        }

        format!(
            "You are the Dungeon Master. The player is preparing a readied action. Narrate them bracing for a trigger. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerCombatAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"READY\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_study(&mut self, action_id: &str) -> String {
        let target_id = action_id
            .trim_start_matches("ACTION_STUDY_")
            .trim_start_matches("STUDY_")
            .to_string();
        let mut rng = rand::thread_rng();
        let int_mod = ability_modifier(self.state.player.intelligence);
        let study_roll = rng.gen_range(1..=20) + int_mod + self.state.player.proficiency_bonus;
        let dc = 10;

        let success = study_roll >= dc;
        self.state.last_roll = format!("d20+{} = {} vs DC {} ({})", int_mod + self.state.player.proficiency_bonus, study_roll, dc, if success { "SUCCESS" } else { "FAILURE" });

        if let Some(room) = self.state.get_current_room_mut() {
            if let Some(enemy) = room.enemies.iter_mut().find(|e| e.id == target_id) {
                if success {
                    enemy.studied = true;
                }
            }
        }

        let fact_packet = format!(
            "You are the Dungeon Master. Narrate the player studying their foe. \
             The player rolled a d20+{} = {} vs DC {} ({}). \
             If successful, they recall or notice key details about the enemy's capabilities and weaknesses. \
             Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"STUDY\"\n  target: \"{}\"\n  outcome: \"{}\"\n\
             DICE_ROLLS:\n  - Study: d20+{} = {} vs DC {}\n",
            int_mod + self.state.player.proficiency_bonus, study_roll, dc,
            if success { "SUCCESS" } else { "FAILURE" },
            target_id, if success { "SUCCESS" } else { "FAILURE" },
            int_mod + self.state.player.proficiency_bonus, study_roll, dc
        );

        if self.state.game_mode == state::GameMode::Combat {
            self.end_turn("player");
            self.advance_turn();
        }
        self.state.generate_available_actions();
        fact_packet
    }

    fn handle_flee(&mut self) -> String {
        if self.state.game_mode == state::GameMode::Combat {
            if !self.consume_action("player") {
                return "SYS_MSG:You have no action remaining this turn.".to_string();
            }
        }

        let mut rng = rand::thread_rng();
        let dex_mod = ability_modifier(self.state.player.dexterity);
        let flee_roll = rng.gen_range(1..=20) + dex_mod;
        let flee_dc = 10;

        self.state.last_roll = format!("d20+{} = {} vs DC {}", dex_mod, flee_roll, flee_dc);

        if flee_roll >= flee_dc {
            // Check for opportunity attacks if not disengaging
            let is_disengaging = self.state.combat_resources.get("player")
                .map(|r| r.is_disengaging).unwrap_or(false);

            let mut oa_log = String::new();
            if !is_disengaging && self.state.game_mode == state::GameMode::Combat {
                oa_log = self.trigger_opportunity_attack("player", "player");
                if !oa_log.is_empty() {
                    self.state.last_roll += &format!(" | {}", oa_log);
                }
            }

            let old_room_id = self.state.current_room_id.clone();
            let mut flee_to_str = String::new();
            if let Some(room) = self.state.get_current_room() {
                if let Some(flee_to) = room.connections.first() {
                    flee_to_str = flee_to.clone();
                }
            }
            if !flee_to_str.is_empty() {
                self.recover_ammo_from_combat(&old_room_id);
                self.state.current_room_id = flee_to_str.clone();
                self.update_visibility();
                self.check_combat_state();
                self.state.generate_available_actions();

                self.state.log_combat(format!("{} flees to {}.", "player", flee_to_str));

                format!(
                    "You are the Dungeon Master. The player successfully fled from combat to {}. {}\
                     Narrate the escape. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: PlayerCombatAction\n\
                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"FLEE\"\n  outcome: \"SUCCESS\"\n\
                      DICE_ROLLS:\n  - Escape (DEX): d20+{} = {} vs DC {}\n",
                    flee_to_str, oa_log, dex_mod, flee_roll, flee_dc
                )
            } else {
                format!("SYS_MSG:There's nowhere to flee to.")
            }
        } else {
            // Failed flee — enemy gets an attack
            let enemy_atk = rng.gen_range(1..=20);
            let player_ac = compute_player_ac(&self.state.player);
            if enemy_atk >= player_ac {
                let enemy_dmg = rng.gen_range(1..=6);
                self.state.apply_damage("player", enemy_dmg, state::DamageType::default()).unwrap(); // TODO(Phase 7): use enemy's own damage_type once added
                self.state.last_roll += &format!(" | Enemy Attack d20={} (HIT) Dmg={}", enemy_atk, enemy_dmg);
                self.state.log_combat(format!("{} tried to flee but failed. Attacked for {} damage.", "player", enemy_dmg));

                if self.state.game_mode == state::GameMode::Combat {
                    self.end_turn("player");
                    self.advance_turn();
                    if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
                        self.state.generate_available_actions();
                    } else {
                        self.state.available_actions.clear();
                    }
                }

                format!(
                    "You are the Dungeon Master. The player failed to flee and was attacked. Narrate the failure. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: PlayerCombatAction\n\
                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"FLEE\"\n  outcome: \"FAILURE\"\n\
                      DICE_ROLLS:\n  - Escape (DEX): d20+{} = {} vs DC {}\n  - Enemy Attack: d20 = {} vs AC {} (HIT for {})\n\
                      STATE_DELTAS:\n  - Player HP is now {}.\n",
                    dex_mod, flee_roll, flee_dc, enemy_atk, player_ac, enemy_dmg, self.state.player.hp
                )
            } else {
                self.state.last_roll += &format!(" | Enemy Attack d20={} (MISS)", enemy_atk);
                self.state.log_combat(format!("{} tried to flee but failed. Enemy missed.", "player"));

                if self.state.game_mode == state::GameMode::Combat {
                    self.end_turn("player");
                    self.advance_turn();
                    if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
                        self.state.generate_available_actions();
                    } else {
                        self.state.available_actions.clear();
                    }
                }

                format!(
                    "You are the Dungeon Master. The player failed to flee, but the enemy missed. Narrate. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: PlayerCombatAction\n\
                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"FLEE\"\n  outcome: \"FAILURE\"\n\
                      DICE_ROLLS:\n  - Escape (DEX): d20+{} = {} vs DC {}\n  - Enemy Attack: d20 = {} vs AC {} (MISS)\n",
                    dex_mod, flee_roll, flee_dc, enemy_atk, player_ac
                )
            }
        }
    }

    // ─── Non-combat actions ──────────────────────────────────────────

    fn handle_use_item(&mut self, action_id: &str) -> String {
        let item_id = action_id.trim_start_matches("USE_ITEM_").to_string();
        let mut fact_packet = String::new();

        if self.state.game_mode == state::GameMode::Combat && !self.consume_action("player") {
            return "SYS_MSG:You have no action remaining this turn.".to_string();
        }

        // Build hands-occupied string for blocked narrations
        let hands_occupied = {
            let mut parts: Vec<String> = Vec::new();
            if let Some(id) = &self.state.player.primary_hand {
                if let Some(item) = self.state.player.inventory.iter().find(|i| &i.instance_id == id) {
                    parts.push(item.display_name.clone());
                }
            }
            if let Some(id) = &self.state.player.secondary_hand {
                if let Some(item) = self.state.player.inventory.iter().find(|i| &i.instance_id == id) {
                    parts.push(item.display_name.clone());
                }
            }
            match parts.len() {
                0 => String::new(),
                1 => parts[0].clone(),
                2 => format!("{} and {}", parts[0], parts[1]),
                _ => parts.join(", "),
            }
        };

        // Check if the item is mounted in a belt utility slot
        let is_belt_mounted = self.state.player.utility_slots.iter()
            .any(|s| s.as_ref().map(|i| i.instance_id == item_id).unwrap_or(false));

        // Check for lantern ignition first (needs separate borrow scope)
        let lantern_data = {
            self.state.player.inventory.iter().find(|i| i.instance_id == item_id).or_else(|| {
                self.state.player.utility_slots.iter().flatten().find(|i| i.instance_id == item_id)
            }).and_then(|item| {
                if item.template_id == "lantern" {
                    Some((item.current_fuel, item.light_radius, item.display_name.clone(), item.instance_id.clone(), item.is_lit))
                } else {
                    None
                }
            })
        };
        if let Some((current_fuel, light_radius, lantern_name, lantern_inst_id, is_lit)) = lantern_data {
            // Reject if already lit
            if is_lit == Some(true) {
                return "SYS_MSG:The lantern is already lit.".to_string();
            }
            // Lantern ignition: check tinderbox + fuel, do NOT consume quantity
            if !self.state.player.inventory.iter().any(|i| i.template_id == "tinderbox") {
                return "SYS_MSG:You need a Tinderbox to ignite the lantern.".to_string();
            }
            if current_fuel.unwrap_or(0) <= 0 {
                return "SYS_MSG:The lantern is out of oil! Refill it with an Oil Flask.".to_string();
            }
            let fuel = current_fuel.unwrap_or(0) as u32;
            let radius = light_radius.unwrap_or(5);

            if !is_belt_mounted {
                // Equip lantern to secondary_hand (like torch)
                if self.state.player.secondary_hand.is_some() {
                    if self.state.player.primary_hand.is_none() {
                        self.state.player.primary_hand = Some(lantern_inst_id.clone());
                    } else {
                        let blocked_msg = if hands_occupied.is_empty() {
                            format!(
                                "You are the Dungeon Master. The player tries to ignite their {} but both hands are already full. Narrate the Dungeon Master preventing this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                                 --- ENGINE FACT PACKET ---\n\
                                 EVENT_TYPE: PlayerAction\n\
                                 RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"USE_ITEM\"\n  target: \"{}\"\n  outcome: \"BLOCKED\"\n\
                                 STATE_DELTAS: (none — action prevented)",
                                lantern_name, lantern_name
                            )
                        } else {
                            format!(
                                "You are the Dungeon Master. The player tries to ignite their {} but their hands are already occupied by {}. Narrate the Dungeon Master preventing this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                                 --- ENGINE FACT PACKET ---\n\
                                 EVENT_TYPE: PlayerAction\n\
                                 RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"USE_ITEM\"\n  target: \"{}\"\n  outcome: \"BLOCKED\"\n\
                                 STATE_DELTAS: (none — action prevented)",
                                lantern_name, hands_occupied, lantern_name
                            )
                        };
                        return blocked_msg;
                    }
                } else {
                    self.state.player.secondary_hand = Some(lantern_inst_id.clone());
                }
            }

            // Set is_lit on the lantern instance
            if let Some(lantern) = self.state.player.inventory.iter_mut().find(|i| i.instance_id == lantern_inst_id) {
                lantern.is_lit = Some(true);
            }
            if let Some(lantern) = self.state.player.utility_slots.iter_mut().flatten().find(|i| i.instance_id == lantern_inst_id) {
                lantern.is_lit = Some(true);
            }
            self.state.player.active_light_source = Some(state::ActiveLightSource {
                item_id: "lantern".to_string(),
                radius,
                remaining_turns: fuel,
                is_belt_mounted,
            });
            self.update_visibility();
            self.state.log_combat(format!("You ignite the {}. (Radius {}, {} turns of fuel)", lantern_name, radius, fuel));
            self.state.generate_available_actions();
            return format!(
                "You are the Dungeon Master. The player ignites their lantern. Narrate the warm steady glow. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                 --- ENGINE FACT PACKET ---\n\
                 EVENT_TYPE: PlayerAction\n\
                 RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"USE_ITEM\"\n  target: \"{}\"\n  outcome: \"SUCCESS\"\n\
                 STATE_DELTAS:\n  - A lantern now illuminates the area (Radius {}, {} turns).",
                lantern_name, radius, fuel
            );
        }

        let mut item_data: Option<(String, Option<String>, String, Option<u32>, Option<u32>, Option<u32>)> = None;
        if let Some(item) = self.state.player.inventory.iter_mut().find(|i| i.instance_id == item_id) {
            if item.quantity > 0 {
                // Don't process items with no effect that aren't torch/lantern
                if item.effect.is_none() && item.template_id != "torch" && item.template_id != "lantern" {
                    return "SYS_MSG:You can't use that item directly.".to_string();
                }
                item_data = Some((
                    item.display_name.clone(),
                    item.effect.clone(),
                    item.template_id.clone(),
                    item.light_radius,
                    item.duration_turns,
                    item.max_duration,
                ));
                item.quantity -= 1;
            }
        }

        if let Some((display_name, effect, template_id, light_radius, duration_turns, _max_duration)) = item_data {
            if template_id == "torch" {
                let radius = light_radius.unwrap_or(3);
                let remaining_turns = duration_turns.unwrap_or(60);
                // If wielding a two-handed weapon, stow it first
                if let Some(ph) = &self.state.player.primary_hand {
                    if self.state.player.inventory.iter()
                        .any(|i| i.instance_id == *ph && i.handedness.as_deref() == Some("TWO_HANDED"))
                    {
                        self.state.player.primary_hand = None;
                    }
                }
                if !is_belt_mounted {
                    // Torch goes to secondary_hand when lit (unless belt-mounted).
                    // If secondary is full, try primary. If both full, block.
                    if self.state.player.secondary_hand.is_some() {
                        if self.state.player.primary_hand.is_none() {
                            self.state.player.primary_hand = Some(item_id.clone());
                        } else {
                            let blocked_msg = if hands_occupied.is_empty() {
                                format!(
                                    "You are the Dungeon Master. The player tries to light a torch but both hands are already full. Narrate the Dungeon Master preventing this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                                     --- ENGINE FACT PACKET ---\n\
                                     EVENT_TYPE: PlayerAction\n\
                                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"USE_ITEM\"\n  target: \"{}\"\n  outcome: \"BLOCKED\"\n\
                                     STATE_DELTAS: (none — action prevented)",
                                    display_name
                                )
                            } else {
                                format!(
                                    "You are the Dungeon Master. The player tries to light a torch but their hands are already occupied by {}. Narrate the Dungeon Master preventing this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                                     --- ENGINE FACT PACKET ---\n\
                                     EVENT_TYPE: PlayerAction\n\
                                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"USE_ITEM\"\n  target: \"{}\"\n  outcome: \"BLOCKED\"\n\
                                     STATE_DELTAS: (none — action prevented)",
                                    hands_occupied, display_name
                                )
                            };
                            return blocked_msg;
                        }
                    } else {
                        self.state.player.secondary_hand = Some(item_id.clone());
                    }
                }
                if let Some(torch) = self.state.player.inventory.iter_mut().find(|i| i.instance_id == item_id) {
                    torch.is_lit = Some(true);
                }
                if let Some(torch) = self.state.player.utility_slots.iter_mut().flatten().find(|i| i.instance_id == item_id) {
                    torch.is_lit = Some(true);
                }
                println!("[DEBUG USE_ITEM] Lighting torch: remaining_turns={}, radius={}, is_belt_mounted={}", remaining_turns, radius, is_belt_mounted);
            self.state.player.active_light_source = Some(state::ActiveLightSource {
                    item_id: "torch".to_string(),
                    radius,
                    remaining_turns,
                    is_belt_mounted,
                });
                self.update_visibility();
                self.state.log_combat(format!("You light the {}. (Radius {}, {} turns)", display_name, radius, remaining_turns));
                fact_packet = format!(
                    "You are the Dungeon Master. The player lit a torch. Narrate the warm glow pushing back the darkness. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: PlayerAction\n\
                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"USE_ITEM\"\n  target: \"{}\"\n  outcome: \"SUCCESS\"\n\
                     STATE_DELTAS:\n  - An active light source now illuminates the area (Radius {}, {} turns).",
                    display_name, radius, remaining_turns
                );
            } else if let Some(eff) = effect {
                if eff.starts_with("HEAL") {
                    let dice_str = eff.trim_start_matches("HEAL_");
                    let heal_amount = roll_dice(dice_str);
                    self.state.player.hp += heal_amount;
                    if self.state.player.hp > self.state.player.max_hp {
                        self.state.player.hp = self.state.player.max_hp;
                    }
                    self.state.last_roll = format!("{} = {} (Healing)", dice_str, heal_amount);

                    fact_packet = format!(
                        "You are the Dungeon Master. The player just used {}. Narrate the healing. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                         --- ENGINE FACT PACKET ---\n\
                         EVENT_TYPE: PlayerAction\n\
                         RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"USE_ITEM\"\n  target: \"{}\"\n  outcome: \"SUCCESS\"\n\
                         DICE_ROLLS:\n  - Healing: {} = {}\n\
                         STATE_DELTAS:\n  - Player HP is now {}.\n",
                        display_name, display_name, dice_str, heal_amount, self.state.player.hp
                    );
                }
            }
        }

        if self.state.game_mode == state::GameMode::Combat {
            self.end_turn("player");
            self.advance_turn();
            if self.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
                self.state.generate_available_actions();
            } else {
                self.state.available_actions.clear();
            }
        } else {
            self.state.generate_available_actions();
        }
        fact_packet
    }

    fn handle_equip_item(&mut self, action_id: &str, slot: &str) -> String {
        let item_id = action_id
            .trim_start_matches("EQUIP_ITEM_PRIMARY_")
            .trim_start_matches("EQUIP_ITEM_SECONDARY_")
            .trim_start_matches("EQUIP_ITEM_")
            .to_string();

        // Extract name before any mutable operation
        let item_name = self.state.player.inventory.iter()
            .find(|i| i.instance_id == item_id)
            .map(|i| i.display_name.clone())
            .unwrap_or_default();

        // Build list of items currently in hands for DM narration
        let hands_occupied = {
            let mut parts: Vec<&str> = Vec::new();
            if let Some(id) = &self.state.player.primary_hand {
                if let Some(item) = self.state.player.inventory.iter().find(|i| &i.instance_id == id) {
                    parts.push(&item.display_name);
                }
            }
            if let Some(id) = &self.state.player.secondary_hand {
                if let Some(item) = self.state.player.inventory.iter().find(|i| &i.instance_id == id) {
                    parts.push(&item.display_name);
                }
            }
            match parts.len() {
                0 => String::new(),
                1 => parts[0].to_string(),
                2 => format!("{} and {}", parts[0], parts[1]),
                _ => parts.join(", "),
            }
        };

        let is_valid = self.state.player.inventory.iter()
            .any(|i| i.instance_id == item_id && (i.item_class == "WEAPON" || i.item_class == "MELEE" || i.item_class == "MAGIC" || i.item_class == "RANGED" || i.handedness.is_some()));

        if is_valid {
            // Two-handed weapon: block if either hand is occupied
            let is_two_handed = self.state.player.inventory.iter()
                .any(|i| i.instance_id == item_id && i.handedness.as_deref() == Some("TWO_HANDED"));
            if is_two_handed && (self.state.player.primary_hand.is_some() || self.state.player.secondary_hand.is_some()) {
                self.state.generate_available_actions();
                let blocked_msg = if hands_occupied.is_empty() {
                    format!(
                        "You are the Dungeon Master. The player tries to equip the {} which requires two hands, but their hands are not free. Narrate the Dungeon Master preventing this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                         --- ENGINE FACT PACKET ---\n\
                         EVENT_TYPE: PlayerAction\n\
                         RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"EQUIP_ITEM\"\n  target: \"{}\"\n  outcome: \"BLOCKED\"\n\
                         STATE_DELTAS: (none — action prevented)",
                        item_name, item_name
                    )
                } else {
                    format!(
                        "You are the Dungeon Master. The player tries to equip the {} which requires two hands, but their hands are already occupied by {}. Narrate the Dungeon Master preventing this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                         --- ENGINE FACT PACKET ---\n\
                         EVENT_TYPE: PlayerAction\n\
                         RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"EQUIP_ITEM\"\n  target: \"{}\"\n  outcome: \"BLOCKED\"\n\
                         STATE_DELTAS: (none — action prevented)",
                        item_name, hands_occupied, item_name
                    )
                };
                return blocked_msg;
            }

            match slot {
                "primary" => {
                    self.state.player.primary_hand = Some(item_id.clone());
                }
                "secondary" => {
                    self.state.player.secondary_hand = Some(item_id.clone());
                }
                _ => {
                    if let Some(msg) = self.state.equip_to_slot(&item_id) {
                        self.state.log_combat(msg);
                    }
                },
            }
        }

        self.state.generate_available_actions();
        format!(
            "You are the Dungeon Master. Narrate the player equipping a new weapon. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"EQUIP_ITEM\"\n  target: \"{}\"\n  outcome: \"SUCCESS\"\n\
             STATE_DELTAS:\n  - Player equipped {} to {} hand.",
            item_name, item_name, if slot == "primary" { "main" } else if slot == "secondary" { "off" } else { "a" }
        )
    }

    fn handle_equip_armour(&mut self, action_id: &str) -> String {
        let item_id = action_id.trim_start_matches("EQUIP_ARMOUR_").to_string();

        let item_idx = self.state.player.inventory.iter().position(|i| i.instance_id == item_id);
        if let Some(idx) = item_idx {
            let item = self.state.player.inventory[idx].clone();
            if item.item_class == "ARMOR" {
                self.state.player.equipped_armour.retain(|a| a.armor_slot != item.armor_slot);
                self.state.player.inventory.remove(idx);
                self.state.player.equipped_armour.push(item.clone());
            }
        }

        self.state.generate_available_actions();
        format!(
            "You are the Dungeon Master. Narrate the player equipping armour. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"EQUIP_ARMOUR\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_unequip_armour(&mut self, action_id: &str) -> String {
        let item_id = action_id.trim_start_matches("UNEQUIP_ARMOUR_").to_string();

        let armour_idx = self.state.player.equipped_armour.iter().position(|a| a.instance_id == item_id);
        if let Some(idx) = armour_idx {
            let item = self.state.player.equipped_armour.remove(idx);
            self.state.player.inventory.push(item.clone());
        }

        self.state.generate_available_actions();
        format!(
            "You are the Dungeon Master. Narrate the player removing armour. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
              RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"UNEQUIP_ARMOUR\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_unequip_hand(&mut self, hand: &str) -> String {
        match hand {
            "primary" => {
                let id = self.state.player.primary_hand.clone();
                self.state.extinguish_light_source(id.as_deref().unwrap_or(""));
                self.state.player.primary_hand.take();
            }
            "secondary" => {
                let id = self.state.player.secondary_hand.clone();
                self.state.extinguish_light_source(id.as_deref().unwrap_or(""));
                self.state.player.secondary_hand.take();
            }
            _ => return "SYS_MSG:Invalid hand.".to_string(),
        }
        self.state.generate_available_actions();
        format!(
            "You are the Dungeon Master. The player stows their weapon. Narrate the motion. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"UNEQUIP_HAND\"\n  target: \"{}\"\n  outcome: \"SUCCESS\"",
            hand
        )
    }

    fn handle_take_item(&mut self, action_id: &str) -> String {
        let item_id = action_id.trim_start_matches("TAKE_ITEM_").to_string();
        let item_name: String;

        // Proximity check (before mutable borrow on room)
        let px = self.state.player.x;
        let py = self.state.player.y;
        let in_range = self.state.get_current_room()
            .and_then(|r| r.loot.iter().find(|i| i.instance_id == item_id))
            .map(|item| {
                item.placed_x.is_some() && item.placed_y.is_some()
                    && (item.placed_x.unwrap() - px).abs() <= 1
                    && (item.placed_y.unwrap() - py).abs() <= 1
            })
            .unwrap_or(false);
        if !in_range {
            return "SYS_MSG:That item is too far away to pick up.".to_string();
        }

        if let Some(room) = self.state.get_current_room_mut() {
            if let Some(pos) = room.loot.iter().position(|i| i.instance_id == item_id) {
                let item = room.loot.remove(pos);
                item_name = item.display_name.clone();
                println!("[TAKE_ITEM] picked up {} (id={}, class={}, qty={})", item.display_name, item.instance_id, item.item_class, item.quantity);
                if state::is_stackable(&item.item_class) {
                    if let Some(existing) = self.state.player.inventory.iter_mut().find(|i| state::stacks_with(i, &item)) {
                        existing.quantity += item.quantity;
                        println!("[TAKE_ITEM] stacked into existing entry, qty now {}", existing.quantity);
                    } else {
                        self.state.player.inventory.push(item.clone());
                        println!("[TAKE_ITEM] pushed as new entry (no matching stack found)");
                    }
                } else {
                    self.state.player.inventory.push(item.clone());
                    println!("[TAKE_ITEM] pushed as new entry (non-stackable)");
                }
            } else {
                item_name = format!("item_{}", item_id);
            }
        } else {
            item_name = format!("item_{}", item_id);
        }

        self.state.generate_available_actions();
        format!(
            "You are the Dungeon Master. The player picked up an item ({}). Narrate this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"TAKE_ITEM\"\n  outcome: \"SUCCESS\"\n  item: \"{}\"",
            item_name, item_name,
        )
    }

    fn handle_pick_up_torch(&mut self, action_id: &str) -> String {
        let coords = action_id.trim_start_matches("PICK_UP_TORCH_");
        let parts: Vec<&str> = coords.splitn(2, '_').collect();
        if parts.len() < 2 { return "SYS_MSG:Invalid pickup coordinates.".to_string(); }
        let tx: i32 = parts[0].parse().unwrap_or(-1);
        let ty: i32 = parts[1].parse().unwrap_or(-1);

        // Clone the ground light source first (immutable borrow)
        let ground_light = {
            let room = self.state.get_current_room();
            room.and_then(|r| {
                if tx >= 0 && tx < r.tile_width && ty >= 0 && ty < r.tile_height {
                    r.tiles[ty as usize][tx as usize].ground_light_source.clone()
                } else {
                    None
                }
            })
        };

        if let Some(light) = ground_light {
            // Stow two-handed weapon if needed (separate mutable borrow scope)
            let primary_was_two_handed = {
                let ph = self.state.player.primary_hand.clone();
                ph.as_ref().is_some_and(|id| self.state.player.inventory.iter()
                    .any(|i| i.instance_id == *id && i.handedness.as_deref() == Some("TWO_HANDED")))
            };
            if primary_was_two_handed {
                self.state.player.primary_hand = None;
            }

            // Find a torch in inventory to equip to secondary_hand
            let torch_instance_id = {
                let primary = self.state.player.primary_hand.clone();
                self.state.player.inventory.iter()
                    .find(|i| i.template_id == "torch" && Some(i.instance_id.clone()) != primary)
                    .map(|i| i.instance_id.clone())
            };

            if let Some(torch_id) = torch_instance_id {
                self.state.player.secondary_hand = Some(torch_id);
            }
            self.state.player.active_light_source = Some(light);
            if let Some(room) = self.state.get_current_room_mut() {
                room.tiles[ty as usize][tx as usize].ground_light_source = None;
            }
            self.update_visibility();
            self.state.log_combat("You pick up the burning torch from the ground.".to_string());
            self.state.generate_available_actions();
            format!(
                "You are the Dungeon Master. Narrate the player picking up a burning torch from the ground. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                 --- ENGINE FACT PACKAGE ---\n\
                 EVENT_TYPE: PlayerAction\n\
                 RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"PICK_UP_TORCH\"\n  outcome: \"SUCCESS\""
            )
        } else {
            "SYS_MSG:No torch found there.".to_string()
        }
    }

    fn handle_refill_lantern(&mut self, action_id: &str) -> String {
        let lantern_id = action_id.trim_start_matches("REFILL_LANTERN_").to_string();

        // Find an oil flask in inventory
        let oil_idx = self.state.player.inventory.iter().position(|i| i.template_id == "oil_flask" && i.quantity > 0);
        let oil_idx = match oil_idx {
            Some(idx) => idx,
            None => return "SYS_MSG:You have no Oil Flasks to refill the lantern.".to_string(),
        };

        let restore = self.state.player.inventory[oil_idx].fuel_restore.unwrap_or(100) as i32;
        // Consume one oil flask
        if self.state.player.inventory[oil_idx].quantity <= 1 {
            self.state.player.inventory.remove(oil_idx);
        } else {
            self.state.player.inventory[oil_idx].quantity -= 1;
        }

        // Find and refill the lantern
        if let Some(lantern) = self.state.player.inventory.iter_mut().find(|i| i.instance_id == lantern_id) {
            let max_dur = lantern.max_duration.unwrap_or(150) as i32;
            let current = lantern.current_fuel.unwrap_or(0);
            let new_fuel = (current + restore).min(max_dur);
            lantern.current_fuel = Some(new_fuel);

            // If lantern is currently lit, update active_light_source.remaining_turns
            if lantern.is_lit.unwrap_or(false) {
                if let Some(light) = &mut self.state.player.active_light_source {
                    if light.item_id == "lantern" {
                        light.remaining_turns = new_fuel as u32;
                    }
                }
            }

            self.state.log_combat(format!("You refill the lantern with oil. Fuel: {} turns.", new_fuel));
        }

        self.state.generate_available_actions();
        format!(
            "You are the Dungeon Master. Narrate the player refilling their lantern with oil. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"REFILL_LANTERN\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_equip_belt(&mut self, action_id: &str) -> String {
        let belt_id = action_id.trim_start_matches("EQUIP_BELT_").to_string();
        let belt_idx = self.state.player.inventory.iter().position(|i| i.instance_id == belt_id);
        let belt_idx = match belt_idx {
            Some(idx) => idx,
            None => return "SYS_MSG:Item not found.".to_string(),
        };

        let belt = self.state.player.inventory[belt_idx].clone();
        if belt.item_class != "BELT" {
            return "SYS_MSG:That item is not a belt.".to_string();
        }

        // Unequip current belt first, move items back to inventory
        if let Some(old_belt) = self.state.player.equipped_belt.take() {
            let mut items_to_return: Vec<state::ItemInstance> = self.state.player.utility_slots.drain(..).flatten().collect();
            // Extinguish any lit items being returned to inventory
            for item in &mut items_to_return {
                if item.is_lit.unwrap_or(false) {
                    println!("[DEBUG EQUIP_BELT] Replacing belt — extinguishing lit item {}", item.template_id);
                    self.state.player.active_light_source = None;
                    item.is_lit = Some(false);
                }
            }
            self.state.player.inventory.push(old_belt);
            self.state.player.inventory.append(&mut items_to_return);
        }

        self.state.player.inventory.remove(belt_idx);
        let slots = belt.tier.map(|t| match t { 4 => 3, 3 => 2, _ => 1 }).unwrap_or(0);
        let mut utility_slots = Vec::new();
        for _ in 0..slots {
            utility_slots.push(None);
        }
        self.state.player.equipped_belt = Some(belt);
        self.state.player.utility_slots = utility_slots;

        self.state.log_combat("You equip the belt.".to_string());
        self.state.generate_available_actions();
        format!(
            "You are the Dungeon Master. Narrate the player equipping a belt. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"EQUIP_BELT\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_unequip_belt(&mut self) -> String {
        let belt = match self.state.player.equipped_belt.take() {
            Some(b) => b,
            None => return "SYS_MSG:No belt equipped.".to_string(),
        };

        let mut items_to_return: Vec<state::ItemInstance> = self.state.player.utility_slots.drain(..).flatten().collect();
        // Extinguish any lit items being returned to inventory
        for item in &mut items_to_return {
            if item.is_lit.unwrap_or(false) {
                println!("[DEBUG UNEQUIP_BELT] Unequipping belt — extinguishing lit item {}", item.template_id);
                self.state.player.active_light_source = None;
                item.is_lit = Some(false);
            }
        }
        self.state.player.inventory.push(belt);
        self.state.player.inventory.append(&mut items_to_return);

        self.state.log_combat("You unequip the belt.".to_string());
        self.state.generate_available_actions();
        format!(
            "You are the Dungeon Master. Narrate the player unequipping a belt. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"UNEQUIP_BELT\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_mount_utility(&mut self, action_id: &str) -> String {
        let rest = action_id.trim_start_matches("MOUNT_UTILITY_");
        let parts: Vec<&str> = rest.splitn(2, '_').collect();
        if parts.len() != 2 {
            return "SYS_MSG:Invalid action format.".to_string();
        }
        let slot_idx: usize = match parts[0].parse() {
            Ok(i) => i,
            Err(_) => return "SYS_MSG:Invalid slot index.".to_string(),
        };
        let item_id = parts[1].to_string();

        // Auto-equip belt from inventory if not already equipped
        if self.state.player.equipped_belt.is_none() {
            let belt_idx = self.state.player.inventory.iter().position(|i| i.item_class == "BELT" && i.quantity > 0);
            if let Some(bi) = belt_idx {
                let belt = self.state.player.inventory[bi].clone();
                self.state.player.inventory.remove(bi);
                let slots = belt.tier.map(|t| match t { 4 => 3, 3 => 2, _ => 1 }).unwrap_or(0);
                let mut utility_slots = Vec::new();
                for _ in 0..slots {
                    utility_slots.push(None);
                }
                self.state.player.equipped_belt = Some(belt);
                self.state.player.utility_slots = utility_slots;
                self.state.log_combat("You equip the belt.".to_string());
            }
        }

        // Validate slot exists
        if slot_idx >= self.state.player.utility_slots.len() {
            return "SYS_MSG:Invalid utility slot.".to_string();
        }
        if self.state.player.utility_slots[slot_idx].is_some() {
            return "SYS_MSG:That utility slot is already occupied.".to_string();
        }

        let item_idx = self.state.player.inventory.iter().position(|i| i.instance_id == item_id);
        let item_idx = match item_idx {
            Some(idx) => idx,
            None => return "SYS_MSG:Item not found in inventory.".to_string(),
        };

        // Clear hand slots if the item is equipped there (mounting from hand)
        if self.state.player.primary_hand.as_deref() == Some(&item_id) {
            self.state.player.primary_hand = None;
        }
        if self.state.player.secondary_hand.as_deref() == Some(&item_id) {
            self.state.player.secondary_hand = None;
        }

        let mut item = self.state.player.inventory.remove(item_idx);

        // If this is the active light source, update is_belt_mounted
        let is_light_source = item.template_id == "torch" || item.template_id == "lantern";
        if is_light_source {
            if let Some(ref mut light) = self.state.player.active_light_source {
                let light_type_matches = (light.item_id == "torch" && item.template_id == "torch")
                    || (light.item_id == "lantern" && item.template_id == "lantern");
                if light_type_matches && light.remaining_turns > 0 {
                    light.is_belt_mounted = true;
                    item.is_lit = Some(true);
                }
            }
        }

        self.state.player.utility_slots[slot_idx] = Some(item);
        self.update_visibility();

        self.state.log_combat(format!("You mount the item to utility slot {}.", slot_idx + 1));
        self.state.generate_available_actions();
        format!(
            "You are the Dungeon Master. Narrate the player mounting an item to their belt. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"MOUNT_UTILITY\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_unmount_utility(&mut self, action_id: &str) -> String {
        let slot_idx: usize = match action_id.trim_start_matches("UNMOUNT_UTILITY_").parse() {
            Ok(i) => i,
            Err(_) => return "SYS_MSG:Invalid slot index.".to_string(),
        };

        if slot_idx >= self.state.player.utility_slots.len() {
            return "SYS_MSG:Invalid utility slot.".to_string();
        }

        let item = match self.state.player.utility_slots[slot_idx].take() {
            Some(i) => i,
            None => return "SYS_MSG:That utility slot is empty.".to_string(),
        };

        // If this was the active light source, update is_belt_mounted
        let is_light_source = item.template_id == "torch" || item.template_id == "lantern";
        if is_light_source {
            if let Some(ref mut light) = self.state.player.active_light_source {
                let light_type_matches = (light.item_id == "torch" && item.template_id == "torch")
                    || (light.item_id == "lantern" && item.template_id == "lantern");
                if light_type_matches {
                    light.is_belt_mounted = false;
                }
            }
        }

        self.state.player.inventory.push(item);
        self.update_visibility();
        self.state.log_combat(format!("You unmount the item from utility slot {}.", slot_idx + 1));
        self.state.generate_available_actions();
        format!(
            "You are the Dungeon Master. Narrate the player unmounting an item from their belt. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerAction\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"UNMOUNT_UTILITY\"\n  outcome: \"SUCCESS\""
        )
    }

    fn handle_search(&mut self) -> String {
        let mut rng = rand::thread_rng();
        let roll = rng.gen_range(1..=20);
        let wis_mod = ability_modifier(self.state.player.wisdom);
        let total = roll + wis_mod;
        let dc = 12;

        let outcome = if total >= dc { "SUCCESS" } else { "FAILURE" };
        self.state.last_roll = format!("d20+{} = {} ({}) vs DC {}", wis_mod, total, outcome, dc);

        let mut fact_packet = format!(
            "You are the Dungeon Master. Narrate the following engine-resolved event. \
             Do NOT roll dice yourself. Respond ONLY with a JSON object using this exact schema:\n\
             {{\n  \"narration\": \"Your vivid prose here...\",\n  \"commands\": [\n    {{ \"type\": \"AUDIO_CUE\", \"cue\": \"footsteps\" }}\n  ]\n}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: PlayerExploration\n\
             RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"SEARCH_AREA\"\n  outcome: \"{}\"\n\
             DICE_ROLLS:\n  - reason: \"Perception Check (Wis)\", roll: \"d20+{}\", natural: {}, total: {}, outcome: \"{}\"",
            outcome, wis_mod, roll, total, outcome
        );

        if outcome == "SUCCESS" {
            let found_item: Option<state::ItemInstance> = {
                if let Some(room) = self.state.get_current_room_mut() {
                    if !room.hidden_caches.is_empty() {
                        Some(room.hidden_caches.remove(0))
                    } else if room.chests.iter().any(|c| !c.broken) {
                        fact_packet.push_str("\nDISCOVERY: Player spots a chest in the room.");
                        None
                    } else {
                        fact_packet.push_str("\nDISCOVERY: Player finds nothing of value.");
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(found) = found_item {
                let item_name = found.display_name.clone();
                state::add_to_inventory(&mut self.state.player.inventory, found);
                fact_packet.push_str(&format!("\nDISCOVERY: Player found hidden loot: {}!", item_name));
                fact_packet.push_str(&format!("\nLOOT_ACQUIRED: {}!", item_name));
            }
        }

        self.state.generate_available_actions();
        fact_packet
    }

    fn handle_open_chest(&mut self, action_id: &str) -> String {
        let chest_id = action_id.trim_start_matches("OPEN_CHEST_").to_string();
        let mut fact_packet = String::new();
        let mut outcome = String::new();
        let mut chest_name = String::new();

        let loot_items: Vec<state::ItemInstance> = {
            if let Some(room) = self.state.get_current_room_mut() {
                if let Some(chest) = room.chests.iter_mut().find(|c| c.id == chest_id) {
                    chest_name = chest.name.clone();
                    if chest.broken {
                        outcome = "BROKEN".to_string();
                        vec![]
                    } else {
                        outcome = "SUCCESS".to_string();
                        let items: Vec<state::ItemInstance> = chest.loot.drain(..).collect();
                        chest.broken = true;
                        items
                    }
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        };

        if outcome == "BROKEN" {
            fact_packet = format!(
                "You are the Dungeon Master. The chest is broken and inaccessible. Narrate this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                 --- ENGINE FACT PACKET ---\n\
                 EVENT_TYPE: PlayerAction\n\
                 RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"OPEN_CHEST\"\n  target: \"{}\"\n  outcome: \"BROKEN\"",
                chest_name
            );
        } else {
            for item in &loot_items {
                let item_name = item.display_name.clone();
                state::add_to_inventory(&mut self.state.player.inventory, item.clone());
                fact_packet.push_str(&format!("\nLOOT_ACQUIRED: {}!", item_name));
            }
            let base = fact_packet.clone();
            fact_packet = format!(
                "You are the Dungeon Master. The player opened a chest and found loot. Narrate this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                 --- ENGINE FACT PACKET ---\n\
                 EVENT_TYPE: PlayerAction\n\
                 RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"OPEN_CHEST\"\n  target: \"{}\"\n  outcome: \"SUCCESS\"{}",
                chest_name, base
            );
        }

        self.state.generate_available_actions();
        fact_packet
    }

    fn handle_pick_lock(&mut self, action_id: &str) -> String {
        let chest_id = action_id.trim_start_matches("PICK_LOCK_").to_string();
        let mut rng = rand::thread_rng();

        let has_tools = self.state.player.inventory.iter()
            .any(|item| item.template_id == "thieves_tools" && item.item_class == "TOOL");
        let is_proficient = self.state.player.thieves_tools_proficiency;
        let dex_mod = ability_modifier(self.state.player.dexterity);

        let (tool_bonus, tool_description) = if !has_tools {
            (0, "no thieves' tools".to_string())
        } else if !is_proficient {
            (0, "tools but no proficiency".to_string())
        } else {
            let quality_bonus = self.state.player.inventory.iter()
                .find(|item| item.template_id == "thieves_tools" && item.item_class == "TOOL")
                .map_or(0, |tool| crate::engine::procedural::get_tool_quality_bonus(tool.rarity));
            let total = self.state.player.proficiency_bonus + quality_bonus;
            (total, format!("tools + proficiency +{} quality", quality_bonus))
        };

        let can_attempt = has_tools;
        let pick_roll = if can_attempt {
            rng.gen_range(1..=20) + dex_mod + tool_bonus
        } else {
            0
        };

        let mut fact_packet = String::new();
        let mut outcome = String::new();
        let mut chest_name = String::new();
        let mut chest_dc = 10;

        let (loot_items, break_chance_now) = {
            if let Some(room) = self.state.get_current_room_mut() {
                if let Some(chest) = room.chests.iter_mut().find(|c| c.id == chest_id) {
                    chest_name = chest.name.clone();
                    chest_dc = chest.dc;
                    if chest.broken {
                        outcome = "BROKEN".to_string();
                        (vec![], 0)
                    } else if !can_attempt {
                        outcome = "NO_TOOLS".to_string();
                        (vec![], 0)
                    } else {
                        let break_roll = rng.gen_range(1..=100);
                        if break_roll <= chest.break_chance {
                            chest.broken = true;
                            outcome = "BROKEN_LOCK".to_string();
                            (vec![], chest.break_chance)
                        } else if pick_roll >= chest.dc {
                            chest.locked = false;
                            let items: Vec<state::ItemInstance> = chest.loot.drain(..).collect();
                            chest.broken = true;
                            outcome = "SUCCESS".to_string();
                            (items, 0)
                        } else {
                            chest.break_chance = (chest.break_chance + 20).min(90);
                            outcome = "FAILURE".to_string();
                            (vec![], chest.break_chance)
                        }
                    }
                } else {
                    outcome = "NO_CHEST".to_string();
                    (vec![], 0)
                }
            } else {
                outcome = "NO_ROOM".to_string();
                (vec![], 0)
            }
        };

        match outcome.as_str() {
            "BROKEN" => {
                fact_packet = format!(
                    "You are the Dungeon Master. The chest is broken. Narrate this. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: PlayerAction\n\
                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"PICK_LOCK\"\n  target: \"{}\"\n  outcome: \"BROKEN\"",
                    chest_name
                );
            }
            "NO_TOOLS" => {
                fact_packet = format!("SYS_MSG:You don't have thieves' tools to pick the lock on {}. You'll need to find some first.", chest_name);
            }
            "BROKEN_LOCK" => {
                self.state.last_roll = format!("Lockpicks break! (Break chance: {}%)", break_chance_now);
                fact_packet = format!(
                    "You are the Dungeon Master. The player's tools snap in the lock, jamming it permanently. Narrate. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: PlayerAction\n\
                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"PICK_LOCK\"\n  target: \"{}\"\n  outcome: \"BROKEN_LOCK\"\n\
                     DICE_ROLLS:\n  - Break chance d100 <= {} (LOCK BROKEN)",
                    chest_name, break_chance_now
                );
            }
            "SUCCESS" => {
                let total_bonus = if has_tools { dex_mod + tool_bonus } else { 0 };
                self.state.last_roll = format!("d20+{} = {} vs DC {} (SUCCESS, {})", total_bonus, pick_roll, chest_dc, tool_description);
                fact_packet = format!(
                    "You are the Dungeon Master. The player successfully picked the lock. Narrate. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: PlayerAction\n\
                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"PICK_LOCK\"\n  target: \"{}\"\n  outcome: \"SUCCESS\"\n\
                     DICE_ROLLS:\n  - Lockpick (DEX): d20+{} = {} vs DC {} (SUCCESS)",
                    chest_name, total_bonus, pick_roll, chest_dc
                );
                for item in &loot_items {
                    let item_name = item.display_name.clone();
                    state::add_to_inventory(&mut self.state.player.inventory, item.clone());
                    fact_packet.push_str(&format!("\nLOOT_ACQUIRED: {}!", item_name));
                }
            }
            "FAILURE" => {
                let total_bonus = if has_tools { dex_mod + tool_bonus } else { 0 };
                self.state.last_roll = format!("d20+{} = {} vs DC {} (FAILURE, {}). Break chance now {}%", total_bonus, pick_roll, chest_dc, tool_description, break_chance_now);
                fact_packet = format!(
                    "You are the Dungeon Master. The player failed to pick the lock. Narrate. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
                     --- ENGINE FACT PACKET ---\n\
                     EVENT_TYPE: PlayerAction\n\
                     RESOLVED_ACTION:\n  actor: \"player\"\n  action: \"PICK_LOCK\"\n  target: \"{}\"\n  outcome: \"FAILURE\"\n\
                     DICE_ROLLS:\n  - Lockpick (DEX): d20+{} = {} vs DC {} (FAILURE)\n\
                     STATE_DELTAS:\n  - Break chance increased to {}%",
                    chest_name, total_bonus, pick_roll, chest_dc, break_chance_now
                );
            }
            _ => {}
        }

        self.state.generate_available_actions();
        fact_packet
    }

    // ─── Loot ────────────────────────────────────────────────────────

    fn generate_loot_for_enemy(&self, enemy: &state::Enemy) -> Vec<state::ItemInstance> {
        let mut rng = rand::thread_rng();
        let mut items = Vec::new();
        for drop in &enemy.loot_table {
            let drop_roll = rng.gen_range(1..=100);
            if drop_roll <= drop.drop_chance {
                if let Some(item) = procedural::generate_item_instance(&self.campaign, &drop.item_class, drop.rarity_min, drop.rarity_max, None) {
                    items.push(item);
                }
            }
        }
        for armour in &enemy.equipped_armour {
            items.push(armour.clone());
        }
        items
    }

    pub fn process_loot_for_dead_enemies(&mut self) {
        let mut rng = rand::thread_rng();
        let dead_data: Vec<(String, i32, Vec<(String, i32, i32, i32)>, Vec<state::ItemInstance>)> = {
            if let Some(room) = self.state.get_current_room_mut() {
                room.enemies.iter().filter(|e| e.hp <= 0).map(|enemy| {
                    let loot_table = enemy.loot_table.clone();
                    let armour: Vec<state::ItemInstance> = enemy.equipped_armour.clone();
                    (enemy.name.clone(), enemy.xp, loot_table.iter().map(|d| (d.item_class.clone(), d.drop_chance, d.rarity_min, d.rarity_max)).collect::<Vec<_>>(), armour)
                }).collect()
            } else {
                Vec::new()
            }
        };
        for (name, xp, drops, armour_pieces) in dead_data {
            let mut items: Vec<state::ItemInstance> = Vec::new();
            for (item_class, drop_chance, r_min, r_max) in &drops {
                let drop_roll = rng.gen_range(1..=100);
                if drop_roll <= *drop_chance {
                    if let Some(item) = procedural::generate_item_instance(&self.campaign, item_class, *r_min, *r_max, None) {
                        items.push(item);
                    }
                }
            }
            for a in armour_pieces {
                items.push(a);
            }
            let gp = (xp / 5).max(1);
            for item in &items {
                state::add_to_inventory(&mut self.state.player.inventory, item.clone());
            }
            self.state.player.gp += gp;
            self.state.last_loot.push(state::LootGroup {
                source_name: name,
                gp,
                items,
            });
        }
    }

    pub fn build_loot_packet(&mut self) -> String {
        if self.state.last_loot.is_empty() {
            return String::new();
        }
        let loot_desc: Vec<String> = self.state.last_loot.iter().map(|group| {
            let mut desc = format!("From {}:", group.source_name);
            if group.gp > 0 {
                desc.push_str(&format!("\n  - {} gp", group.gp));
            }
            for item in &group.items {
                desc.push_str(&format!("\n  - {} ({} gp)", item.display_name, item.gp_value));
            }
            desc
        }).collect();
        self.state.last_loot.clear();
        format!(
            "You are the Dungeon Master. Narrate the player collecting loot after battle. \
             Describe finding and gathering the spoils. Keep it brief (1-2 sentences). \
             Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: LootCollection\n\
             LOOT:\n{}\n",
            loot_desc.join("\n")
        )
    }

    /// Roll 50% recovery per unit of ammo consumed during the just-ended combat,
    /// dropping any recovered ammo into the room where the fight happened.
    fn recover_ammo_from_combat(&mut self, room_id: &str) {
        if self.state.ammo_consumed_this_combat.is_empty() {
            return;
        }
        let mut rng = rand::thread_rng();
        let consumed = std::mem::take(&mut self.state.ammo_consumed_this_combat);
        let px = self.state.player.x;
        let py = self.state.player.y;

        for (ammo_type, count) in consumed {
            let recovered = (0..count).filter(|_| rng.gen_bool(0.5)).count() as i32;
            if recovered <= 0 {
                continue;
            }

            let template_id = self.campaign.items.base_items.iter()
                .find(|(_, item)| item.item_class == "AMMO" && item.ammo_type.as_deref() == Some(ammo_type.as_str()))
                .map(|(id, _)| id.clone());

            if let Some(template_id) = template_id {
                if let Some(mut item) = procedural::generate_item_instance(&self.campaign, "AMMO", 2, 2, Some(&template_id)) {
                    item.quantity = recovered;
                    item.placed_x = Some(px);
                    item.placed_y = Some(py);
                    if let Some(room) = self.state.rooms.iter_mut().find(|r| r.id == room_id) {
                        room.loot.push(item);
                    }
                }
            }
        }
    }

    pub fn build_outcome_packet(&self) -> String {
        let event_type = if self.state.game_mode == state::GameMode::Combat {
            "PlayerCombatAction"
        } else {
            "PlayerAction"
        };
        format!(
            "You are the Dungeon Master. The player initiated an action, and the Engine has resolved the mechanics.\n\
             Narrate ONLY the outcome of the player's action based on the fact packet. Do NOT roll dice.\n\
             Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n\
             --- ENGINE FACT PACKET ---\n\
             EVENT_TYPE: {}\n\
             RESOLVED_ACTION:\n  {}\n",
            event_type, self.state.last_combat_event
        )
    }

    // ─── Free Text ───────────────────────────────────────────────────

    pub fn handle_free_text(&mut self, user_input: &str) -> String {
        self.state.last_combat_event = "".to_string();

        let inventory_str = self.state.player.inventory.iter()
            .map(|i| format!("{} (x{}, ID: {})", i.display_name, i.quantity, i.instance_id))
            .collect::<Vec<_>>().join(", ");
            
        let equipped_weapon_str = {
            if let Some(w) = self.state.get_equipped_weapon() {
                format!("{} (Class: {}, Damage: {}{}, ID: {})", w.display_name, w.item_class, w.damage_dice.clone().unwrap_or_default(), if w.damage_bonus.unwrap_or(0) >= 0 { format!("+{}", w.damage_bonus.unwrap()) } else { format!("{}", w.damage_bonus.unwrap()) }, w.instance_id)
            } else {
                "Unarmed".to_string()
            }
        };
            
        let room_items_str = self.state.get_current_room()
            .map(|r| {
                if !r.loot.is_empty() {
                    r.loot.iter().map(|i| format!("{} (ID: {})", i.display_name, i.instance_id)).collect::<Vec<_>>().join(", ")
                } else {
                    "None".to_string()
                }
            })
            .unwrap_or("None".to_string());
            
        let enemies_str = self.state.get_current_room()
            .map(|r| r.enemies.iter().filter(|e| e.hp > 0).map(|e| format!("{} (ID: {}, HP: {})", e.name, e.id, e.hp)).collect::<Vec<_>>().join(", "))
            .unwrap_or("None".to_string());

        let mode_str = match self.state.game_mode {
            state::GameMode::Combat => format!(
                "COMBAT - Round {}. Initiative: {:?}. It is {}'s turn.\n\
                 You are in a combat encounter. If the player takes an action that would trigger combat mechanics (attack, dodge, disengage, etc.), \
                 issue the appropriate command. Otherwise, narrate their intent.\n\
                 If attacking, issue: {{\"type\": \"ATTACK\", \"target\": \"<enemy_id>\"}}.\n",
                self.state.round_number,
                self.state.initiative_entries.iter().map(|e| format!("{} ({})", e.name, e.roll)).collect::<Vec<_>>(),
                self.state.get_current_turn_id().cloned().unwrap_or_default()
            ),
            state::GameMode::Exploration => "EXPLORATION".to_string(),
            state::GameMode::GameOver => "GAME_OVER".to_string(),
        };

        format!(
            "You are the Dungeon Master. The player typed the following free-text action:\n\
             \"{}\"\n\n\
             --- LORE CONTEXT ---\n{}\n\n\
             --- STRICT CONTEXT (You can ONLY interact with things in this list) ---\n\
             Light Source: {}\n\
             Player Inventory: [{}]\n\
             Equipped Weapon: {}\n\
             Visible Items in Room: [{}]\n\
             Alive Enemies: [{}]\n\
             Mode: {}\n\n\
              IMPORTANT RULES:\n\
              1. You CANNOT narrate success for an action that requires mechanics. You MUST issue a command.\n\
              2. If the player tries to pick up an item, issue: {{ \"type\": \"ADD_ITEM\", \"container\": \"player\", \"item_id\": \"<item_id>\", \"quantity\": 1 }}\n\
              3. If the player tries to use an item, issue: {{ \"type\": \"USE_ITEM\", \"target\": \"player\", \"item_id\": \"<item_id>\" }}\n\
              4. If the player tries to equip a weapon, issue: {{ \"type\": \"EQUIP_ITEM\", \"target\": \"player\", \"item_id\": \"<item_id>\" }}\n\
              5. If the player tries to ATTACK, issue: {{ \"type\": \"ATTACK\", \"target\": \"<enemy_id>\" }}. CRITICAL: Your narration MUST only describe the player's INTENT or PREPARATION for the attack. Do NOT narrate hit, miss, or damage. The Engine will resolve that.\n\
              6. If the player tries to dodge, disengage, hide, or study an enemy in combat, tell them to use the available action buttons.\n\
              7. LEGALITY CHECK: If the player tries to cast a spell, check the Equipped Weapon Class. If it is NOT MAGIC, narrate failure.\n\
              8. If the player tries to interact with something NOT in STRICT CONTEXT, narrate that they cannot find it.\n\
              9. The \"commands\" array must contain ONLY valid JSON objects (e.g., {{\"type\": \"ATTACK\"}}).\n\
              10. Do NOT issue commands for \"end_combat\", \"loot\", or \"enemy_death\". The Engine handles these.\n\
              Do NOT roll dice. Respond ONLY with JSON: {{\"narration\": \"...\", \"commands\": []}}\n\n",
            user_input, self.state.lore_context, self.light_source_status(), inventory_str, equipped_weapon_str, room_items_str, enemies_str, mode_str
        )
    }
}
