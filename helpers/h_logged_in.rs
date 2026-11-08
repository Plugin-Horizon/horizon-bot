fn horizonbot_logged_in(game: &mut crate::game::Game<'_>, player: u128, name: &str) {
    crate::game::horizonbot_glue::logged_in(game, player, name);
}
