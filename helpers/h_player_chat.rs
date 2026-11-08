// A companion's owner giving an order in chat: the line is dropped for everyone and answered.
// Lib side (lan::hub::chat), so it reaches the plugin through `crate::horizonbot`.
fn horizonbot_player_chat(player: u128, name: &str, message: &str, replies: &mut Vec<crate::text::Text>, cancel: &mut bool) {
    let (cancelled, lines) = crate::horizonbot::with(|g| {
        let Some(p) = g.plugin.as_mut() else { return (false, Vec::new()) };
        let (c, lines, effects) = p.chat(player, name, message);
        g.queue.effects.extend(effects);
        (c, lines)
    });
    if cancelled {
        *cancel = true;
        replies.extend(lines.iter().map(|l| crate::text::Text::literal(l)));
    }
}
