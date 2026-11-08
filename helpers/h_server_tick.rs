fn horizonbot_server_tick(game: &mut crate::game::Game<'_>, phase: &horizon_client::mod_api::ServerTickPhase) {
    if matches!(phase, horizon_client::mod_api::ServerTickPhase::ServerTickStart { .. }) {
        crate::game::horizonbot_glue::server_tick_start(game);
    }
}
