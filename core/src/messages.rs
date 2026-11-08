//! The lines players see. Every line has a key; `messages.yml` (an optional override file,
//! nested by the key's dots) replaces single lines. `{0}`, `{1}`... are filled in.

use crate::yaml::Yaml;
use std::collections::BTreeMap;

/// The default wording for every key.
pub const DEFAULTS: &[(&str, &str)] = &[
    ("chat.attack-who", "Who should I go after? Add a name, like \"bot attack Villain\"."),
    ("chat.cancelled", "{0} dropped the current target."),
    ("chat.following", "{0} will tag along behind you."),
    ("chat.free", "{0} may wander off on their own again."),
    ("chat.guarding", "{0} will defend this spot."),
    ("chat.help", "Commands you can chat: \"bot follow\", \"bot guard\", \"bot stay\" (hold this spot), \"bot stop\" (freeze, no fighting back either), \"bot free\", \"bot attack <name>\", \"bot cancel\" (drop the target but keep following or guarding), \"bot summon\", \"bot status\". Put one bot's name first to give it an order alone, e.g. \"{0} stay\"; without a name all your bots here listen. Unless stopped, they still hit back when attacked."),
    ("chat.hunting", "{0} is going after {1}."),
    ("chat.not-found", "{0} was not found."),
    ("chat.not-here-many", "None of your bots are around at the moment."),
    ("chat.not-here-one", "Your bot is not around at the moment."),
    ("chat.state-away", "elsewhere"),
    ("chat.state-fighting", "in a fight with {0}"),
    ("chat.state-following", "trailing {0}"),
    ("chat.state-free", "wandering"),
    ("chat.state-guarding", "defending a spot"),
    ("chat.state-marching", "on the way to {0}"),
    ("chat.state-paused", "waiting"),
    ("chat.staying", "{0} will stay put, but still defend themselves when hit."),
    ("chat.stopped", "{0} froze completely and will neither move nor retaliate until you say otherwise."),
    ("chat.summon-failed", "{0} could not be brought over. Try again shortly."),
    ("chat.summon-wait", "Too soon: {0}s left before the next summon."),
    ("chat.summoned", "{0} was brought to your side."),
    ("err.no-bot", "There is no bot called {0}."),
    ("err.no-bot-spawned", "{0} is not currently spawned."),
    ("err.no-faction", "There is no faction called {0}."),
    ("err.no-faction-members", "Faction {0} does not exist or is empty."),
    ("err.no-faction-spawned", "Faction {0} does not exist or none of its members are spawned."),
    ("err.no-kit", "There is no kit called {0}."),
    ("err.no-overfaction", "There is no overfaction called {0}."),
    ("err.no-path", "There is no path called \"{0}\"."),
    ("err.no-permission", "You are not allowed to do that."),
    ("err.no-player", "{0} is not online."),
    ("err.no-preset", "There is no preset called \"{0}\"."),
    ("err.no-script", "There is no script called \"{0}\"."),
    ("err.no-technique", "There is no technique called {0}."),
    ("err.wrong-version", "HorizonBot cannot spawn bots on this server version. Supported: {0}. See the console for details."),
    ("prompt.ask", "{0}. Reply in chat within 60 seconds, or write \"cancel\"."),
    ("prompt.bot-name", "Enter a name for the bot"),
    ("prompt.cancelled", "Cancelled."),
    ("prompt.faction-name", "Enter a name for the new faction"),
    ("prompt.faction-skin", "Which account's skin should {0} wear? Give a Minecraft username"),
    ("prompt.kit-name", "Enter a name for the new kit"),
    ("prompt.save-kit", "Enter a kit name to store {0}'s gear under"),
    ("stats.everyone", "all players"),
    ("stats.line", "{0}{1}: {2} kills / {3} deaths (K/D {4}), accuracy {5} ({6} of {7} swings), {8} bow hits, damage dealt {9}, damage taken {10}"),
    ("stats.no-stats", "{0} has no recorded stats."),
    ("stats.none-yet", "Nobody has fought a bot yet."),
    ("stats.reset", "Cleared the stats of {0}."),
    ("stats.top", "Best {0} by {1}:"),
    ("update.available", "HorizonBot {0} is available (running {1}) - {2}"),
    // Feedback for commands.
    ("cmd.spawned", "{0} spawned."),
    ("cmd.spawned-some", "Only {0} of {1} spawned: the name table is used up."),
    ("cmd.removed", "{0} removed."),
    ("cmd.removed-all", "{0} bots removed."),
    ("cmd.list", "{0} bots: {1}"),
    ("cmd.done", "Done ({0} bots)."),
    ("cmd.set", "{0} set to {1}."),
    ("cmd.reloaded", "HorizonBot reloaded."),
    ("cmd.none-spawned", "No bots are spawned right now."),
    ("cmd.unknown", "{0}"),
];

#[derive(Clone, Debug, Default)]
pub struct Messages {
    overrides: BTreeMap<String, String>,
}

fn flatten(prefix: &str, v: &Yaml, out: &mut BTreeMap<String, String>) {
    match v {
        Yaml::Map(m) => {
            for (k, x) in m {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(&key, x, out);
            }
        }
        other => {
            if let Some(t) = other.as_text() {
                out.insert(prefix.to_owned(), t);
            }
        }
    }
}

impl Messages {
    pub fn load(doc: &Yaml) -> Messages {
        let mut overrides = BTreeMap::new();
        flatten("", doc, &mut overrides);
        Messages { overrides }
    }

    /// The line for a key with its values filled in.
    pub fn get(&self, key: &str, args: &[&str]) -> String {
        let template = self
            .overrides
            .get(key)
            .map(String::as_str)
            .or_else(|| DEFAULTS.iter().find(|(k, _)| *k == key).map(|(_, v)| *v))
            .unwrap_or(key);
        let mut out = template.to_owned();
        for (i, a) in args.iter().enumerate() {
            out = out.replace(&format!("{{{i}}}"), a);
        }
        out
    }
}

/// `&` colour codes to section signs, as players write them in `messages.yml`.
pub fn colours(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' && chars.peek().is_some_and(|n| "0123456789abcdefklmnor".contains(n.to_ascii_lowercase())) {
            out.push('\u{a7}');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_win_and_values_fill() {
        let doc = crate::yaml::parse("chat:\n  following: \"{0} is on your six.\"\n").unwrap();
        let m = Messages::load(&doc);
        assert_eq!(m.get("chat.following", &["Fido"]), "Fido is on your six.");
        assert_eq!(m.get("err.no-bot", &["Rex"]), "There is no bot called Rex.");
        assert_eq!(m.get("nonsense.key", &[]), "nonsense.key");
        assert_eq!(colours("&cred &zno"), "\u{a7}cred &zno");
    }
}
