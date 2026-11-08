// A bot's moves come from its own client physics, which only approximate vanilla's: the
// anticheat's prediction would flag them. Bots are not checked (GAPS.md: a supported way).
fn horizonbot_skip_anticheat(hub: &crate::lan::hub::Hub, i: usize) -> core::ops::ControlFlow<bool> {
    if hub.guests.get(i).is_some_and(|g| crate::horizonbot::is_bot(g.uuid)) {
        return core::ops::ControlFlow::Break(false);
    }
    core::ops::ControlFlow::Continue(())
}
