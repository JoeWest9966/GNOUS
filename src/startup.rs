//! `startup-layouts`: 开机后自动铺开的桌面布局。
//!
//! One `Page` = one workspace: which windows it holds and how the screen is
//! split. `Boot` holds the timing/failure policy. The whole thing is a JSON
//! file under `$XDG_CONFIG_HOME/gnous/`; this module owns the model, the
//! on-disk round trip and the honest probes (displays, executable lookup).
//!
//! Nothing here launches a window yet — that is the executor's job and it is
//! deliberately not faked. What the UI can already prove is real: which
//! commands resolve on this machine, and what the connected display reports.
//!
//! Manifest: `extensions/startup-layouts/manifest.toml`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::sys;

/// Where the executor will append what actually happened. Read-only here: an
/// empty journal must stay empty rather than be seeded with demo rows.
pub const JOURNAL: &str = "startup-journal.jsonl";

// ============================================================================
// Model (serialized)
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Normal,
    Maximized,
    Fullscreen,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Normal => "normal",
            Role::Maximized => "maximized",
            Role::Fullscreen => "fullscreen",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowSpec {
    pub app: String,
    /// Two-character mark shown in the small square, like the demo.
    pub tag: String,
    /// Where the window comes from: 应用 / GNOUS 面板 / GNOME 扩展 …
    pub source: String,
    pub command: String,
    pub role: Role,
    pub delay_ms: u32,
}

/// How a page's screen is divided. Ratios only, never pixels: the same layout
/// has to survive a different display.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "ratios")]
pub enum Split {
    Columns(Vec<f32>),
    Rows(Vec<f32>),
    Grid,
}

