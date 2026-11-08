fn horizonbot_level_tick(game: &mut crate::game::Game<'_>, dimension: &str) {
    crate::game::horizonbot_glue::level_tick(game, dimension);
}
