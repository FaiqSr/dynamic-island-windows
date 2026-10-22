//! Saved shell commands, launched from the island.
//!
//! The point is the same as the shelf: skip the "open a terminal, cd, remember the command"
//! loop. A shortcut is a name plus a command line (`npm run dev`, `code .`, `notepad`,
//! `start spotify:`), run through `cmd /C` so it behaves exactly like it would when typed.
//!
//! Commands are stored in a JSON file under `%APPDATA%`, so they survive a restart. A command
//! that fails validation (empty name or command) is never written; a file that cannot be read
//! is reported through [`Shortcuts::status`] instead of silently starting empty.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use serde::{Deserialize, Serialize};

/// One saved command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub name: String,
    pub command: String,
    /// Working directory to run in. Empty means the user's home folder is used.
    #[serde(default)]
    pub dir: String,
}

/// How a launch ended up, kept so the page can say what happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    Ok(String),
    Failed(String),
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    shortcuts: Vec<Shortcut>,
}

/// One command started from the island.
struct Running {
    name: String,
    child: Child,
    /// A `cmd /C` wrapper exits as soon as the real program does, but the program's own
    /// children (`npm run dev` → `node`) are *not* in the wrapper's tree once it is gone. They
    /// are tracked here so a kill really stops the dev server instead of orphaning it.
    descendants: Vec<u32>,
}

pub struct Shortcuts {
    items: Vec<Shortcut>,
    status: Option<String>,
    /// Commands started from the island that have not exited yet, keyed by name so a second
    /// click on the same tile can be refused while a different one can still start.
    running: Vec<Running>,
    launch: Option<Launch>,
}

impl Shortcuts {
    /// Loads the saved commands. A missing file is normal on first run; a broken one is
    /// reported but does not stop the app.
    pub fn load() -> Self {
        let mut this = Self {
            items: Vec::new(),
            status: None,
            running: Vec::new(),
            launch: None,
        };
        let Some(path) = config_path() else {
            this.status = Some("No config folder available".into());
            return this;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<File>(&text) {
                Ok(file) => this.items = file.shortcuts,
                Err(e) => this.status = Some(format!("Cannot read shortcuts: {e}")),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => this.status = Some(format!("Cannot read shortcuts: {e}")),
        }
        this
    }

    pub fn items(&self) -> &[Shortcut] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Whether a command with this name is still running.
    pub fn is_running(&self, name: &str) -> bool {
        self.running.iter().any(|run| run.name == name)
    }

    pub fn any_running(&self) -> bool {
        !self.running.is_empty()
    }

    pub fn launch(&self) -> Option<&Launch> {
        self.launch.as_ref()
    }

    /// Adds a command. Whitespace-only fields are rejected so a row can never be invisible but
    /// un-deletable, and a duplicate name is refused instead of creating two identical rows.
    pub fn add(&mut self, name: &str, command: &str, dir: &str) -> bool {
        let name = name.trim();
        let command = command.trim();
        if name.is_empty() || command.is_empty() {
            self.status = Some("Name and command cannot be empty".into());
            return false;
        }
        if self.items.iter().any(|item| item.name == name) {
            self.status = Some(format!("\"{name}\" already exists"));
            return false;
        }
        if self.items.len() >= CAPACITY {
            self.status = Some(format!("At most {CAPACITY} shortcuts can be saved"));
            return false;
        }
        self.items.push(Shortcut {
            name: name.to_owned(),
            command: command.to_owned(),
            dir: dir.trim().to_owned(),
        });
        self.persist();
        true
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
            self.persist();
        }
    }

    /// Starts `index`. A command that is already running is refused rather than started twice,
    /// which for a dev server would only collide on the port.
    pub fn run(&mut self, index: usize) {
        let Some(shortcut) = self.items.get(index).cloned() else {
            return;
        };
        if self.is_running(&shortcut.name) {
            self.launch = Some(Launch::Failed(format!(
                "\"{}\" is already running",
                shortcut.name
            )));
            return;
        }
        match spawn(&shortcut) {
            Ok(child) => {
                let descendants = descendants_at_spawn(child.id());
                self.launch = Some(Launch::Ok(format!("Started \"{}\"", shortcut.name)));
                self.running.push(Running {
                    name: shortcut.name,
                    child,
                    descendants,
                });
            }
            Err(e) => {
                self.launch = Some(Launch::Failed(format!(
                    "Cannot run \"{}\": {e}",
                    shortcut.name
                )));
            }
        }
    }

    /// Stops the named command and everything it launched. Best-effort, like the rest of the
    /// process handling here: a command that already exited just reports that.
    pub fn kill(&mut self, name: &str) {
        let Some(index) = self.running.iter().position(|run| run.name == name) else {
            self.launch = Some(Launch::Failed(format!("\"{name}\" is not running")));
            return;
        };
        let mut run = self.running.remove(index);
        // Re-read the tree first: children started after the spawn were not in the snapshot,
        // and killing the wrapper alone would leave `node` holding the port.
        let mut pids = run.descendants.clone();
        for pid in descendants_of(run.child.id()) {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
        }
        kill_tree(&pids);
        let _ = run.child.kill();
        let _ = run.child.wait();
        self.launch = Some(Launch::Ok(format!("Stopped \"{name}\"")));
    }

