fn horizonbot_register_commands(commands: &mut Vec<horizon_client::mod_api::NativeCommand>) {
    crate::game::horizonbot_glue::register_commands(commands);
}
