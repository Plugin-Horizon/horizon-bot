// `bypass-damage-protection`: a hit involving a bot that another mod cancelled goes through
// anyway (it overrides protection, so it is off unless set). Cancelled ones are noted for
// `/horizonbot diagnose`. Runs on every exit of the hook (RETURN), so `value` is the unit result.
fn horizonbot_damage_after(value: (), entity: i32, causing: i32, canceled: &mut bool) -> () {
    if !*canceled || horizon_client::horizonbot::is_ghost_entity(entity) {
        return value;
    }
    if horizon_client::horizonbot::bypass_protection(entity, causing) {
        *canceled = false;
    } else {
        horizon_client::horizonbot::note_cancelled(entity, causing);
    }
    value
}