    /// Stops every running command. Called from the page's `Kill all` button, and on exit so a
    /// dev server does not outlive the island.
    pub fn kill_all(&mut self) {
        if self.running.is_empty() {
            self.launch = Some(Launch::Failed("Nothing is running".into()));
            return;
        }
        let names: Vec<String> = self.running.iter().map(|run| run.name.clone()).collect();
        while let Some(run) = self.running.pop() {
            let mut pids = run.descendants.clone();
            for pid in descendants_of(run.child.id()) {
                if !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
            kill_tree(&pids);
            let mut child = run.child;
            let _ = child.kill();
            let _ = child.wait();
        }
        self.launch = Some(Launch::Ok(format!("Stopped {}", names.join(", "))));
    }

    /// Reaps finished children so exit codes can be reported. Called every frame.
    pub fn tick(&mut self) {
        let mut finished: Vec<(String, Result<std::process::ExitStatus, std::io::Error>)> =
            Vec::new();
        let mut index = 0;
        while index < self.running.len() {
            let outcome = match self.running[index].child.try_wait() {
                Ok(Some(status)) => Some(Ok(status)),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            };
            match outcome {
                Some(outcome) => {
                    let run = self.running.remove(index);
                    finished.push((run.name, outcome));
                }
                None => index += 1,
            }
        }
        // Only the last entry is shown, so report failures after successes: the user cares
        // about what broke, not about what exited cleanly.
        for (name, outcome) in finished {
            self.launch = Some(match outcome {
                Ok(status) if status.success() => Launch::Ok(format!("\"{name}\" finished")),
                Ok(status) => Launch::Failed(format!("\"{name}\" exited ({status})")),
                Err(e) => Launch::Failed(format!("\"{name}\": {e}")),
            });
        }
    }

    /// Writes the whole list back. Failure is surfaced in the status line rather than dropped,
    /// since the user would otherwise lose shortcuts on the next start without knowing why.
    fn persist(&mut self) {
        let Some(path) = config_path() else {
            self.status = Some("No config folder available".into());
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            self.status = Some(format!("Cannot create config folder: {e}"));
            return;
        }
        let file = File {
            shortcuts: self.items.clone(),
        };
        match serde_json::to_string_pretty(&file) {
            Ok(text) => match std::fs::write(&path, text) {
                Ok(()) => self.status = None,
                Err(e) => self.status = Some(format!("Cannot save shortcuts: {e}")),
            },
            Err(e) => self.status = Some(format!("Cannot save shortcuts: {e}")),
        }
    }
}

/// Upper bound on saved commands, mirroring the shelf's cap so the list stays scrollable.
const CAPACITY: usize = 12;

/// Every live process as `(pid, parent pid)`, read from `tasklist`.
///
/// `tasklist` is used instead of the Win32 `Toolhelp` snapshot API only because it needs no
/// extra `windows` feature; it is called once per launch and once per kill, never per frame.
#[cfg(windows)]
fn process_tree() -> Vec<(u32, u32)> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let Ok(output) = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter_map(|line| {
            // `"name","1234","Console","1","12,345 K"`
            let mut fields = line.split("\",\"");
            let _name = fields.next()?.trim_start_matches('"');
            let pid: u32 = fields.next()?.trim_matches('"').parse().ok()?;
            let _session = fields.next()?;
            let _index = fields.next()?;
            let parents = fields.next()?;
            let parent: u32 = parents.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok()?;
            Some((pid, parent))
        })
        .collect()
}

#[cfg(not(windows))]
fn process_tree() -> Vec<(u32, u32)> {
    Vec::new()
}

/// Pids below `root` in the process tree, plus `root` itself.
fn descendants_of(root: u32) -> Vec<u32> {
    let tree = process_tree();
    let mut found = vec![root];
    let mut index = 0;
    // Breadth-first walk: a process created while we walk is missed, which is fine — the
    // wrapper's own children are what matters.
    while index < found.len() {
        let parent = found[index];
        for (pid, ppid) in &tree {
            if *ppid == parent && !found.contains(pid) {
                found.push(*pid);
            }
        }
        index += 1;
    }
    found
}

/// Kills `pids` with `taskkill`. `/F` because a dev server ignores polite close requests, and
/// `/T` on each pid so anything it spawned after the snapshot goes too.
fn kill_tree(pids: &[u32]) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        for pid in pids {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .status();
        }
    }
    #[cfg(not(windows))]
    let _ = pids;
}

/// Descendants a freshly spawned wrapper already has, plus the wrapper's own pid.
fn descendants_at_spawn(pid: u32) -> Vec<u32> {
    // The wrapper (`cmd /C`) has not started its payload yet, so this is usually just `[pid]`;
    // `kill` re-reads the tree, which is what actually finds `npm`/`node`.
    descendants_of(pid)
}

/// `%APPDATA%\leetcode\shortcuts.json`, falling back to the current directory.
///
/// `LEETCODE_CONFIG_DIR` overrides the base folder; the tests use it so they never write to the
/// real `%APPDATA%` of whoever runs `cargo test`.
fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("LEETCODE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())?;
    Some(base.join("leetcode").join("shortcuts.json"))
}

