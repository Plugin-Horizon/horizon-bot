//! Companions: a bot handed to a player, who orders it by typing in chat. A message is an order
//! only if its sender owns a spawned bot and it says `bot`/`bots` plus an order word; orders are
//! hidden from everyone else.

#[derive(Clone, Debug, PartialEq)]
pub enum Order {
    Follow,
    Guard,
    Stay,
    Stop,
    Free,
    Attack(String),
    Cancel,
    Summon,
    Status,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChatOrder {
    pub order: Order,
    /// Only these bots (by name); empty = all of the sender's bots.
    pub only: Vec<String>,
}

fn order_word(w: &str) -> Option<&'static str> {
    Some(match w {
        "follow" | "come" | "heel" | "escort" => "follow",
        "guard" | "defend" | "post" | "hold" => "guard",
        "stay" | "wait" | "freeze" => "stay",
        "stop" | "halt" | "disengage" | "shutdown" => "stop",
        "free" | "release" | "dismiss" | "roam" | "unleash" => "free",
        "attack" | "kill" | "fight" | "hunt" => "attack",
        "cancel" | "forget" | "calm" | "truce" => "cancel",
        "summon" | "teleport" | "warp" => "summon",
        "status" | "where" | "list" => "status",
        "help" => "help",
        _ => return None,
    })
}

/// Reads a chat line against the sender's bot names. None: not an order (it goes to chat).
pub fn parse(message: &str, own_bots: &[String]) -> Option<ChatOrder> {
    if own_bots.is_empty() {
        return None;
    }
    let words: Vec<String> = message
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    let lower: Vec<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
    if !lower.iter().any(|w| w == "bot" || w == "bots") {
        return None;
    }
    let (i, kind) = lower.iter().enumerate().find_map(|(i, w)| order_word(w).map(|k| (i, k)))?;
    let only: Vec<String> = own_bots
        .iter()
        .filter(|b| lower.iter().any(|w| w.eq_ignore_ascii_case(b)))
        .cloned()
        .collect();
    let order = match kind {
        "follow" => Order::Follow,
        "guard" => Order::Guard,
        "stay" => Order::Stay,
        "stop" => Order::Stop,
        "free" => Order::Free,
        "attack" => {
            // The first word after the order that is not one of the sender's own bots.
            let target = words[i + 1..]
                .iter()
                .find(|w| {
                    let l = w.to_ascii_lowercase();
                    l != "bot" && l != "bots" && !own_bots.iter().any(|b| b.eq_ignore_ascii_case(w))
                })?
                .clone();
            Order::Attack(target)
        }
        "cancel" => Order::Cancel,
        "summon" => Order::Summon,
        "status" => Order::Status,
        _ => Order::Help,
    };
    Some(ChatOrder { order, only })
}

/// `bot summon` works once every ten seconds.
pub const SUMMON_COOLDOWN_TICKS: u64 = 200;
/// `bot guard` holds the spot within this radius.
pub const GUARD_RADIUS: f64 = 10.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn bots() -> Vec<String> {
        vec!["Fido".into(), "Rex".into()]
    }

    #[test]
    fn orders_need_the_word_bot_and_an_order() {
        assert_eq!(parse("bot follow", &bots()), Some(ChatOrder { order: Order::Follow, only: vec![] }));
        assert_eq!(parse("Bots, heel!", &bots()).unwrap().order, Order::Follow);
        assert_eq!(parse("follow me", &bots()), None);
        assert_eq!(parse("that bot is cool", &bots()), None);
        assert_eq!(parse("bot follow", &[]), None);
    }

    #[test]
    fn one_bot_by_name_and_attack_targets() {
        assert_eq!(parse("bot Fido stay", &bots()), Some(ChatOrder { order: Order::Stay, only: vec!["Fido".into()] }));
        assert_eq!(
            parse("bot attack Steve", &bots()),
            Some(ChatOrder { order: Order::Attack("Steve".into()), only: vec![] })
        );
        assert_eq!(
            parse("bot Rex kill Steve", &bots()),
            Some(ChatOrder { order: Order::Attack("Steve".into()), only: vec!["Rex".into()] })
        );
        assert_eq!(parse("bot attack", &bots()), None);
        assert_eq!(parse("bot where are you", &bots()).unwrap().order, Order::Status);
    }
}
