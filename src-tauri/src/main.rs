#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod llm;
mod campaign;

use engine::GameEngine;
use llm::{gemma::GemmaEngine, mock::MockLlm, LlmManager, LlmProvider};
use campaign::loader::load_campaign;
use campaign::schema::CampaignData;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use std::fs;

pub struct AppState {
    pub engine: Mutex<GameEngine>,
    pub llm: Arc<LlmManager>,
    pub campaign_dir: Mutex<String>,
}

#[derive(serde::Serialize)]
pub struct CampaignInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[tauri::command]
fn list_campaigns() -> Vec<CampaignInfo> {
    let campaigns_dir = Path::new("campaigns");
    let mut campaigns = Vec::new();
    if let Ok(entries) = fs::read_dir(campaigns_dir) {
        for entry in entries.flatten() {
            let dir_path = entry.path();
            if !dir_path.is_dir() { continue; }
            let main_path = dir_path.join("main.json");
            if !main_path.exists() { continue; }
            if let Ok(content) = fs::read_to_string(&main_path) {
                if let Ok(main) = serde_json::from_str::<campaign::schema::CampaignMain>(&content) {
                    campaigns.push(CampaignInfo {
                        id: main.campaign_id.clone(),
                        name: main.campaign_name.clone(),
                        description: main.description.clone(),
                    });
                }
            }
        }
    }
    campaigns
}