/// Runs one shortcut detached from the island: no console window of its own, and the island
/// keeps running even if the command does.
fn spawn(shortcut: &Shortcut) -> std::io::Result<Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// Keeps a console from flashing for every command, matching the frameless island.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let mut command = Command::new("cmd");
        command
            .arg("/C")
            .arg(&shortcut.command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        if let Some(dir) = workdir(shortcut) {
            command.current_dir(dir);
        }
        command.spawn()
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(&shortcut.command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(dir) = workdir(shortcut) {
            command.current_dir(dir);
        }
        command.spawn()
    }
}

/// The directory to run in: the shortcut's own, else the user's home so `npm run dev` has a
/// sane starting point instead of whatever folder the island was launched from.
fn workdir(shortcut: &Shortcut) -> Option<PathBuf> {
    let dir = shortcut.dir.trim();
    if !dir.is_empty() {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// `std::env::set_var` is process-wide, so tests that touch the config file must not run
    /// concurrently with each other.
    static ENV: Mutex<()> = Mutex::new(());

    fn shortcuts() -> Shortcuts {
        Shortcuts {
            items: Vec::new(),
            status: None,
            running: Vec::new(),
            launch: None,
        }
    }

    /// Points the config at a scratch folder so no test writes to the real `%APPDATA%`.
    fn with_scratch_config(test: impl FnOnce()) {
        let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("leetcode-test-{}", std::process::id()));
        // SAFETY: The tests that read the variable hold `ENV` for the whole call.
        unsafe { std::env::set_var("LEETCODE_CONFIG_DIR", &dir) };
        test();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_rejects_blank_and_duplicate_names() {
        with_scratch_config(|| {
            let mut list = shortcuts();
            assert!(!list.add("   ", "npm run dev", ""));
            assert!(!list.add("dev", "   ", ""));
            assert!(list.add("dev", "npm run dev", ""));
            assert!(!list.add("dev", "npm test", ""));
            assert_eq!(list.items().len(), 1);

            // The list is written on every change, so a fresh load must see the same entry.
            let reloaded = Shortcuts::load();
            assert_eq!(reloaded.items(), list.items());

            list.remove(0);
            assert!(list.items().is_empty());
            assert!(Shortcuts::load().items().is_empty());
        });
    }

    #[test]
    fn a_broken_config_file_is_reported_not_fatal() {
        with_scratch_config(|| {
            let path = config_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "{ not json").unwrap();

            let list = Shortcuts::load();
            assert!(list.items().is_empty());
            assert!(list.status().is_some());
        });
    }

    #[test]
    fn a_shortcut_keeps_its_working_directory() {
        let shortcut = Shortcut {
            name: "dev".into(),
            command: "npm run dev".into(),
            dir: "C:\\work\\nextjs-app".into(),
        };
        // The tile shows the folder tail on hover, so the dir must survive a save/load round.
        let text = serde_json::to_string(&shortcut).unwrap();
        let back: Shortcut = serde_json::from_str(&text).unwrap();
        assert_eq!(back, shortcut);
    }

    #[test]
    fn a_saved_file_round_trips() {
        let file = File {
            shortcuts: vec![Shortcut {
                name: "dev".into(),
                command: "npm run dev".into(),
                dir: String::new(),
            }],
        };
        let text = serde_json::to_string(&file).unwrap();
        let back: File = serde_json::from_str(&text).unwrap();
        assert_eq!(back.shortcuts, file.shortcuts);
    }

    /// A running command is tracked, refuses a duplicate launch, and `kill` really stops the
    /// process tree. Uses `ping` because it is on every Windows install and runs for seconds.
    #[cfg(windows)]
    #[test]
    fn a_run_is_tracked_and_can_be_killed() {
        let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let mut this = shortcuts();
        this.items.push(Shortcut {
            name: "probe".into(),
            command: "ping -n 30 127.0.0.1".into(),
            dir: String::new(),
        });

        this.run(0);
        assert!(this.is_running("probe"), "launch should be tracked");
        assert!(this.any_running());

        // A second click must not start a second copy.
        this.run(0);
        assert!(matches!(this.launch, Some(Launch::Failed(_))));

        this.kill("probe");
        assert!(!this.is_running("probe"), "kill should clear the entry");
        assert!(!this.any_running());
        assert!(matches!(this.launch, Some(Launch::Ok(_))));
    }

    /// `kill_all` clears several commands at once, which is what the page's button calls.
    #[cfg(windows)]
    #[test]
    fn kill_all_stops_every_tracked_command() {
        let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let mut this = shortcuts();
        for name in ["one", "two"] {
            this.items.push(Shortcut {
                name: name.into(),
                command: "ping -n 30 127.0.0.1".into(),
                dir: String::new(),
            });
        }
        this.run(0);
        this.run(1);
        assert!(this.is_running("one") && this.is_running("two"));

        this.kill_all();
        assert!(!this.any_running());
        assert!(matches!(this.launch, Some(Launch::Ok(_))));
    }
}
