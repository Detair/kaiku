//! Process scanner for detecting running games/applications.

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

use super::{GameEntry, GamesDatabase};

/// Scanner for detecting running games.
pub struct ProcessScanner {
    system: System,
    pub games_db: GamesDatabase,
}

impl ProcessScanner {
    /// Create a new process scanner.
    pub fn new() -> Self {
        Self {
            system: System::new_with_specifics(
                RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
            ),
            games_db: GamesDatabase::load(),
        }
    }

    /// Refresh process list and find matching game.
    /// Returns the first matching game found.
    /// Uses `match_args` filtering for games that require command line argument checking.
    pub fn scan(&mut self) -> Option<GameEntry> {
        self.system.refresh_processes(ProcessesToUpdate::All, true);

        for process in self.system.processes().values() {
            let Some(name) = process.name().to_str() else {
                continue;
            };
            let cmd_args: Vec<String> = process
                .cmd()
                .iter()
                .filter_map(|s| s.to_str().map(String::from))
                .collect();

            if let Some(game) = self.games_db.find_by_process_and_args(name, &cmd_args) {
                return Some(game.clone());
            }
        }
        None
    }

    /// Scan and return all detected games (not just first).
    /// Uses `match_args` filtering for games that require command line argument checking.
    pub fn scan_all(&mut self) -> Vec<GameEntry> {
        self.system.refresh_processes(ProcessesToUpdate::All, true);

        let mut found = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        for process in self.system.processes().values() {
            let Some(name) = process.name().to_str() else {
                continue;
            };
            let cmd_args: Vec<String> = process
                .cmd()
                .iter()
                .filter_map(|s| s.to_str().map(String::from))
                .collect();

            if let Some(game) = self.games_db.find_by_process_and_args(name, &cmd_args) {
                if seen_names.insert(game.name.clone()) {
                    found.push(game.clone());
                }
            }
        }
        found
    }
}

impl Default for ProcessScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_creation() {
        let scanner = ProcessScanner::new();
        assert!(!scanner.games_db.games.is_empty());
    }

    #[test]
    fn test_scanner_scan_runs() {
        let mut scanner = ProcessScanner::new();
        // Just verify scan doesn't panic - actual results depend on running processes
        let _ = scanner.scan();
    }

    #[test]
    fn test_scanner_scan_all_runs() {
        let mut scanner = ProcessScanner::new();
        // Just verify scan_all doesn't panic
        let results = scanner.scan_all();
        // Results are system-dependent, just ensure we got a valid vec
        drop(results);
    }
}