#[tauri::command]
fn save_game(state: tauri::State<AppState>) -> Result<(), String> {
    let engine = state.engine.lock().map_err(|e| e.to_string())?;
    let save_path = Path::new("save_game.json");
    let json = serde_json::to_string_pretty(&engine.state).map_err(|e| e.to_string())?;
    fs::write(save_path, json).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_save_exists() -> bool {
    Path::new("save_game.json").exists()
}

#[tauri::command]
fn delete_save() -> Result<(), String> {
    let save_path = Path::new("save_game.json");
    if save_path.exists() {
        fs::remove_file(save_path).map_err(|e| e.to_string())
    } else {
        Ok(())
    }
}

#[tauri::command]
fn get_game_state(state: tauri::State<AppState>) -> engine::state::SessionState {
    state.engine.lock().unwrap().state.clone()
}

async fn stream_llm_narration(app: tauri::AppHandle, fact_packet: String) {
    let llm = {
        let app_state = app.state::<AppState>();
        let llm_manager = app_state.llm.clone();
        let x = llm_manager.active_engine.read().await.clone();
        x
    };

    let _ = app.emit("llm-start", ());

    match llm.generate_response(app.clone(), fact_packet).await {
        Ok(response) => {
            println!("✅ Gemma generated narration: {}", response.narration);
            
            let app_state = app.state::<AppState>();
            let mut engine = app_state.engine.lock().unwrap();
            engine.process_commands(&response);
        }
        Err(e) => {
            eprintln!("LLM Error: {}", e);
            let _ = app.emit("llm-done", ());
        }
    }
}

#[tauri::command]
async fn generate_narration(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    prompt: String,
) -> Result<(), String> {
    let fact_packet = {
        let mut engine = state.engine.lock().unwrap();
        engine.handle_free_text(&prompt)
    };

    let llm = state.llm.active_engine.read().await.clone();
    let app_handle = app.clone();
    
    tokio::spawn(async move {
        let _ = app_handle.emit("llm-start", ());
        
        match llm.generate_response(app_handle.clone(), fact_packet).await {
            Ok(response) => {
                println!("✅ Gemma generated narration: {}", response.narration);
                
                let action_triggered = {
                    let app_state = app_handle.state::<AppState>();
                    let mut engine = app_state.engine.lock().unwrap();
                    engine.process_commands(&response);
                    engine.process_loot_for_dead_enemies();
                    
                    // FIX: If the LLM issued an attack, we must advance the turn!
                    let mut is_combat_action = false;
                    for cmd_value in &response.commands {
                        if let Ok(cmd) = serde_json::from_value::<engine::commands::Command>(cmd_value.clone()) {
                            if matches!(cmd, engine::commands::Command::Attack { .. } | engine::commands::Command::UseItem { .. }) {
                                is_combat_action = true;
                                break;
                            }
                        }
                    }

                    // Handle DM_Choose commands: emit event so the frontend clicks the button
                    for cmd_value in &response.commands {
                        if let Ok(cmd) = serde_json::from_value::<engine::commands::Command>(cmd_value.clone()) {
                            if let engine::commands::Command::DmChoose { action_id } = cmd {
                                let _ = app_handle.emit("dm-choose", &action_id);
                            }
                        }
                    }
                    
                    if is_combat_action && engine.state.game_mode == engine::state::GameMode::Combat {
                        engine.advance_turn();
                        // If advancing put us back on the player (e.g., only 1 enemy), generate actions
                        if engine.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false) {
                            let campaign = engine.campaign.clone();
                            engine.state.generate_available_actions(&campaign);
                        } else {
                            engine.state.available_actions.clear(); // Enemy's turn, lock UI
                        }
                    }
                    
                    !engine.state.last_combat_event.is_empty()
                };
                
                if action_triggered {
                    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                    
                    let (outcome_packet, roll_snap) = {
                        let app_state = app_handle.state::<AppState>();
                        let engine = app_state.engine.lock().unwrap();
                        (engine.build_outcome_packet(), engine.state.last_roll.clone())
                    };
                    
                    stream_llm_narration(app_handle.clone(), outcome_packet).await;
                    if roll_snap != "None" && !roll_snap.is_empty() {
                        let _ = app_handle.emit("dice-rolled", &roll_snap);
                    }
                    
                    // Emit state AFTER narration
                    {
                        let app_state = app_handle.state::<AppState>();
                        let engine = app_state.engine.lock().unwrap();
                        let _ = app_handle.emit("state-updated", &engine.state);
                    }
                    
                    // Enemy turn loop
                    loop {
                        let enemy_packet_data = {
                            let app_state = app_handle.state::<AppState>();
                            let mut engine = app_state.engine.lock().unwrap();
                            
                            if let Some(room) = engine.state.get_current_room_mut() {
                                room.enemies.retain(|e| e.hp > 0);
                            }
                            engine.update_visibility();
                            engine.check_combat_state();
                            
                            if engine.state.game_mode == engine::state::GameMode::Combat {
                                let current_id = engine.state.get_current_turn_id().cloned();
                                if current_id.is_some() && current_id.as_deref() != Some("player") {
                                    let packet = engine.handle_enemy_turn(current_id.as_ref().unwrap());
                                    let roll = engine.state.last_roll.clone();
                                    Some((packet, roll))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        };
                        
                        if let Some((packet, roll_snap)) = enemy_packet_data {
                            if !packet.is_empty() {
                                stream_llm_narration(app_handle.clone(), packet).await;
                                tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                            }
                            if roll_snap != "None" && !roll_snap.is_empty() {
                                let _ = app_handle.emit("dice-rolled", &roll_snap);
                            }
                            
                            let next_is_player = {
                                let app_state = app_handle.state::<AppState>();
                                let mut engine = app_state.engine.lock().unwrap();
                                engine.advance_turn();
                                engine.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false)
                            };
                            
                            if next_is_player {
                                let app_state = app_handle.state::<AppState>();
                                let mut engine = app_state.engine.lock().unwrap();
                                let campaign = engine.campaign.clone();
                                engine.state.generate_available_actions(&campaign);
                                let _ = app_handle.emit("state-updated", &engine.state);
                                break;
                            } else {
                                let app_state = app_handle.state::<AppState>();
                                let engine = app_state.engine.lock().unwrap();
                                let _ = app_handle.emit("state-updated", &engine.state);
                            }
                        } else {
                            break;
                        }
                    }
                    
                    // Separate loot narration after combat ends
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let loot_packet = {
                        let app_state = app_handle.state::<AppState>();
                        let mut engine = app_state.engine.lock().unwrap();
                        engine.build_loot_packet()
                    };
                    if !loot_packet.is_empty() {
                        stream_llm_narration(app_handle.clone(), loot_packet).await;
                        let app_state = app_handle.state::<AppState>();
                        let engine = app_state.engine.lock().unwrap();
                        let _ = app_handle.emit("state-updated", &engine.state);
                    }
                } else {
                    let app_state = app_handle.state::<AppState>();
                    let engine = app_state.engine.lock().unwrap();
                    let _ = app_handle.emit("state-updated", &engine.state);
                }
            }
            Err(e) => {
                eprintln!("LLM Error: {}", e);
                let _ = app_handle.emit("llm-done", ());
            }
        }
    });
    
    Ok(())
}

#[tauri::command]
async fn player_button_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    action_id: String,
) -> Result<(), String> {
    let (fact_packet, roll_snap) = {
        let mut engine = state.engine.lock().unwrap();
        let packet = engine.handle_button_action(&action_id);
        let roll = engine.state.last_roll.clone();
        (packet, roll)
    };

    // Emit state immediately so the frontend reflects HP/status changes right away
    {
        let engine = state.engine.lock().unwrap();
        let _ = app.emit("state-updated", &engine.state);
    }

    let app_handle = app.clone();

    tokio::spawn(async move {
        if fact_packet == "SYS_MSG:_SILENT" {
            // Silent state update: no LLM narration, no system message
        } else if fact_packet.starts_with("SYS_MSG:") {
            let msg = fact_packet.trim_start_matches("SYS_MSG:");
            let _ = app_handle.emit("system-message", msg);
        } else {
            stream_llm_narration(app_handle.clone(), fact_packet).await;
        }
        if roll_snap != "None" && !roll_snap.is_empty() {
            let _ = app_handle.emit("dice-rolled", &roll_snap);
        }

        // Narrate any pending loot before emitting state or running enemy turns
        let pending_loot = {
            let app_state = app_handle.state::<AppState>();
            let mut engine = app_state.engine.lock().unwrap();
            engine.build_loot_packet()
        };
        if !pending_loot.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
            stream_llm_narration(app_handle.clone(), pending_loot).await;
        }

        // Run enemy-turn loop if combat is active and it's not the player's turn
        let needs_enemy_turns = {
            let app_state = app_handle.state::<AppState>();
            let engine = app_state.engine.lock().unwrap();
            engine.state.game_mode == engine::state::GameMode::Combat
                && engine.state.get_current_turn_id().map(|id| id != "player").unwrap_or(false)
        };
        
        if needs_enemy_turns {
            loop {
                let enemy_packet_data = {
                    let app_state = app_handle.state::<AppState>();
                    let mut engine = app_state.engine.lock().unwrap();
                    
                    if let Some(room) = engine.state.get_current_room_mut() {
                        room.enemies.retain(|e| e.hp > 0);
                    }
                    engine.update_visibility();
                    engine.check_combat_state();
                    
                    if engine.state.game_mode == engine::state::GameMode::Combat {
                        let current_id = engine.state.get_current_turn_id().cloned();
                        if current_id.is_some() && current_id.as_deref() != Some("player") {
                            let packet = engine.handle_enemy_turn(current_id.as_ref().unwrap());
                            let roll = engine.state.last_roll.clone();
                            Some((packet, roll))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                
                if let Some((packet, roll_snap)) = enemy_packet_data {
                    if !packet.is_empty() {
                        stream_llm_narration(app_handle.clone(), packet).await;
                        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                    }
                    if roll_snap != "None" && !roll_snap.is_empty() {
                        let _ = app_handle.emit("dice-rolled", &roll_snap);
                    }
                    
                    let next_is_player = {
                        let app_state = app_handle.state::<AppState>();
                        let mut engine = app_state.engine.lock().unwrap();
                        engine.advance_turn();
                        engine.state.get_current_turn_id().map(|id| id == "player").unwrap_or(false)
                    };
                    
                    if next_is_player {
                        // Narrate any pending loot before giving the player control
                        let loot_packet = {
                            let app_state = app_handle.state::<AppState>();
                            let mut engine = app_state.engine.lock().unwrap();
                            engine.build_loot_packet()
                        };
                        if !loot_packet.is_empty() {
                            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                            stream_llm_narration(app_handle.clone(), loot_packet).await;
                        }
                        let app_state = app_handle.state::<AppState>();
                        let mut engine = app_state.engine.lock().unwrap();
                        let campaign = engine.campaign.clone();
                        engine.state.generate_available_actions(&campaign);
                        let _ = app_handle.emit("state-updated", &engine.state);
                        break;
                    } else {
                        let app_state = app_handle.state::<AppState>();
                        let engine = app_state.engine.lock().unwrap();
                        let _ = app_handle.emit("state-updated", &engine.state);
                    }
                } else {
                    break;
                }
            }
        }

        // Final state emission (skipped if already emitted inside enemy loop)
        {
            let app_state = app_handle.state::<AppState>();
            let engine = app_state.engine.lock().unwrap();
            let _ = app_handle.emit("state-updated", &engine.state);
        }
    });

    Ok(())
}

#[tauri::command]
async fn initialize_gemma(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let gemma = GemmaEngine::load_async(&app)
        .await
        .map_err(|e| e.to_string())?;

    let mut active = state.llm.active_engine.write().await;
    *active = Arc::new(gemma);

    Ok(())
}

fn initialize_game(campaign_id: &str) -> (Arc<CampaignData>, GameEngine) {
    let campaign_dir = Path::new("campaigns").join(campaign_id);
    let campaign_data = load_campaign(&campaign_dir).expect("Failed to load campaign!");
    let campaign_arc = Arc::new(campaign_data);
    let initial_state = engine::state::SessionState::new_from_campaign(&campaign_arc);
    let mut engine = GameEngine::new_with_state(initial_state, campaign_arc.clone());
    engine.update_visibility();
    (campaign_arc, engine)
}

#[tauri::command]
async fn start_game(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    campaign_id: String,
) -> Result<(), String> {
    let (_campaign_arc, engine) = initialize_game(&campaign_id);
    let mut app_state = state.engine.lock().map_err(|e| e.to_string())?;
    *app_state = engine;
    let mut dir = state.campaign_dir.lock().map_err(|e| e.to_string())?;
    *dir = campaign_id;
    drop(app_state);
    drop(dir);
    let _ = app.emit("state-updated", &state.engine.lock().unwrap().state);
    Ok(())
}

fn main() {
    // Try loading from save first, otherwise start with default campaign
    let (_campaign_arc, initial_engine) = if Path::new("save_game.json").exists() {
        match fs::read_to_string("save_game.json") {
            Ok(json) => {
                match serde_json::from_str::<engine::state::SessionState>(&json) {
                    Ok(saved_state) => {
                        println!("✅ Loaded save game");
                        let campaign_id = "the_sunless_crypt";
                        let campaign_dir = Path::new("campaigns").join(campaign_id);
                        let campaign_data = load_campaign(&campaign_dir).expect("Failed to load campaign!");
                        let campaign_arc = Arc::new(campaign_data);
                        (campaign_arc.clone(), GameEngine::new_with_state(saved_state, campaign_arc))
                    }
                    Err(_) => initialize_game("the_sunless_crypt")
                }
            }
            Err(_) => initialize_game("the_sunless_crypt")
        }
    } else {
        initialize_game("the_sunless_crypt")
    };

    let mock_engine: Arc<dyn LlmProvider> = Arc::new(MockLlm);

    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .manage(AppState {
            engine: Mutex::new(initial_engine),
            llm: Arc::new(LlmManager::new(mock_engine)),
            campaign_dir: Mutex::new("the_sunless_crypt".to_string()),
        })
        .setup(|app| {
            let app_handle = app.handle();
            let llm_manager = app_handle.state::<AppState>().llm.clone();
            let app_handle_clone = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let _ = app_handle_clone.emit("gemma-status", "Loading Gemma 4...");
                match GemmaEngine::load_async(&app_handle_clone).await {
                    Ok(gemma) => {
                        let mut active = llm_manager.active_engine.write().await;
                        *active = Arc::new(gemma);
                        println!("✅ Gemma 4 loaded on startup");
                        let _ = app_handle_clone.emit("gemma-status", "Gemma 4 Active 🐉");
                    }
                    Err(e) => {
                        eprintln!("⚠️ Failed to load Gemma on startup: {}", e);
                        let _ = app_handle_clone.emit("gemma-status", "Mock LLM Active (Gemma failed to load)");
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_game_state,
            generate_narration,
            initialize_gemma,
            player_button_action,
            save_game,
            load_save_exists,
            delete_save,
            list_campaigns,
            start_game
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}