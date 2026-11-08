fn horizonbot_player_died(game: &mut crate::game::Game<'_>, player: u128) {
    crate::game::horizonbot_glue::player_died(game, player);
}
