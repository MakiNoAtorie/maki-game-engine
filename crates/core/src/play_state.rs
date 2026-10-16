//! The state of the game: title, play, pause, cutscene, end. In the core, so that
//! each crate can read it: the fixed step stands in a pause (`App::advance`), and the
//! sound is silent then. The rules of a game (a module of `maki-gameplay`) say what
//! the other states do.

use bevy_ecs::resource::Resource;
use serde::{Deserialize, Serialize};

/// The state of the game. `App::new` adds it; its value at start is `Play`.
/// A change goes through [`PlayState::change`]; `ResetPlay` sets the start state.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlayState {
    /// The title screen of a game.
    Title,
    /// The game plays.
    #[default]
    Play,
    /// No fixed step runs: the events, the scripts, and the walk stand.
    Pause,
    /// The game plays a scene: the pawn does not take the move actions.
    Cutscene,
    /// The game is over.
    End,
}

impl PlayState {
    /// True: the game can go from `self` to `to`.
    // ponytail: a pause goes back to play only, not to a cutscene. Keep the state
    // before the pause when a pause in a cutscene is needed.
    pub fn can_change(self, to: PlayState) -> bool {
        use PlayState::*;
        matches!(
            (self, to),
            (Title, Play)
                | (Play, Pause | Cutscene | End | Title)
                | (Pause, Play | Title)
                | (Cutscene, Play | End)
                | (End, Title)
        )
    }

    /// Goes to `to` if the table of [`PlayState::can_change`] allows it. False: no
    /// change. The same state: true, and no write.
    pub fn change(&mut self, to: PlayState) -> bool {
        if *self == to {
            return true;
        }
        let allowed = self.can_change(to);
        if allowed {
            *self = to;
        }
        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::PlayState::{self, *};

    #[test]
    fn the_game_changes_only_along_the_table() {
        let all = [Title, Play, Pause, Cutscene, End];
        let allowed: Vec<(PlayState, PlayState)> = all
            .iter()
            .flat_map(|&from| all.iter().map(move |&to| (from, to)))
            .filter(|&(from, to)| from.can_change(to))
            .collect();
        assert_eq!(
            allowed,
            [
                (Title, Play),
                (Play, Title),
                (Play, Pause),
                (Play, Cutscene),
                (Play, End),
                (Pause, Title),
                (Pause, Play),
                (Cutscene, Play),
                (Cutscene, End),
                (End, Title),
            ]
        );
        let mut state = Title;
        assert!(!state.change(End) && state == Title);
        assert!(state.change(Play) && state == Play);
        assert!(state.change(Play) && state == Play);
    }
}
