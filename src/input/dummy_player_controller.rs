use ggez::{Context, GameResult};

use crate::input::player_controller::PlayerController;
use crate::shared_game_state::SharedGameState;

/// A no-op implementation of player controller.
#[derive(Clone)]
pub struct DummyPlayerController;

impl DummyPlayerController {
    pub fn new() -> DummyPlayerController {
        DummyPlayerController
    }
}

impl PlayerController for DummyPlayerController {
    fn update(&mut self, _state: &SharedGameState, _ctx: &mut Context) -> GameResult {
        Ok(())
    }

    fn update_trigger(&mut self) {}

    fn move_up(&self) -> bool {
        false
    }

    fn move_left(&self) -> bool {
        false
    }

    fn move_down(&self) -> bool {
        false
    }

    fn move_right(&self) -> bool {
        false
    }

    fn prev_weapon(&self) -> bool {
        false
    }

    fn next_weapon(&self) -> bool {
        false
    }

    fn jump(&self) -> bool {
        false
    }

    fn shoot(&self) -> bool {
        false
    }

    fn trigger_up(&self) -> bool {
        false
    }

    fn trigger_left(&self) -> bool {
        false
    }

    fn trigger_down(&self) -> bool {
        false
    }

    fn trigger_right(&self) -> bool {
        false
    }

    fn trigger_prev_weapon(&self) -> bool {
        false
    }

    fn trigger_next_weapon(&self) -> bool {
        false
    }

    fn trigger_jump(&self) -> bool {
        false
    }

    fn trigger_shoot(&self) -> bool {
        false
    }

    fn trigger_menu_ok(&self) -> bool {
        false
    }

    fn trigger_menu_back(&self) -> bool {
        false
    }

    fn trigger_menu_pause(&self) -> bool {
        false
    }

    fn look_up(&self) -> bool {
        false
    }

    fn look_left(&self) -> bool {
        false
    }

    fn look_down(&self) -> bool {
        false
    }

    fn look_right(&self) -> bool {
        false
    }

    fn move_analog_x(&self) -> f64 {
        0.0
    }

    fn move_analog_y(&self) -> f64 {
        0.0
    }
}