impl Split {
    pub fn label(&self) -> &'static str {
        match self {
            Split::Columns(_) => "主次分割",
            Split::Rows(_) => "上下分割",
            Split::Grid => "四宫格",
        }
    }

    pub fn preset(&self, windows: usize) -> Preset {
        match self {
            Split::Grid => Preset::Grid,
            Split::Rows(_) => Preset::Stack,
            Split::Columns(ratios) => {
                if windows <= 1 {
                    Preset::Solo
                } else if ratios
                    .first()
                    .is_some_and(|first| (first - 1.0 / windows as f32).abs() < 0.02)
                {
                    Preset::Equal
                } else {
                    Preset::Main
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Solo,
    Equal,
    Main,
    Stack,
    Grid,
}

impl Preset {
    pub const ALL: [Preset; 5] = [
        Preset::Solo,
        Preset::Equal,
        Preset::Main,
        Preset::Stack,
        Preset::Grid,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Solo => "单窗",
            Preset::Equal => "等分",
            Preset::Main => "主次",
            Preset::Stack => "上下",
            Preset::Grid => "网格",
        }
    }

    /// A preset is only offered when the page has the windows it needs.
    pub fn enabled(self, windows: usize) -> bool {
        match self {
            Preset::Solo => windows == 1,
            Preset::Grid => windows == 4,
            Preset::Main | Preset::Stack => windows >= 2,
            Preset::Equal => windows >= 1,
        }
    }

    pub fn apply(self, windows: usize) -> Split {
        let count = windows.max(1);
        match self {
            Preset::Solo => Split::Columns(vec![1.0]),
            Preset::Equal => Split::Columns(vec![1.0 / count as f32; count]),
            Preset::Main => {
                // 2 窗 → 62/38，3 窗 → 56/22/22，更多窗按首列递减。
                let first = 0.5 + 0.12 * (count as f32 - 1.0);
                let rest = (1.0 - first) / (count as f32 - 1.0).max(1.0);
                Split::Columns(
                    std::iter::once(first)
                        .chain(std::iter::repeat_n(rest, count - 1))
                        .collect(),
                )
            }
            Preset::Stack => Split::Rows(vec![1.0 / count as f32; count]),
            Preset::Grid => Split::Grid,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub id: String,
    pub name: String,
    /// Whether the boot pass lays this page out at all.
    pub boot: bool,
    pub split: Split,
    pub windows: Vec<WindowSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Boot {
    pub delay_ms: u32,
    pub wait_ready: bool,
    pub every_login: bool,
    pub retries: u32,
    pub timeout_ms: u32,
    pub primary_display: String,
    pub degrade: bool,
    pub pad_workspaces: bool,
}

impl Default for Boot {
    fn default() -> Self {
        Self {
            delay_ms: 3000,
            wait_ready: true,
            every_login: true,
            retries: 1,
            timeout_ms: 8000,
            primary_display: String::new(),
            degrade: true,
            pad_workspaces: true,
        }
    }
}

/// Which boot switch a row toggles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootOption {
    WaitReady,
    EveryLogin,
    Degrade,
    PadWorkspaces,
}

impl BootOption {
    pub const ALL: [BootOption; 4] = [
        BootOption::WaitReady,
        BootOption::EveryLogin,
        BootOption::Degrade,
        BootOption::PadWorkspaces,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BootOption::WaitReady => "等待会话稳定",
            BootOption::EveryLogin => "每次登录都执行",
            BootOption::Degrade => "副屏缺失时降级",
            BootOption::PadWorkspaces => "工作区不足时补齐",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            BootOption::WaitReady => "gnome-shell 与 DBus 就绪后再开始",
            BootOption::EveryLogin => "关闭则只在首次登录（新会话）时执行",
            BootOption::Degrade => "把副屏页面挤到主屏，而不是丢弃",
            BootOption::PadWorkspaces => "GNOME 固定工作区数量 = 页面数",
        }
    }
}

/// What the boot page's numeric rows adjust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stepper {
    Delay,
    Retries,
    Timeout,
}

impl Stepper {
    pub const ALL: [Stepper; 3] = [Stepper::Delay, Stepper::Retries, Stepper::Timeout];

    pub fn label(self) -> &'static str {
        match self {
            Stepper::Delay => "登录后延迟",
            Stepper::Retries => "失败重试",
            Stepper::Timeout => "单页超时",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Stepper::Delay => "会话就绪后再等的时长",
            Stepper::Retries => "单页失败后重来几次，仍失败则跳过",
            Stepper::Timeout => "超过即判定失败，继续下一页",
        }
    }

    pub fn value(self, boot: &Boot) -> String {
        match self {
            Stepper::Delay => format!("{:.1} s", boot.delay_ms as f32 / 1000.0),
            Stepper::Retries => format!("{} 次", boot.retries),
            Stepper::Timeout => format!("{:.1} s", boot.timeout_ms as f32 / 1000.0),
        }
    }

    /// Step by one unit; clamped so the JSON can never hold nonsense.
    pub fn step(self, boot: &mut Boot, up: bool) {
        match self {
            Stepper::Delay => {
                let next = if up {
                    boot.delay_ms + 500
                } else {
                    boot.delay_ms.saturating_sub(500)
                };
                boot.delay_ms = next.clamp(0, 30_000);
            }
            Stepper::Retries => {
                boot.retries = if up {
                    (boot.retries + 1).min(5)
                } else {
                    boot.retries.saturating_sub(1)
                };
            }
            Stepper::Timeout => {
                let next = if up {
                    boot.timeout_ms + 1000
                } else {
                    boot.timeout_ms.saturating_sub(1000)
                };
                boot.timeout_ms = next.clamp(1000, 120_000);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub version: u32,
    pub pages: Vec<Page>,
    pub boot: Boot,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            version: 1,
            pages: default_pages(),
            boot: Boot::default(),
        }
    }
}

fn window(
    app: &str,
    tag: &str,
    source: &str,
    command: &str,
    role: Role,
    delay_ms: u32,
) -> WindowSpec {
    WindowSpec {
        app: app.to_string(),
        tag: tag.to_string(),
        source: source.to_string(),
        command: command.to_string(),
        role,
        delay_ms,
    }
}

fn default_pages() -> Vec<Page> {
    vec![
        Page {
            id: "p1".into(),
            name: "监控台".into(),
            boot: true,
            split: Split::Columns(vec![0.62, 0.38]),
            windows: vec![
                window(
                    "Vitals",
                    "VT",
                    "GNOME 扩展",
                    "gnome-extensions enable vitals@corentin",
                    Role::Maximized,
                    0,
                ),
                window(
                    "Processes",
                    "PS",
                    "GNOUS 面板",
                    "gnous --page processes --view live",
                    Role::Normal,
                    300,
                ),
            ],
        },
        Page {
            id: "p2".into(),
            name: "开发".into(),
            boot: true,
            split: Split::Columns(vec![0.50, 0.26, 0.24]),
            windows: vec![
                window("Code", "CD", "应用", "code", Role::Normal, 0),
                window("Terminal", "TM", "应用", "kgx", Role::Normal, 150),
                window("Firefox", "FF", "应用", "firefox", Role::Normal, 400),
            ],
        },
        Page {
            id: "p3".into(),
            name: "写作".into(),
            boot: true,
            split: Split::Columns(vec![1.0]),
            windows: vec![window(
                "Obsidian",
                "OB",
                "应用",
                "obsidian",
                Role::Maximized,
                0,
            )],
        },
        Page {
            id: "p4".into(),
            name: "会议".into(),
            boot: false,
            split: Split::Grid,
            windows: vec![
                window("Meet", "MT", "应用", "firefox", Role::Normal, 0),
                window(
                    "Notes",
                    "NT",
                    "GNOUS 面板",
                    "gnous --page notes",
                    Role::Normal,
                    200,
                ),
                window("Chat", "CH", "应用", "element-desktop", Role::Normal, 300),
                window(
                    "Calendar",
                    "CA",
                    "应用",
                    "gnome-calendar",
                    Role::Normal,
                    400,
                ),
            ],
        },
        Page {
            id: "p5".into(),
            name: "备用".into(),
            boot: false,
            split: Split::Columns(vec![0.5, 0.5]),
            windows: vec![
                window(
                    "Files",
                    "FI",
                    "应用",
                    "nautilus ~/Downloads",
                    Role::Normal,
                    0,
                ),
                window("Music", "MU", "应用", "spotify", Role::Normal, 500),
            ],
        },
    ]
}

// ============================================================================
// Probes (real, never guessed)
// ============================================================================

#[derive(Clone, Debug)]
pub struct Display {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Where the numbers came from, so the UI can say it out loud.
    pub source: &'static str,
}

/// Connected outputs and their preferred mode.
///
/// `/sys/class/drm` is the portable answer on this machine (Wayland has no
/// xrandr). The preferred mode is not always the active one, so it is labelled
/// as such instead of being presented as "current resolution".
pub fn displays() -> Vec<Display> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    dirs.sort();
    for dir in dirs {
        let Some(name) = dir.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        // card0-DP-1 → DP-1; skip the bare card0 node.
        let Some((_, output)) = name.split_once('-') else {
            continue;
        };
        let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
        if status.trim() != "connected" {
            continue;
        }
        let modes = std::fs::read_to_string(dir.join("modes")).unwrap_or_default();
        let Some(mode) = modes.lines().next() else {
            continue;
        };
        let Some((width, height)) = mode.split_once('x') else {
            continue;
        };
        let (Ok(width), Ok(height)) = (width.trim().parse::<u32>(), height.trim().parse::<u32>())
        else {
            continue;
        };
        out.push(Display {
            name: output.to_string(),
            width,
            height,
            source: "DRM 首选模式",
        });
    }
    out
}

fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("gnous")
}

