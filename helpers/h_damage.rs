fn horizonbot_damage(entity: i32, causing: i32, amount: f32, canceled: &mut bool) {
    // A ghost can't be hurt.
    if horizon_client::horizonbot::is_ghost_entity(entity) {
        *canceled = true;
        return;
    }
    crate::game::horizonbot_glue::damage(entity, causing, amount, *canceled);
}
