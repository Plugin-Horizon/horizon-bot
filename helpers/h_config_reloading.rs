fn horizonbot_config_reloading(config: &crate::game::native_configs::NativeConfigFile) {
    if config.mod_id == "horizonbot" {
        crate::game::horizonbot_glue::config_text(&config.text, true);
    }
}