fn config_path() -> PathBuf {
    config_dir().join("startup.json")
}

/// First whitespace-separated token of a command line.
pub fn first_token(command: &str) -> &str {
    command.split_whitespace().next().unwrap_or_default()
}

/// Resolve a command against PATH. Returns the absolute path that would be
/// executed, or `None` when it is not there — the UI shows the miss, it does
/// not assume the app exists.
pub fn resolve(command: &str) -> Option<PathBuf> {
    let token = first_token(command);
    if token.is_empty() {
        return None;
    }
    let path = PathBuf::from(token);
    if path.is_absolute() || token.contains('/') {
        return path.is_file().then_some(path);
    }
    let search = std::env::var_os("PATH")?;
    std::env::split_paths(&search)
        .map(|dir| dir.join(token))
        .find(|candidate| candidate.is_file())
}

// ============================================================================
// Runtime state
// ============================================================================

#[derive(Clone, Debug)]
pub struct PreviewLine {
    pub ok: bool,
    /// Page not participating in boot: neither runnable nor missing.
    pub skipped: bool,
    pub head: String,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct Preview {
    pub at: String,
    pub lines: Vec<PreviewLine>,
}

impl Preview {
    pub fn ok_count(&self) -> usize {
        self.lines
            .iter()
            .filter(|line| line.ok && !line.skipped)
            .count()
    }

    pub fn skip_count(&self) -> usize {
        self.lines.iter().filter(|line| line.skipped).count()
    }

