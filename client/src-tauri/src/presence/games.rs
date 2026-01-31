//! Games database for process name matching.

use serde::{Deserialize, Serialize};

/// A game entry in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEntry {
    /// Process names that match this game (case-insensitive).
    pub process_names: Vec<String>,
    /// Optional command-line arguments to match.
    #[serde(default)]
    pub match_args: Vec<String>,
    /// Display name of the game.
    pub name: String,
    /// Type of activity.
    #[serde(rename = "type")]
    pub activity_type: String,
}

/// Database of known games.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamesDatabase {
    pub games: Vec<GameEntry>,
}

impl GamesDatabase {
    /// Load the games database from embedded JSON.
    pub fn load() -> Self {
        let json = include_str!("../../resources/games.json");
        serde_json::from_str(json).expect("Invalid games.json")
    }

    /// Find a game by process name (case-insensitive).
    /// Note: This ignores `match_args` filtering. Use `find_by_process_and_args` for full matching.
    #[allow(dead_code)]
    pub fn find_by_process(&self, process_name: &str) -> Option<&GameEntry> {
        let lower = process_name.to_lowercase();
        self.games
            .iter()
            .find(|g| g.process_names.iter().any(|p| p.to_lowercase() == lower))
    }

    /// Known generic launchers that require command line argument checking.
    const GENERIC_LAUNCHERS: &'static [&'static str] = &[
        "javaw.exe",
        "java.exe",
        "javaw",
        "java",
        "python.exe",
        "python",
        "python3",
        "python3.exe",
        "node.exe",
        "node",
    ];

    /// Find a game by process name and command line arguments.
    /// If the game entry has `match_args` AND the process is a generic launcher,
    /// the command line must contain at least one of the `match_args`.
    /// This prevents false positives for generic launchers like javaw.exe.
    pub fn find_by_process_and_args(
        &self,
        process_name: &str,
        cmd_args: &[String],
    ) -> Option<&GameEntry> {
        let lower_name = process_name.to_lowercase();
        let is_generic_launcher = Self::GENERIC_LAUNCHERS
            .iter()
            .any(|&launcher| launcher.to_lowercase() == lower_name);

        let cmd_lower: Vec<String> = cmd_args.iter().map(|s| s.to_lowercase()).collect();
        let cmd_joined = cmd_lower.join(" ");

        self.games.iter().find(|game| {
            // First check if process name matches
            let name_matches = game
                .process_names
                .iter()
                .any(|p| p.to_lowercase() == lower_name);

            if !name_matches {
                return false;
            }

            // If no match_args specified, name match is sufficient
            if game.match_args.is_empty() {
                return true;
            }

            // Only apply match_args filter for generic launchers
            // Game-specific executables (like minecraft.exe) don't need args checking
            if !is_generic_launcher {
                return true;
            }

            // For generic launchers, check if any match_args appear in the command line
            game.match_args
                .iter()
                .any(|arg| cmd_joined.contains(&arg.to_lowercase()))
        })
    }
}

impl Default for GamesDatabase {
    fn default() -> Self {
        Self::load()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_games_database() {
        let db = GamesDatabase::load();
        assert!(!db.games.is_empty(), "Games database should not be empty");
    }

    #[test]
    fn test_find_minecraft() {
        let db = GamesDatabase::load();
        let game = db.find_by_process("minecraft.exe");
        assert!(game.is_some());
        assert_eq!(game.unwrap().name, "Minecraft");
    }

    #[test]
    fn test_find_case_insensitive() {
        let db = GamesDatabase::load();
        let game = db.find_by_process("MINECRAFT.EXE");
        assert!(game.is_some());
        assert_eq!(game.unwrap().name, "Minecraft");
    }

    #[test]
    fn test_find_unknown_returns_none() {
        let db = GamesDatabase::load();
        let game = db.find_by_process("unknown_game.exe");
        assert!(game.is_none());
    }

    #[test]
    fn test_find_by_process_and_args_simple() {
        let db = GamesDatabase::load();
        // VS Code has no match_args, so should match on process name alone
        let game = db.find_by_process_and_args("code.exe", &[]);
        assert!(game.is_some());
        assert_eq!(game.unwrap().name, "Visual Studio Code");
    }

    #[test]
    fn test_find_by_process_and_args_with_match_args() {
        let db = GamesDatabase::load();
        // javaw.exe with minecraft args should match Minecraft
        let args = vec![
            "javaw.exe".to_string(),
            "-jar".to_string(),
            "minecraft.jar".to_string(),
        ];
        let game = db.find_by_process_and_args("javaw.exe", &args);
        assert!(game.is_some());
        assert_eq!(game.unwrap().name, "Minecraft");
    }

    #[test]
    fn test_find_by_process_and_args_javaw_without_minecraft() {
        let db = GamesDatabase::load();
        // javaw.exe without minecraft args should NOT match
        let args = vec![
            "javaw.exe".to_string(),
            "-jar".to_string(),
            "someother.jar".to_string(),
        ];
        let game = db.find_by_process_and_args("javaw.exe", &args);
        assert!(game.is_none());
    }

    #[test]
    fn test_find_by_process_and_args_minecraft_native() {
        let db = GamesDatabase::load();
        // minecraft.exe (native launcher) should match without args check
        let game = db.find_by_process_and_args("minecraft.exe", &[]);
        assert!(game.is_some());
        assert_eq!(game.unwrap().name, "Minecraft");
    }
}
