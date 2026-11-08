fn horizonbot_menu_clicked(game: &mut crate::game::Game<'_>, uuid: u128, slot: i32, button: i32) {
    crate::game::horizonbot_glue::menu_clicked(game, uuid, slot, button);
}