    pub fn miss_count(&self) -> usize {
        self.lines.iter().filter(|line| !line.ok).count()
    }
}

pub struct State {
    pub layout: Layout,
    /// Selected page id; kept as an id because pages can be added.
    pub selected: String,
    pub window: usize,
    pub displays: Vec<Display>,
    pub note: Option<String>,
    pub error: Option<String>,
    pub preview: Option<Preview>,
    pub saved_at: String,
    /// User-authored launch projects, scanned from `<config>/gnous/projects/`.
    pub projects: Vec<Project>,
    /// Selected project id; empty when the folder holds no project yet.
    pub project: String,
    pub projects_at: String,
    pub projects_error: Option<String>,
}

impl State {
    pub fn load() -> Self {
        let path = config_path();
        let mut error = None;
        let mut fresh = false;
        let layout = match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<Layout>(&raw) {
                Ok(layout) => layout,
                Err(why) => {
                    error = Some(format!(
                        "{} 解析失败：{why}；已载入默认布局",
                        path.display()
                    ));
                    Layout::default()
                }
            },
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
                fresh = true;
                Layout::default()
            }
            Err(why) => {
                error = Some(format!("{} 读取失败：{why}", path.display()));
                Layout::default()
            }
        };
        let displays = displays();
        let (projects, projects_error) = scan_projects();
        let project = projects
            .first()
            .map(|project| project.id.clone())
            .unwrap_or_default();
        let mut state = Self {
            selected: layout
                .pages
                .first()
                .map(|page| page.id.clone())
                .unwrap_or_default(),
            layout,
            window: 0,
            displays,
            note: None,
            error,
            preview: None,
            saved_at: sys::local_clock(),
            projects,
            project,
            projects_at: sys::local_clock(),
            projects_error,
        };
        if state.layout.boot.primary_display.is_empty() {
            if let Some(display) = state.displays.first() {
                state.layout.boot.primary_display = display.name.clone();
            }
        }
        // First run: create the file now, so "配置文件" points at something
        // that exists instead of appearing only after the first edit.
        if fresh {
            state.persist();
        }
        state
    }

    pub fn path() -> PathBuf {
        config_path()
    }

    // ---- projects --------------------------------------------------------

    /// Re-reads the projects folder. Selection is kept when the project still
    /// exists, otherwise it falls back to the first one.
    pub fn refresh_projects(&mut self) {
        let (projects, error) = scan_projects();
        if !projects.iter().any(|project| project.id == self.project) {
            self.project = projects
                .first()
                .map(|project| project.id.clone())
                .unwrap_or_default();
        }
        self.projects = projects;
        self.projects_error = error;
        self.projects_at = sys::local_clock();
    }

    pub fn selected_project(&self) -> Option<&Project> {
        self.projects
            .iter()
            .find(|project| project.id == self.project)
    }

    pub fn select_project(&mut self, id: &str) {
        if self.projects.iter().any(|project| project.id == id) {
            self.project = id.to_string();
        }
    }

    /// Flips `boot` in the project manifest and re-scans so the list reflects
    /// what is actually on disk. Returns false (with `error` set) on failure.
    pub fn toggle_project_boot(&mut self, id: &str) -> bool {
        let Some(project) = self.projects.iter().find(|project| project.id == id) else {
            return false;
        };
        let wanted = !project.boot;
        match set_project_boot(id, wanted) {
            Ok(path) => {
                self.error = None;
                self.note = Some(format!(
                    "{} · 随开机启动 = {} · {}",
                    project.name,
                    if wanted { "是" } else { "否" },
                    path.display()
                ));
                self.refresh_projects();
                true
            }
            Err(why) => {
                self.error = Some(why);
                false
            }
        }
    }

    pub fn boot_projects(&self) -> usize {
        self.projects.iter().filter(|project| project.boot).count()
    }

    pub fn ready_projects(&self) -> usize {
        self.projects
            .iter()
            .filter(|project| project.readiness.is_ready())
            .count()
    }

    /// The display layouts are stored against: the one picked on the Boot
    /// page, falling back to the first connected output.
    pub fn primary(&self) -> Option<&Display> {
        self.displays
            .iter()
            .find(|display| display.name == self.layout.boot.primary_display)
            .or_else(|| self.displays.first())
    }

    pub fn page(&self) -> Option<&Page> {
        self.layout
            .pages
            .iter()
            .find(|page| page.id == self.selected)
    }

    pub fn page_mut(&mut self) -> Option<&mut Page> {
        let id = self.selected.clone();
        self.layout.pages.iter_mut().find(|page| page.id == id)
    }

    pub fn selected_index(&self) -> usize {
        self.layout
            .pages
            .iter()
            .position(|page| page.id == self.selected)
            .unwrap_or(0)
    }

    pub fn select(&mut self, id: &str) {
        if self.layout.pages.iter().any(|page| page.id == id) {
            self.selected = id.to_string();
            self.window = 0;
        }
    }

    pub fn select_window(&mut self, index: usize) {
        self.window = index;
    }

    pub fn boot_pages(&self) -> usize {
        self.layout.pages.iter().filter(|page| page.boot).count()
    }

    pub fn boot_windows(&self) -> usize {
        self.layout
            .pages
            .iter()
            .filter(|page| page.boot)
            .map(|page| page.windows.len())
            .sum()
    }

    pub fn toggle_boot(&mut self, id: &str) {
        if let Some(page) = self.layout.pages.iter_mut().find(|page| page.id == id) {
            page.boot = !page.boot;
            let name = page.name.clone();
            let on = page.boot;
            self.note = Some(format!("{} · 开机{}铺开", name, if on { "" } else { "不" }));
        }
    }

    pub fn apply_preset(&mut self, preset: Preset) -> bool {
        let Some(page) = self.page_mut() else {
            return false;
        };
        if !preset.enabled(page.windows.len()) {
            return false;
        }
        page.split = preset.apply(page.windows.len());
        self.window = 0;
        self.note = Some(format!("排列改为 {}（只存比例）", preset.label()));
        true
    }

    pub fn remove_selected_window(&mut self) -> bool {
        let index = self.window;
        let Some(page) = self.page_mut() else {
            return false;
        };
        if index >= page.windows.len() {
            return false;
        }
        let removed = page.windows.remove(index);
        page.split = page
            .split
            .preset(page.windows.len())
            .apply(page.windows.len());
        self.window = index
            .saturating_sub(1)
            .min(page.windows.len().saturating_sub(1));
        self.note = Some(format!("已移除 {}", removed.app));
        true
    }

    pub fn new_page(&mut self) -> String {
        let mut n = self.layout.pages.len() + 1;
        while self
            .layout
            .pages
            .iter()
            .any(|page| page.id == format!("p{n}"))
        {
            n += 1;
        }
        let id = format!("p{n}");
        self.layout.pages.push(Page {
            id: id.clone(),
            name: format!("新页面 {n}"),
            boot: false,
            split: Split::Columns(vec![1.0]),
            windows: Vec::new(),
        });
        self.selected = id.clone();
        self.window = 0;
        self.note = Some("已新建空白页".into());
        id
    }

    pub fn toggle_option(&mut self, option: BootOption) {
        let boot = &mut self.layout.boot;
        match option {
            BootOption::WaitReady => boot.wait_ready = !boot.wait_ready,
            BootOption::EveryLogin => boot.every_login = !boot.every_login,
            BootOption::Degrade => boot.degrade = !boot.degrade,
            BootOption::PadWorkspaces => boot.pad_workspaces = !boot.pad_workspaces,
        }
        self.note = Some(format!("{} · 已切换", option.label()));
    }

    pub fn step(&mut self, stepper: Stepper, up: bool) {
        stepper.step(&mut self.layout.boot, up);
        self.note = Some(format!(
            "{} → {}",
            stepper.label(),
            stepper.value(&self.layout.boot)
        ));
    }

    /// Write the layout. Called after every mutation, so a crash never loses
    /// the checkbox the user just flipped.
    pub fn persist(&mut self) {
        let path = config_path();
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let raw = serde_json::to_string_pretty(&self.layout).map_err(std::io::Error::other)?;
            std::fs::write(&path, raw + "\n")
        })();
        match result {
            Ok(()) => {
                self.saved_at = sys::local_clock();
                self.error = None;
            }
            Err(why) => {
                self.error = Some(format!("写入 {} 失败：{why}", path.display()));
            }
        }
    }

    /// Check every boot command against PATH. The result is evidence, not a
    /// promise: it says what *would* be executed.
    pub fn run_preview(&mut self) {
        let mut lines = Vec::new();
        for (index, page) in self.layout.pages.iter().enumerate() {
            let slot = format!("{:02} {}", index + 1, page.name);
            if !page.boot {
                lines.push(PreviewLine {
                    ok: true,
                    skipped: true,
                    head: format!("{slot} · 跳过"),
                    body: "未参与开机启动".into(),
                });
                continue;
            }
            if page.windows.is_empty() {
                lines.push(PreviewLine {
                    ok: true,
                    skipped: true,
                    head: format!("{slot} · 空白页"),
                    body: "只占一个工作区，不启动窗口".into(),
                });
                continue;
            }
            for win in &page.windows {
                let command = win.command.trim();
                if command.is_empty() {
                    lines.push(PreviewLine {
                        ok: false,
                        skipped: false,
                        head: format!("{slot} · {}", win.app),
                        body: "命令为空，无法启动".into(),
                    });
                    continue;
                }
                match resolve(command) {
                    Some(path) => lines.push(PreviewLine {
                        ok: true,
                        skipped: false,
                        head: format!("{slot} · {}", win.app),
                        body: format!("找到 {}", path.display()),
                    }),
                    None => lines.push(PreviewLine {
                        ok: false,
                        skipped: false,
                        head: format!("{slot} · {}", win.app),
                        body: format!("PATH 中找不到 `{}`", first_token(command)),
                    }),
                }
            }
        }
        let miss = lines.iter().filter(|line| !line.ok).count();
        let at = sys::local_clock();
        self.note = Some(if miss == 0 {
            format!("预演完成 · {at} · 全部命令都能解析")
        } else {
            format!("预演完成 · {at} · {miss} 个命令找不到")
        });
        self.preview = Some(Preview { at, lines });
    }
}

// ============================================================================
// Geometry (ratios, rendered as fractions of the screen)
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Slot {
    /// Percent label, used when no display is known: never pretend pixels.
    pub fn percent(&self) -> String {
        format!(
            "{}% × {}%",
            (self.w * 100.).round() as i32,
            (self.h * 100.).round() as i32
        )
    }

    pub fn pixels(&self, display: &Display) -> String {
        format!(
            "{}×{} · x {} y {}",
            (self.w * display.width as f32).round() as i32,
            (self.h * display.height as f32).round() as i32,
            (self.x * display.width as f32).round() as i32,
            (self.y * display.height as f32).round() as i32,
        )
    }
}

/// Slot rectangles for a page, in reading order. Ratios that do not match the
/// window count fall back to an even split, so a hand-edited file still draws.
pub fn slots(page: &Page) -> Vec<Slot> {
    let count = page.windows.len();
    if count == 0 {
        return Vec::new();
    }
    match &page.split {
        Split::Columns(ratios) => {
            let ratios = fit(ratios, count);
            let mut x = 0.0;
            ratios
                .iter()
                .map(|ratio| {
                    let slot = Slot {
                        x,
                        y: 0.0,
                        w: *ratio,
                        h: 1.0,
                    };
                    x += ratio;
                    slot
                })
                .collect()
        }
        Split::Rows(ratios) => {
            let ratios = fit(ratios, count);
            let mut y = 0.0;
            ratios
                .iter()
                .map(|ratio| {
                    let slot = Slot {
                        x: 0.0,
                        y,
                        w: 1.0,
                        h: *ratio,
                    };
                    y += ratio;
                    slot
                })
                .collect()
        }
        Split::Grid => (0..count)
            .map(|index| Slot {
                x: (index % 2) as f32 * 0.5,
                y: (index / 2) as f32 * 0.5,
                w: 0.5,
                h: 0.5,
            })
            .collect(),
    }
}

fn fit(ratios: &[f32], count: usize) -> Vec<f32> {
    if ratios.len() == count && ratios.iter().all(|ratio| *ratio > 0.01) {
        ratios.to_vec()
    } else {
        vec![1.0 / count as f32; count]
    }
}

// ============================================================================
// Projects — user-authored launch scripts
// ============================================================================
//
// A project is a directory under `<config>/gnous/projects/` holding a
// `project.toml` manifest plus the script it names. Nothing here executes a
// script: the page only registers, reads and shows what is on disk, so an
// absent or broken project stays visibly absent instead of looking launched.

pub const PROJECT_MANIFEST: &str = "project.toml";
pub const PROJECT_DEFAULT_SCRIPT: &str = "run.sh";

/// How much of the script the page keeps in memory for the read-only preview.
const BODY_LIMIT: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// Manifest parsed, script exists and carries the executable bit.
    Ready,
    /// Script is there but `chmod +x` was never run.
    NotExecutable,
    /// Manifest names a script that is not on disk.
    Missing,
    /// `project.toml` is absent or not valid TOML.
    Broken,
}

impl Readiness {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "可执行",
            Self::NotExecutable => "缺可执行位",
            Self::Missing => "脚本不存在",
            Self::Broken => "清单无法解析",
        }
    }

    /// Rank used to sort the list: broken projects float to the top so they
    /// cannot hide behind a healthy one.
    pub fn rank(self) -> u8 {
        match self {
            Self::Broken => 0,
            Self::Missing => 1,
            Self::NotExecutable => 2,
            Self::Ready => 3,
        }
    }

    pub fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Clone, Debug)]
pub struct Project {
    /// Directory name; also the stable key the UI selects by.
    pub id: String,
    pub name: String,
    pub summary: String,
    pub script: String,
    pub interpreter: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub boot: bool,
    pub order: i64,
    pub dir: PathBuf,
    pub readiness: Readiness,
    /// Why the manifest is unusable, verbatim from the parser.
    pub problem: Option<String>,
    pub lines: usize,
    /// First `BODY_LIMIT` lines of the script, when it is readable text.
    pub body: Option<String>,
}

impl Project {
    pub fn path(&self) -> PathBuf {
        self.dir.join(&self.script)
    }

    /// The command line the executor *would* run. Shown so the reader can see
    /// exactly what is registered — it is not copied from the script.
    pub fn command(&self) -> String {
        let mut parts = vec![self.interpreter.clone(), self.path().display().to_string()];
        for arg in &self.args {
            if arg.contains(char::is_whitespace) {
                parts.push(format!("\"{arg}\""));
            } else {
                parts.push(arg.clone());
            }
        }
        parts.join(" ")
    }

    /// Working directory resolved against the project folder.
    pub fn workdir(&self) -> PathBuf {
        if self.cwd.is_empty() {
            self.dir.clone()
        } else {
            self.dir.join(&self.cwd)
        }
    }
}

pub fn projects_root() -> PathBuf {
    config_dir().join("projects")
}

fn manifest_str(table: &toml::value::Table, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Reads one project directory. Never fails: a broken manifest becomes a
/// `Broken` project carrying the parser message.
fn read_project(id: String, dir: PathBuf) -> Project {
    let manifest = dir.join(PROJECT_MANIFEST);
    let mut problem = None;
    let mut table = toml::value::Table::new();
    match std::fs::read_to_string(&manifest) {
        Ok(raw) => match raw.parse::<toml::Value>() {
            Ok(toml::Value::Table(parsed)) => table = parsed,
            Ok(_) => problem = Some("顶层不是 TOML 表".to_string()),
            Err(why) => problem = Some(format!("TOML 解析失败：{why}")),
        },
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
            problem = Some(format!("缺少 {PROJECT_MANIFEST}"));
        }
        Err(why) => problem = Some(format!("读取失败：{why}")),
    }

    let script = manifest_str(&table, "script").unwrap_or_else(|| PROJECT_DEFAULT_SCRIPT.into());
    let interpreter = manifest_str(&table, "interpreter").unwrap_or_else(|| "bash".into());
    let args = table
        .get("args")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|value| value.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let path = dir.join(&script);

    let (readiness, mut body, mut lines) = match std::fs::read_to_string(&path) {
        Ok(raw) => {
            let total = raw.lines().count();
            let kept: Vec<&str> = raw.lines().take(BODY_LIMIT).collect();
            let body = kept.join("\n");
            (Readiness::Ready, Some(body), total)
        }
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => (Readiness::Missing, None, 0),
        Err(_) => {
            // Present but not readable as text (binary, permissions).
            (Readiness::NotExecutable, None, 0)
        }
    };
    let readiness = if problem.is_some() {
        Readiness::Broken
    } else if readiness == Readiness::Ready && !is_executable(&path) {
        Readiness::NotExecutable
    } else {
        readiness
    };
    if !readiness.is_ready() {
        body = body.filter(|_| readiness == Readiness::NotExecutable);
        if readiness != Readiness::NotExecutable {
            lines = 0;
        }
    }

    Project {
        name: manifest_str(&table, "name").unwrap_or_else(|| id.clone()),
        summary: manifest_str(&table, "summary").unwrap_or_default(),
        interpreter,
        args,
        cwd: manifest_str(&table, "cwd").unwrap_or_default(),
        boot: table
            .get("boot")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        order: table
            .get("order")
            .and_then(|value| value.as_integer())
            .unwrap_or(50),
        dir,
        script,
        id,
        readiness,
        problem,
        lines,
        body,
    }
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &std::path::Path) -> bool {
    true
}

/// Scans every project directory. Directories that do not carry a manifest
/// are still listed as `Broken` — a half-created project is a fact the user
/// needs to see, not something to silently skip.
pub fn scan_projects() -> (Vec<Project>, Option<String>) {
    let root = projects_root();
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), None);
        }
        Err(why) => {
            return (
                Vec::new(),
                Some(format!("{} 读取失败：{why}", root.display())),
            );
        }
    };

    let mut projects = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if id.starts_with('.') {
            continue;
        }
        projects.push(read_project(id, dir));
    }
    projects.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| a.readiness.rank().cmp(&b.readiness.rank()))
            .then_with(|| a.name.cmp(&b.name))
    });
    (projects, None)
}

/// Flips `boot` in the project's own manifest, keeping every other line
/// (comments included) untouched. Returns the manifest that was rewritten.
pub fn set_project_boot(id: &str, boot: bool) -> Result<PathBuf, String> {
    let dir = projects_root().join(id);
    let manifest = dir.join(PROJECT_MANIFEST);
    let raw = std::fs::read_to_string(&manifest)
        .map_err(|why| format!("{} 读取失败：{why}", manifest.display()))?;
    let wanted = format!("boot = {boot}");
    let mut replaced = false;
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        let is_boot = trimmed
            .strip_prefix("boot")
            .map(|rest| rest.trim_start().starts_with('='))
            .unwrap_or(false);
        if is_boot && !replaced {
            out.push(wanted.clone());
            replaced = true;
        } else if !is_boot {
            out.push(line.to_string());
        }
    }
    if !replaced {
        out.push(wanted);
    }
    let mut text = out.join("\n");
    if raw.ends_with('\n') {
        text.push('\n');
    }
    std::fs::write(&manifest, text)
        .map_err(|why| format!("{} 写入失败：{why}", manifest.display()))?;
    Ok(manifest)
}

/// Starter manifest shown in the empty state. It is a literal, not a generated
/// file: nothing is written until the user puts it there themselves.
pub const PROJECT_TEMPLATE: &str = r#"# <config>/gnous/projects/<id>/project.toml
name = "示例项目"
summary = "登录后它要做什么，一句话"
script = "run.sh"          # 相对本目录
interpreter = "bash"
args = []
cwd = "."                  # 相对本目录，或绝对路径
boot = false               # 随开机启动
order = 10                 # 开机顺序，小的先跑
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_cover_the_screen() {
        let page = Page {
            id: "p".into(),
            name: "p".into(),
            boot: true,
            split: Split::Columns(vec![0.62, 0.38]),
            windows: vec![],
        };
        let page = Page {
            windows: vec![
                window("a", "A", "应用", "a", Role::Normal, 0),
                window("b", "B", "应用", "b", Role::Normal, 0),
            ],
            ..page
        };
        let slots = slots(&page);
        assert_eq!(slots.len(), 2);
        assert!((slots[0].w - 0.62).abs() < 1e-6);
        assert!((slots[1].x - 0.62).abs() < 1e-6);
        assert!((slots[0].w + slots[1].w - 1.0).abs() < 1e-6);
    }

    #[test]
    fn preset_round_trip() {
        for preset in Preset::ALL {
            let windows = match preset {
                Preset::Solo => 1,
                Preset::Grid => 4,
                _ => 3,
            };
            let split = preset.apply(windows);
            assert_eq!(split.preset(windows), preset);
            let page = Page {
                id: "p".into(),
                name: "p".into(),
                boot: true,
                split,
                windows: (0..windows)
                    .map(|i| window("a", "A", "应用", "a", Role::Normal, i as u32))
                    .collect(),
            };
            let slots = slots(&page);
            assert_eq!(slots.len(), windows);
        }
    }

    #[test]
    fn mismatched_ratios_fall_back_to_even_split() {
        let page = Page {
            id: "p".into(),
            name: "p".into(),
            boot: true,
            split: Split::Columns(vec![1.0]),
            windows: vec![
                window("a", "A", "应用", "a", Role::Normal, 0),
                window("b", "B", "应用", "b", Role::Normal, 0),
            ],
        };
        let slots = slots(&page);
        assert!((slots[0].w - 0.5).abs() < 1e-6);
        assert!((slots[1].w - 0.5).abs() < 1e-6);
    }
}
