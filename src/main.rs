mod ext;
mod fixes;
mod icons;
mod openshell;
mod pages;
mod proc;
mod registry;
mod root_align;
mod scroll;
mod startup;
mod sys;
mod table_state;
mod theme;
mod toggles;

use std::time::{Duration, Instant};

use root_align::Action as RootAlignAction;

use ext::Provider as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Bounds, ClipboardItem, Context, CursorStyle, Div, DragMoveEvent, FontWeight,
    Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _,
    PathBuilder, Pixels, Point, Render, ResizeEdge, Stateful, StatefulInteractiveElement as _,
    Styled, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowDecorations, WindowOptions,
    canvas, div, point, px, relative, rgba, size,
};
use theme::{
    FONT, accent, accent_hover, alpha, black, caps, dim, h, line, line_faint, line_strong, muted,
    ok, surface_2, text, tracked, txt, v, warn, white,
};

// ============================================================================
// App
// ============================================================================

/// Window chrome state: press tracking for drag + double-click-to-zoom.
const DOUBLE_CLICK: Duration = Duration::from_millis(320);
const DRAG_THRESHOLD: f32 = 4.;

/// A signal the user is about to send to a process. Signal names stay here so
/// the dialog and the keyboard shortcuts cannot disagree about what is armed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProcAction {
    Terminate,
    Kill,
    Stop,
    Continue,
}

impl ProcAction {
    pub(crate) fn title(self) -> &'static str {
        match self {
            ProcAction::Terminate => "结束进程",
            ProcAction::Kill => "强制结束进程",
            ProcAction::Stop => "暂停进程",
            ProcAction::Continue => "继续进程",
        }
    }

    pub(crate) fn verb(self) -> &'static str {
        match self {
            ProcAction::Terminate => "确认结束",
            ProcAction::Kill => "确认强杀",
            ProcAction::Stop => "确认暂停",
            ProcAction::Continue => "确认继续",
        }
    }

    pub(crate) fn warning(self) -> &'static str {
        match self {
            ProcAction::Terminate => "将发送 SIGTERM：进程有机会清理后退出，但也可能忽略这个信号。",
            ProcAction::Kill => "将发送 SIGKILL：进程立即终止，未保存的数据会丢失，且无法拦截。",
            ProcAction::Stop => "将发送 SIGSTOP：进程暂停执行，直到收到 SIGCONT 才恢复。",
            ProcAction::Continue => "将发送 SIGCONT：恢复被 SIGSTOP 暂停的进程。",
        }
    }

    pub(crate) fn signal(self) -> i32 {
        match self {
            ProcAction::Terminate => libc::SIGTERM,
            ProcAction::Kill => libc::SIGKILL,
            ProcAction::Stop => libc::SIGSTOP,
            ProcAction::Continue => libc::SIGCONT,
        }
    }

    pub(crate) fn signal_label(self) -> &'static str {
        match self {
            ProcAction::Terminate => "SIGTERM",
            ProcAction::Kill => "SIGKILL",
            ProcAction::Stop => "SIGSTOP",
            ProcAction::Continue => "SIGCONT",
        }
    }

    pub(crate) fn danger(self) -> bool {
        matches!(self, ProcAction::Kill | ProcAction::Terminate)
    }
}

/// Everything the confirmation dialog needs, captured when the action is
/// requested (the snapshot may have changed by the time the user confirms).
#[derive(Clone, Debug)]
pub(crate) struct ProcDialog {
    pub(crate) pid: u32,
    pub(crate) action: ProcAction,
    pub(crate) name: String,
    pub(crate) command: String,
}

/// Owned snapshot of transient process-view UI passed to the plugin renderer.
/// Owned (not borrowed) so overlay callbacks never capture `render`'s stack.
pub(crate) struct ProcOverlays {
    pub(crate) dialog: Option<ProcDialog>,
    pub(crate) menu: Option<(u32, f32, f32)>,
    pub(crate) toast: Option<String>,
    pub(crate) selected: Option<u32>,
    pub(crate) paused: bool,
    pub(crate) frozen: Option<u32>,
    pub(crate) viewport: (f32, f32),
    pub(crate) menu_name: String,
}

pub(crate) struct App {
    probe: sys::Probe,
    snap: sys::Snapshot,
    provider: ext::FilesystemProvider,
    extensions: Vec<ext::Extension>,
    toggles: toggles::Toggles,
    switches: Vec<toggles::Toggle>,
    proc_sampler: proc::Sampler,
    proc: proc::Snapshot,
    /// Total CPU% of the last 30 samples, for the live view.
    cpu_history: Vec<f32>,
    proc_selected: Option<u32>,
    /// Pending signal awaiting the confirmation dialog.
    proc_dialog: Option<ProcDialog>,
    /// Right-click popup on a table row: (pid, window x, window y).
    proc_menu: Option<(u32, f32, f32)>,
    /// Short-lived status message (copy feedback, signal result, ...).
    proc_toast: Option<String>,
    proc_toast_at: Option<Instant>,
    pub(crate) table: table_state::TableState,
    /// Scroll state for the shared page scroller and for the table body. The
    /// tick column and the keyboard both drive these, so a broken wheel is
    /// never a dead end.
    pub(crate) content_led: scroll::Led,
    pub(crate) table_led: scroll::Led,
    /// uid 0 = root; "我的" 过滤以此为基准.
    my_uid: u32,
    /// One state entry per `fixes::FIXES` row, in the same order.
    fixes: Vec<fixes::FixState>,
    /// HH:MM of the most recent fixes probe.
    fixes_checked: String,
    /// Batch "apply available fixes" needs a second click before mutating.
    fixes_apply_armed: bool,
    /// State of the root-align diagnostic console.
    root_align: root_align::State,
    /// Startup layouts: pages, splits and the boot policy (JSON-backed).
    startup: startup::State,
    /// Snapshot + action log of the openshell panel.
    openshell: openshell::State,
    /// UUID of the extension whose details are open in the right panel.
    selected: Option<String>,
    /// Last provider failure, shown inside the detail panel.
    error: Option<String>,
    nav: Nav,
    last_click: Option<Instant>,
    drag_anchor: Option<Point<gpui_kit::Pixels>>,
}

/// How often the overview re-samples the machine.
const REFRESH: Duration = Duration::from_secs(1);
/// Extensions change rarely; re-reading gsettings every tick would be waste.
const EXTENSION_TICKS: u32 = 10;

impl App {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut probe = sys::Probe::new();
        let snap = probe.sample();
        let provider = ext::FilesystemProvider::discover();
        let extensions = provider.list();
        let toggles = toggles::Toggles::new();
        let switches = toggles.read();
        // Two samples 120 ms apart: the first one has no previous reading, so
        // per-process CPU would be zero for the whole first second.
        let mut proc_sampler = proc::Sampler::new();
        let _ = proc_sampler.sample();
        std::thread::sleep(Duration::from_millis(120));
        let proc = proc_sampler.sample();
        // Sampling runs on the UI thread: every probe is a procfs/sysfs read,
        // so a tick costs microseconds and never races the render.
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(REFRESH).await;
                ticks = ticks.wrapping_add(1);
                let rescan = ticks % EXTENSION_TICKS == 0;
                let alive = this
                    .update(cx, |app, cx| {
                        app.snap = app.probe.sample();
                        if app
                            .proc_toast_at
                            .is_some_and(|at| at.elapsed() > Duration::from_millis(2400))
                        {
                            app.proc_toast = None;
                            app.proc_toast_at = None;
                        }
                        if app.nav.page.plugin == "processes" && !app.table.paused {
                            app.proc = app.proc_sampler.sample();
                            app.cpu_history.push(app.proc.cpu_total);
                            let excess = app.cpu_history.len().saturating_sub(30);
                            app.cpu_history.drain(0..excess);
                        }
                        if rescan {
                            app.provider.reload();
                            app.extensions = app.provider.list();
                            app.switches = app.toggles.read();
                        }
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
        Self {
            probe,
            snap,
            provider,
            extensions,
            toggles,
            switches,
            proc_sampler,
            proc,
            cpu_history: Vec::new(),
            proc_selected: None,
            proc_dialog: None,
            proc_menu: None,
            proc_toast: None,
            proc_toast_at: None,
            table: table_state::TableState::default(),
            content_led: scroll::Led::new(),
            table_led: scroll::Led::new(),
            my_uid: unsafe { libc::geteuid() },
            fixes: fixes::FIXES
                .iter()
                .enumerate()
                .map(|(index, fix)| {
                    let mut state = fixes::FixState::empty(fix.id);
                    state.expanded = index == 0;
                    state
                })
                .collect(),
            fixes_checked: sys::local_clock(),
            fixes_apply_armed: false,
            root_align: root_align::State::new(),
            startup: startup::State::load(),
            openshell: openshell::State::new(),
            selected: None,
            error: None,
            nav: Nav::default(),
            last_click: None,
            drag_anchor: None,
        }
    }

    /// Read the real `status` once when the diagnostic page is opened; the
    /// user can always re-run it from the button if the system changed.
    fn enter_root_align_page(&mut self, cx: &mut Context<Self>) {
        if self.root_align.status.is_none() && !self.root_align.busy {
            self.run_root_align_with_tab_switch(RootAlignAction::Status, false, cx);
        }
    }

    /// Read the whole gateway once when the panel is first opened.
    fn enter_openshell_page(&mut self, cx: &mut Context<Self>) {
        if !self.openshell.checked && !self.openshell.busy {
            self.probe_openshell(self.openshell.target.clone(), cx);
        }
    }

    /// Re-read the gateway on a background thread. The UI thread only flips
    /// `busy`; every value on the page comes from the returned snapshot.
    fn probe_openshell(&mut self, target: Option<String>, cx: &mut Context<Self>) {
        if self.openshell.busy {
            return;
        }
        self.openshell.busy = true;
        let weak = cx.entity().downgrade();
        let task = cx
            .background_executor()
            .spawn(async move { openshell::probe(target.as_deref()) });
        cx.spawn(async move |_, cx| {
            let snapshot = task.await;
            let _ = weak.update(cx, |app: &mut App, cx| {
                app.openshell.busy = false;
                app.openshell.absorb(snapshot);
                cx.notify();
            });
            cx.refresh();
        })
        .detach();
        cx.notify();
    }

    /// Run one panel action. Mutating actions need a second click; successful
    /// ones re-probe so the page shows the post-action truth.
    pub(crate) fn run_openshell(&mut self, action: openshell::Action, cx: &mut Context<Self>) {
        if self.openshell.busy {
            return;
        }
        if action.is_mutating() && self.openshell.armed.as_ref() != Some(&action) {
            self.openshell.armed = Some(action);
            cx.notify();
            return;
        }
        self.openshell.armed = None;
        if action.is_probe() {
            self.probe_openshell(self.openshell.target.clone(), cx);
            return;
        }
        self.openshell.busy = true;
        let weak = cx.entity().downgrade();
        let task_action = action.clone();
        let task = cx
            .background_executor()
            .spawn(async move { openshell::run(&task_action) });
        cx.spawn(async move |_, cx| {
            let outcome = task.await;
            let ok = outcome.ok;
            let _ = weak.update(cx, |app: &mut App, cx| {
                app.openshell.busy = false;
                app.openshell.record(outcome);
                if ok {
                    let target = app.openshell.target.clone();
                    app.probe_openshell(target, cx);
                }
                cx.notify();
            });
            cx.refresh();
        })
        .detach();
        cx.notify();
    }

    /// Point the detail blocks (policy / logs / ssh / providers) at a sandbox.
    pub(crate) fn select_openshell_target(&mut self, name: String, cx: &mut Context<Self>) {
        self.openshell.target = Some(name.clone());
        self.probe_openshell(Some(name), cx);
        cx.notify();
    }

    /// Switch section: the rail highlight and the tab row both follow.
    fn go_section(&mut self, section: Section, cx: &mut Context<Self>) {
        let page = section
            .pages()
            .into_iter()
            .next()
            .unwrap_or(PageRef::OVERVIEW);
        self.nav = Nav { section, page };
        self.selected = None;
        self.error = None;
        self.fixes_apply_armed = false;
        self.root_align.armed = None;
        self.openshell.armed = None;
        self.close_process_overlays();
        cx.notify();
    }

    fn go_page(&mut self, page: PageRef, cx: &mut Context<Self>) {
        self.nav.page = page;
        self.selected = None;
        self.error = None;
        self.fixes_apply_armed = false;
        self.root_align.armed = None;
        self.openshell.armed = None;
        self.close_process_overlays();
        if page.plugin == "processes" {
            self.proc = self.proc_sampler.sample();
            self.cpu_history.push(self.proc.cpu_total);
            let excess = self.cpu_history.len().saturating_sub(30);
            self.cpu_history.drain(0..excess);
        }
        if page.plugin == "root-align" {
            self.enter_root_align_page(cx);
        }
        if page.plugin == "openshell" {
            self.enter_openshell_page(cx);
        }
        cx.notify();
    }

    /// Expand/collapse one fix row; other rows stay as they are.
    pub(crate) fn toggle_fix(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if let Some(state) = self.fixes.iter_mut().find(|state| state.id == id) {
            state.expanded = !state.expanded;
        }
        cx.notify();
    }

    /// Run one action on the background executor; the UI only shows `执行中`
    /// until the real tool output comes back.
    pub(crate) fn run_fix(
        &mut self,
        id: &'static str,
        action: fixes::Action,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.fixes.iter_mut().find(|state| state.id == id) {
            if state.busy {
                return;
            }
            state.busy = true;
        }
        let weak = cx.entity().downgrade();
        let task = cx
            .background_executor()
            .spawn(async move { fixes::run(id, action) });
        cx.spawn(async move |_, cx| {
            let outcome = task.await;
            let recheck = outcome.ok && outcome.action != fixes::Action::Check;
            let _ = weak.update(cx, |app: &mut App, cx| {
                if let Some(state) = app.fixes.iter_mut().find(|state| state.id == id) {
                    state.absorb(outcome);
                }
                app.fixes_checked = sys::local_clock();
                cx.notify();
                // Apply/revert output is not a status probe: immediately run
                // `check` again so the row reflects what the system accepted.
                if recheck {
                    app.run_fix(id, fixes::Action::Check, cx);
                }
            });
            cx.refresh();
        })
        .detach();
        cx.notify();
    }

    /// Run `check` for every fix. Each action has its own task, so one slow
    /// tool never blocks the others.
    pub(crate) fn check_all_fixes(&mut self, cx: &mut Context<Self>) {
        for fix in fixes::FIXES {
            self.run_fix(fix.id, fixes::Action::Check, cx);
        }
    }

    /// Two-step batch apply: the first click only arms the button, the second
    /// applies every fix that is not already healthy/applied.
    pub(crate) fn apply_pending_fixes(&mut self, cx: &mut Context<Self>) {
        if !self.fixes_apply_armed {
            self.fixes_apply_armed = true;
            cx.notify();
            return;
        }
        self.fixes_apply_armed = false;
        // Only rows a real probe marked `待处理` may be applied. `未体检`
        // is never guessed at: run「全部体检」first.
        let pending: Vec<&'static str> = self
            .fixes
            .iter()
            .filter(|state| state.status == fixes::Status::Attention)
            .map(|state| state.id)
            .collect();
        if pending.is_empty() {
            cx.notify();
            return;
        }
        for id in pending {
            self.run_fix(id, fixes::Action::Apply, cx);
        }
    }

    /// Run one root-align action. Mutating actions need a second click; after
    /// a successful apply/revert, `status` is re-run automatically.
    pub(crate) fn run_root_align(&mut self, action: root_align::Action, cx: &mut Context<Self>) {
        self.run_root_align_with_tab_switch(action, true, cx);
    }

    fn run_root_align_with_tab_switch(
        &mut self,
        action: root_align::Action,
        switch_tab: bool,
        cx: &mut Context<Self>,
    ) {
        if self.root_align.busy {
            return;
        }
        if action.is_mutating() && self.root_align.armed != Some(action) {
            self.root_align.armed = Some(action);
            cx.notify();
            return;
        }
        self.root_align.armed = None;
        self.root_align.busy = true;
        let weak = cx.entity().downgrade();
        let task = cx
            .background_executor()
            .spawn(async move { root_align::run(action) });
        cx.spawn(async move |_, cx| {
            let outcome = task.await;
            let recheck = outcome.ok
                && matches!(
                    outcome.action,
                    RootAlignAction::Apply | RootAlignAction::Revert
                );
            let _ = weak.update(cx, |app: &mut App, cx| {
                app.root_align.absorb(outcome, switch_tab);
                cx.notify();
                // The automatic status probe refreshes state without stealing
                // the visible APPLY LOG tab.
                if recheck {
                    app.run_root_align_with_tab_switch(RootAlignAction::Status, false, cx);
                }
            });
            cx.refresh();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn select_root_tab(&mut self, tab: root_align::Tab, cx: &mut Context<Self>) {
        self.root_align.tab = tab;
        cx.notify();
    }

    pub(crate) fn select_root_domain(&mut self, domain: usize, cx: &mut Context<Self>) {
        self.root_align.domain = domain;
        cx.notify();
    }

    /// Send a signal and immediately re-sample, so the UI shows the result.
    /// The caller decides how to surface a failure (toast, not the extension
    /// detail panel's `error` field).
    pub(crate) fn signal_process(
        &mut self,
        pid: u32,
        sig: i32,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let result = proc::signal(pid, sig);
        self.proc = self.proc_sampler.sample();
        cx.notify();
        result
    }

    /// Ask the user to confirm a signal before sending it.
    pub(crate) fn request_signal(&mut self, pid: u32, action: ProcAction, cx: &mut Context<Self>) {
        self.proc_selected = Some(pid);
        self.proc_menu = None;
        self.proc_dialog = Some(ProcDialog {
            pid,
            action,
            name: self.process_name(pid),
            command: proc::command(pid),
        });
        cx.notify();
    }

    /// Confirm the open dialog (only the explicit button calls this).
    pub(crate) fn confirm_signal(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.proc_dialog.take() else {
            return;
        };
        self.proc_toast = Some(
            match self.signal_process(dialog.pid, dialog.action.signal(), cx) {
                Ok(()) => format!(
                    "已向 PID {} 发送 {}",
                    dialog.pid,
                    dialog.action.signal_label()
                ),
                Err(error) => format!("发送失败：{error}"),
            },
        );
        self.proc_toast_at = Some(Instant::now());
        cx.notify();
    }

    pub(crate) fn cancel_signal(&mut self, cx: &mut Context<Self>) {
        self.proc_dialog = None;
        cx.notify();
    }

    /// Open / close the row context menu. Position is window-local pixels.
    pub(crate) fn toggle_process_menu(&mut self, pid: u32, x: f32, y: f32, cx: &mut Context<Self>) {
        self.proc_selected = Some(pid);
        self.proc_dialog = None;
        self.proc_menu = Some((pid, x, y));
        cx.notify();
    }

    pub(crate) fn close_process_menu(&mut self, cx: &mut Context<Self>) {
        self.proc_menu = None;
        cx.notify();
    }

    fn close_process_overlays(&mut self) {
        self.proc_dialog = None;
        self.proc_menu = None;
        self.proc_toast = None;
        self.proc_toast_at = None;
    }

    pub(crate) fn process_name(&self, pid: u32) -> String {
        self.proc
            .processes
            .iter()
            .find(|p| p.pid == pid)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| format!("PID {pid}"))
    }

    /// Keep the selection and close the menu, so the detail panel stays open.
    pub(crate) fn focus_process_detail(&mut self, pid: u32, cx: &mut Context<Self>) {
        self.proc_selected = Some(pid);
        self.proc_menu = None;
        cx.notify();
    }

    /// Copy text to both the clipboard and the X11 primary selection, so a
    /// paste works with Ctrl+V and middle-click alike.
    pub(crate) fn copy_text(&mut self, text: String, label: &str, cx: &mut Context<Self>) {
        let item = ClipboardItem::new_string(text);
        cx.write_to_clipboard(item.clone());
        cx.write_to_primary(item);
        self.proc_toast = Some(format!("已复制{label}"));
        self.proc_toast_at = Some(Instant::now());
        self.proc_menu = None;
        cx.notify();
    }

    /// Build a TSV row matching the table columns.
    pub(crate) fn copy_process_row(&mut self, pid: u32, cx: &mut Context<Self>) {
        let Some(process) = self.proc.processes.iter().find(|p| p.pid == pid) else {
            return;
        };
        let text = format!(
            "{}\t{}\t{}\t{}\t{:.1}%\t{}\t{}\t{}",
            process.name,
            process.pid,
            process.user,
            process.state.label(),
            process.cpu,
            process.mem_label(),
            process.threads,
            process.age_label(),
        );
        self.copy_text(text, "进程行", cx);
    }

    pub(crate) fn copy_process_command(&mut self, pid: u32, cx: &mut Context<Self>) {
        let cmd = proc::command(pid);
        let text = if cmd.is_empty() {
            format!("PID {pid}")
        } else {
            cmd
        };
        self.copy_text(text, "命令行", cx);
    }

    /// Handle a keystroke while the Table view is visible. Plain characters
    /// are search input; all actions use Ctrl so typing "pipewire" can never
    /// accidentally arm SIGSTOP ("p") or copy ("c").
    pub(crate) fn table_key(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.proc_dialog.is_some() {
            match key {
                "enter" => self.confirm_signal(cx),
                "escape" => self.cancel_signal(cx),
                _ => {}
            }
            return;
        }
        // A menu is mouse-driven; any keystroke dismisses it so its labels
        // never go stale after a keyboard-driven state change.
        self.proc_menu = None;
        match key {
            "backspace" => self.table.backspace(),
            "escape" => {
                self.table.clear_query();
                self.proc_selected = None;
            }
            "ctrl-space" => self.table.paused = !self.table.paused,
            "ctrl-f" | "ctrl-F" => {
                self.table.frozen_pid = if self.table.frozen_pid.is_some() {
                    None
                } else {
                    self.proc_selected
                }
            }
            "ctrl-k" => {
                if let Some(pid) = self.proc_selected {
                    self.request_signal(pid, ProcAction::Terminate, cx);
                }
            }
            "ctrl-K" => {
                if let Some(pid) = self.proc_selected {
                    self.request_signal(pid, ProcAction::Kill, cx);
                }
            }
            "ctrl-p" => {
                if let Some(pid) = self.proc_selected {
                    self.request_signal(pid, ProcAction::Stop, cx);
                }
            }
            "ctrl-P" => {
                if let Some(pid) = self.proc_selected {
                    self.request_signal(pid, ProcAction::Continue, cx);
                }
            }
            "ctrl-c" => {
                if let Some(pid) = self.proc_selected {
                    self.copy_process_row(pid, cx);
                }
            }
            "ctrl-C" => {
                if let Some(pid) = self.proc_selected {
                    self.copy_process_command(pid, cx);
                }
            }
            "ctrl-1" => self.table.scope = table_state::Scope::All,
            "ctrl-2" => self.table.scope = table_state::Scope::Mine,
            "ctrl-3" => self.table.scope = table_state::Scope::System,
            ch if !ch.starts_with("ctrl-")
                && ch.len() == 1
                && !ch.chars().next().unwrap().is_control() =>
            {
                for c in ch.chars() {
                    self.table.set_query_char(c);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn sort_table(&mut self, key: table_state::SortKey, cx: &mut Context<Self>) {
        self.table.sort_by(key);
        cx.notify();
    }

    pub(crate) fn set_table_scope(&mut self, scope: table_state::Scope, cx: &mut Context<Self>) {
        self.table.scope = scope;
        cx.notify();
    }

    pub(crate) fn toggle_table_pause(&mut self, cx: &mut Context<Self>) {
        self.table.paused = !self.table.paused;
        cx.notify();
    }

    pub(crate) fn toggle_table_freeze(&mut self, cx: &mut Context<Self>) {
        if self.table.frozen_pid.is_some() {
            self.table.frozen_pid = None;
        } else {
            self.table.frozen_pid = self.proc_selected;
        }
        cx.notify();
    }

    /// Select a process row; used by every process view.
    pub(crate) fn select_process(&mut self, pid: u32, cx: &mut Context<Self>) {
        self.proc_selected = if self.proc_selected == Some(pid) {
            None
        } else {
            Some(pid)
        };
        cx.notify();
    }

    /// Open the detail panel for `uuid`, or close it when it is already open.
    fn select_extension(&mut self, uuid: &str, cx: &mut Context<Self>) {
        self.selected = if self.selected.as_deref() == Some(uuid) {
            None
        } else {
            Some(uuid.to_string())
        };
        self.error = None;
        cx.notify();
    }

    /// Flip a right-column switch and re-read all of them, so the UI shows
    /// what the system accepted rather than what we asked for.
    fn toggle_switch(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        self.error = self.toggles.set(id, on).err();
        self.switches = self.toggles.read();
        cx.notify();
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.error = None;
        cx.notify();
    }

    /// Flip an extension through the active provider, then re-read the
    /// inventory so the row reflects what the system actually accepted.
    fn toggle_extension(&mut self, uuid: &str, enabled: bool, cx: &mut Context<Self>) {
        self.error = match self.provider.set_enabled(uuid, enabled) {
            Ok(()) => None,
            Err(error) => Some(error),
        };
        self.provider.reload();
        self.extensions = self.provider.list();
        cx.notify();
    }

    // ---- startup-layouts -------------------------------------------------

    pub(crate) fn startup_toggle_boot(&mut self, id: &str, cx: &mut Context<Self>) {
        self.startup.toggle_boot(id);
        self.startup.persist();
        cx.notify();
    }

    pub(crate) fn startup_select_page(&mut self, id: &str, cx: &mut Context<Self>) {
        self.startup.select(id);
        cx.notify();
    }

    pub(crate) fn startup_select_window(&mut self, index: usize, cx: &mut Context<Self>) {
        self.startup.select_window(index);
        cx.notify();
    }

    pub(crate) fn startup_preset(&mut self, preset: startup::Preset, cx: &mut Context<Self>) {
        if self.startup.apply_preset(preset) {
            self.startup.persist();
        }
        cx.notify();
    }

    pub(crate) fn startup_new_page(&mut self, cx: &mut Context<Self>) {
        self.startup.new_page();
        self.startup.persist();
        cx.notify();
    }

    pub(crate) fn startup_remove_window(&mut self, cx: &mut Context<Self>) {
        if self.startup.remove_selected_window() {
            self.startup.persist();
        }
        cx.notify();
    }

    pub(crate) fn startup_toggle_option(
        &mut self,
        option: startup::BootOption,
        cx: &mut Context<Self>,
    ) {
        self.startup.toggle_option(option);
        self.startup.persist();
        cx.notify();
    }

    pub(crate) fn startup_step(
        &mut self,
        stepper: startup::Stepper,
        up: bool,
        cx: &mut Context<Self>,
    ) {
        self.startup.step(stepper, up);
        self.startup.persist();
        cx.notify();
    }

    pub(crate) fn startup_pick_display(&mut self, name: &str, cx: &mut Context<Self>) {
        self.startup.layout.boot.primary_display = name.to_string();
        self.startup.note = Some(format!("铺开屏 → {name}"));
        self.startup.persist();
        cx.notify();
    }

    pub(crate) fn startup_recheck_displays(&mut self, cx: &mut Context<Self>) {
        self.startup.displays = startup::displays();
        if self.startup.layout.boot.primary_display.is_empty() {
            if let Some(display) = self.startup.displays.first() {
                self.startup.layout.boot.primary_display = display.name.clone();
            }
        }
        self.startup.note = Some(match self.startup.displays.first() {
            Some(display) => format!(
                "探测到 {} 个显示器 · {} · {}×{}",
                self.startup.displays.len(),
                display.name,
                display.width,
                display.height
            ),
            None => "未探测到已连接显示器".to_string(),
        });
        cx.notify();
    }

    pub(crate) fn startup_preview(&mut self, cx: &mut Context<Self>) {
        self.startup.run_preview();
        cx.notify();
    }

    pub(crate) fn startup_save(&mut self, cx: &mut Context<Self>) {
        self.startup.persist();
        if self.startup.error.is_none() {
            self.startup.note = Some(format!(
                "已保存 · {} · {}",
                self.startup.saved_at,
                startup::State::path().display()
            ));
        }
        cx.notify();
    }

    pub(crate) fn startup_select_project(&mut self, id: &str, cx: &mut Context<Self>) {
        self.startup.select_project(id);
        cx.notify();
    }

    pub(crate) fn startup_rescan_projects(&mut self, cx: &mut Context<Self>) {
        self.startup.refresh_projects();
        match self.startup.projects_error.clone() {
            Some(error) => {
                self.startup.note = None;
                self.startup.error = Some(error);
            }
            None => {
                self.startup.error = None;
                self.startup.note = Some(format!(
                    "已扫描 · {} 个项目 · {}",
                    self.startup.projects.len(),
                    startup::projects_root().display()
                ));
            }
        }
        cx.notify();
    }

    pub(crate) fn startup_toggle_project_boot(&mut self, id: &str, cx: &mut Context<Self>) {
        self.startup.toggle_project_boot(id);
        cx.notify();
    }

    /// Creates the projects folder when it is missing, then hands it to the
    /// desktop file manager. No project files are generated here.
    pub(crate) fn startup_open_projects(&mut self, cx: &mut Context<Self>) {
        let root = startup::projects_root();
        match std::fs::create_dir_all(&root) {
            Ok(()) => {
                self.startup.error = None;
                self.startup.note = Some(format!("项目目录 · {}", root.display()));
                let _ = std::process::Command::new("xdg-open").arg(&root).spawn();
            }
            Err(why) => {
                self.startup.note = None;
                self.startup.error = Some(format!("{} 创建失败：{why}", root.display()));
            }
        }
        cx.notify();
    }

    /// The run button is deliberately inert: there is no executor yet, and
    /// this says so instead of faking a launch.
    pub(crate) fn startup_run_project_unavailable(&mut self, cx: &mut Context<Self>) {
        self.startup.error = None;
        self.startup.note = Some("执行器未接入：按钮不会启动任何进程".to_string());
        cx.notify();
    }
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let key_weak = cx.entity().downgrade();
        let table_view = self.nav.page.plugin == "processes" && self.nav.page.view == "table";
        let viewport = window.viewport_size();
        let viewport = (viewport.width.as_f32(), viewport.height.as_f32());
        let dialog = self.proc_dialog.clone();
        let menu = self.proc_menu;
        let menu_name = menu
            .map(|(pid, _, _)| self.process_name(pid))
            .unwrap_or_default();
        let toast = self.proc_toast.clone();
        let proc_selected = self.proc_selected;
        let paused = self.table.paused;
        let frozen = self.table.frozen_pid;

        h().relative()
            .size_full()
            .items_stretch()
            .bg(black())
            .font_family(FONT)
            .text_color(text())
            // Keyboard events follow the focus-to-root dispatch path. With no
            // focusable element the path is just the root, so a child listener
            // never sees them: the window root owns every shortcut.
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                let stroke = &event.keystroke;
                if stroke.modifiers.alt || stroke.modifiers.platform {
                    return;
                }
                let key = stroke.key.as_str();
                // Normalise shortcuts once, here. Plain characters belong to
                // the search field; every state-changing shortcut needs Ctrl.
                // On Linux Shift+K arrives as key "k" with an upper-case
                // key_char, and Ctrl+K has no key_char at all.
                let normalized = if key.len() == 1 && key.chars().all(|c| c.is_ascii_alphabetic()) {
                    let ch = key.chars().next().unwrap();
                    let ch = if stroke.modifiers.shift {
                        ch.to_ascii_uppercase()
                    } else {
                        stroke
                            .key_char
                            .as_deref()
                            .filter(|c| c.chars().count() == 1 && !c.chars().any(char::is_control))
                            .and_then(|c| c.chars().next())
                            .unwrap_or(ch)
                    };
                    if stroke.modifiers.control {
                        format!("ctrl-{ch}")
                    } else {
                        ch.to_string()
                    }
                } else if stroke.modifiers.control {
                    match key {
                        "space" => "ctrl-space".to_string(),
                        "1" | "2" | "3" => format!("ctrl-{key}"),
                        _ => return,
                    }
                } else {
                    stroke
                        .key_char
                        .as_deref()
                        .filter(|c| c.chars().count() == 1 && !c.chars().any(char::is_control))
                        .map(str::to_string)
                        .unwrap_or_else(|| key.to_string())
                };
                let _ = key_weak.update(cx, |app, cx| app.key_down(&normalized, cx));
            })
            .child(rail(self.nav.section, &weak))
            .child(
                v().flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(self.topbar(cx))
                    .child(content(
                        &Overview {
                            snap: &self.snap,
                            extensions: &self.extensions,
                            switches: &self.switches,
                            selected: self.selected.as_deref(),
                            error: self.error.as_deref(),
                            page: self.nav.page,
                            proc: &self.proc,
                            cpu_history: &self.cpu_history,
                            proc_selected,
                            table: &self.table,
                            my_uid: self.my_uid,
                            fixes: &self.fixes,
                            fixes_checked: &self.fixes_checked,
                            fixes_apply_armed: self.fixes_apply_armed,
                            root_align: &self.root_align,
                            startup: &self.startup,
                            openshell: &self.openshell,
                            content_led: &self.content_led,
                            table_led: &self.table_led,
                        },
                        cx,
                    )),
            )
            .child(resize_grips())
            .when(table_view, |this| {
                this.child(pages::processes::overlays(
                    ProcOverlays {
                        dialog,
                        menu,
                        toast,
                        selected: proc_selected,
                        paused,
                        frozen,
                        viewport,
                        menu_name,
                    },
                    &weak,
                ))
            })
    }
}

// ============================================================================
// Navigation: rail selects a section, the top bar tabs select a page inside it
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Overview,
    Extensions,
    Appearance,
    Devices,
    Startup,
    Power,
}

impl Section {
    const ALL: &'static [Section] = &[
        Section::Overview,
        Section::Extensions,
        Section::Appearance,
        Section::Devices,
        Section::Startup,
        Section::Power,
    ];

    fn index(self) -> u32 {
        match self {
            Section::Overview => 0,
            Section::Extensions => 1,
            Section::Appearance => 2,
            Section::Devices => 3,
            Section::Startup => 4,
            Section::Power => 5,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Overview => icons::GRID,
            Section::Extensions => icons::PUZZLE,
            Section::Appearance => icons::SUN,
            Section::Devices => icons::MONITOR,
            Section::Startup => icons::STARTUP,
            Section::Power => icons::POWER,
        }
    }

    /// Key used by plugin manifests to attach pages to this section.
    fn key(self) -> &'static str {
        match self {
            Section::Overview => "overview",
            Section::Extensions => "extensions",
            Section::Appearance => "appearance",
            Section::Devices => "devices",
            Section::Startup => "startup",
            Section::Power => "power",
        }
    }

    /// Pages this section shows in the top bar. First one is the default.
    /// Core pages come first, then whatever plugins registered for the section.
    fn pages(self) -> Vec<PageRef> {
        let core = match self {
            Section::Overview => vec![PageRef::OVERVIEW],
            Section::Extensions => vec![PageRef::INSTALLED],
            Section::Appearance => vec![PageRef::APPEARANCE],
            Section::Devices => vec![PageRef::DEVICES],
            Section::Startup => Vec::new(),
            Section::Power => vec![PageRef::POWER],
        };
        let mut pages = core;
        for (manifest, view) in registry::views_for(self.key()) {
            pages.push(PageRef {
                plugin: manifest.id.as_str(),
                view: view.id.as_str(),
                label: view.label.as_str(),
            });
        }
        if pages.is_empty() {
            pages.push(PageRef::PENDING);
        }
        pages
    }
}

/// One top-bar tab. `plugin` is `core` for built-in pages, otherwise the id of
/// the plugin that declared the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PageRef {
    plugin: &'static str,
    view: &'static str,
    label: &'static str,
}

impl PageRef {
    const OVERVIEW: PageRef = PageRef {
        plugin: "core",
        view: "overview",
        label: "Overview",
    };
    const INSTALLED: PageRef = PageRef {
        plugin: "core",
        view: "installed",
        label: "Installed",
    };
    const APPEARANCE: PageRef = PageRef {
        plugin: "core",
        view: "appearance",
        label: "Appearance",
    };
    const DEVICES: PageRef = PageRef {
        plugin: "core",
        view: "devices",
        label: "Devices",
    };
    const POWER: PageRef = PageRef {
        plugin: "core",
        view: "power",
        label: "Power",
    };
    /// A section whose plugin contributed no views: draw an honest placeholder
    /// instead of hard-coding an extension name here.
    const PENDING: PageRef = PageRef {
        plugin: "core",
        view: "pending",
        label: "Pending",
    };
}

/// Single source of truth for navigation: both the rail and the tabs render
/// from it, so they can never disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Nav {
    section: Section,
    page: PageRef,
}

impl Default for Nav {
    fn default() -> Self {
        Self {
            section: Section::Overview,
            page: PageRef::OVERVIEW,
        }
    }
}

// ============================================================================
// Left icon rail
// ============================================================================

fn rail(current: Section, weak: &WeakEntity<App>) -> Div {
    let mut column = v()
        .w(px(68.))
        .flex_shrink_0()
        .items_center()
        .py(px(22.))
        .gap(px(6.))
        .border_r(px(1.))
        .border_color(line())
        .child(
            div()
                .size(px(30.))
                .mb(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .border(px(1.))
                .border_color(alpha(0xffffff2e))
                .child(tracked("G", 11., FontWeight::SEMIBOLD, text(), 0.08)),
        );
    for section in Section::ALL {
        column = column.child(rail_button(*section, *section == current, weak));
    }
    column.child(div().flex_1()).child(rail_settings_button())
}

fn rail_button(section: Section, active: bool, weak: &WeakEntity<App>) -> Stateful<Div> {
    let color = if active { text() } else { dim() };
    let weak = weak.clone();
    div()
        .relative()
        .w(px(40.))
        .h(px(40.))
        .flex()
        .items_center()
        .justify_center()
        .id(("rail", section.index()))
        .cursor_pointer()
        .hover(|this| this.bg(alpha(0xffffff0a)))
        .when(active, |this| {
            this.child(
                div()
                    .absolute()
                    .left(px(-14.))
                    .top(px(12.))
                    .bottom(px(12.))
                    .w(px(2.))
                    .bg(accent()),
            )
        })
        .child(icons::icon(section.icon(), 18., color))
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |app, cx| app.go_section(section, cx));
        })
}

/// Settings is not a section yet; keep the icon but leave it inert.
fn rail_settings_button() -> impl IntoElement {
    div()
        .w(px(40.))
        .h(px(40.))
        .flex()
        .items_center()
        .justify_center()
        .child(icons::icon(icons::SETTINGS, 18., dim()))
}

// ============================================================================
// Top bar
// ============================================================================

impl App {
    /// The 56px header doubles as the window titlebar: drag to move the window,
    /// double-click to zoom (maximize/restore), right-click for the native
    /// GNOME window menu.
    fn topbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let weak_down = cx.entity().downgrade();
        let weak_move = cx.entity().downgrade();
        let weak_up = cx.entity().downgrade();
        let tab_weak = cx.entity().downgrade();
        let snap = &self.snap;

        h().id("titlebar")
            .h(px(56.))
            .flex_shrink_0()
            .items_center()
            .gap(px(34.))
            .px(px(40.))
            .border_b(px(1.))
            .border_color(line())
            .child(tracked("GNOUS", 13., FontWeight::MEDIUM, text(), 0.34))
            .child(
                h().h_full().items_stretch().gap(px(30.)).children(
                    self.nav
                        .section
                        .pages()
                        .iter()
                        .map(|page| nav_item(*page, *page == self.nav.page, &tab_weak)),
                ),
            )
            .child(div().flex_1())
            .child(
                h().items_center()
                    .gap(px(22.))
                    .text_size(px(12.))
                    .text_color(muted())
                    .child(
                        h().items_center()
                            .gap(px(7.))
                            .child(div().size(px(5.)).rounded_full().bg(if snap.healthy {
                                ok()
                            } else {
                                warn()
                            }))
                            .child(div().child(if snap.healthy {
                                "System nominal"
                            } else {
                                "System attention"
                            })),
                    )
                    .child(div().child(snap.session.clone()))
                    .child(div().text_color(text()).child(snap.clock.clone())),
            )
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                let now = Instant::now();
                let mut zoom = false;
                let _ = weak_down.update(cx, |this, _| {
                    let double = this
                        .last_click
                        .is_some_and(|at| now.duration_since(at) < DOUBLE_CLICK);
                    if double {
                        this.last_click = None;
                        this.drag_anchor = None;
                        zoom = true;
                    } else {
                        this.last_click = Some(now);
                        this.drag_anchor = Some(event.position);
                    }
                });
                if zoom {
                    window.zoom_window();
                }
            })
            .on_mouse_move(move |event, window, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let mut begin_move = false;
                let _ = weak_move.update(cx, |this, _| {
                    if let Some(anchor) = this.drag_anchor {
                        let moved = (event.position.x.as_f32() - anchor.x.as_f32()).abs()
                            > DRAG_THRESHOLD
                            || (event.position.y.as_f32() - anchor.y.as_f32()).abs()
                                > DRAG_THRESHOLD;
                        if moved {
                            this.drag_anchor = None;
                            begin_move = true;
                        }
                    }
                });
                if begin_move {
                    window.start_window_move();
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                let _ = weak_up.update(cx, |this, _| this.drag_anchor = None);
            })
            .on_mouse_down(MouseButton::Right, |event, window, _| {
                window.show_window_menu(event.position);
            })
    }
}

/// Invisible resize grips for the client-side-decorated window edges.
fn resize_grips() -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .child(grip("rz-top", ResizeEdge::Top, |d| {
            d.top_0().left(px(12.)).right(px(12.)).h(px(5.))
        }))
        .child(grip("rz-bottom", ResizeEdge::Bottom, |d| {
            d.bottom_0().left(px(12.)).right(px(12.)).h(px(5.))
        }))
        .child(grip("rz-left", ResizeEdge::Left, |d| {
            d.left_0().top(px(12.)).bottom(px(12.)).w(px(5.))
        }))
        .child(grip("rz-right", ResizeEdge::Right, |d| {
            d.right_0().top(px(12.)).bottom(px(12.)).w(px(5.))
        }))
        .child(grip("rz-top-left", ResizeEdge::TopLeft, |d| {
            d.top_0().left_0().size(px(12.))
        }))
        .child(grip("rz-top-right", ResizeEdge::TopRight, |d| {
            d.top_0().right_0().size(px(12.))
        }))
        .child(grip("rz-bottom-left", ResizeEdge::BottomLeft, |d| {
            d.bottom_0().left_0().size(px(12.))
        }))
        .child(grip("rz-bottom-right", ResizeEdge::BottomRight, |d| {
            d.bottom_0().right_0().size(px(12.))
        }))
}

fn grip(id: &'static str, edge: ResizeEdge, place: impl FnOnce(Div) -> Div) -> impl IntoElement {
    place(div().absolute())
        .id(id)
        .on_mouse_down(MouseButton::Left, move |_, window, _| {
            window.start_window_resize(edge);
        })
}

fn nav_item(page: PageRef, active: bool, weak: &WeakEntity<App>) -> Stateful<Div> {
    let color = if active { text() } else { muted() };
    let weakness = weak.clone();
    div()
        .h_full()
        .flex()
        .items_center()
        .id(page.view)
        .cursor_pointer()
        .border_b(px(1.))
        .border_color(if active { accent() } else { alpha(0x00000000) })
        .hover(|this| this.border_color(if active { accent() } else { line_strong() }))
        .child(caps(page.label, 12., FontWeight::NORMAL, color, 0.13))
        .on_click(move |_, _, cx| {
            let _ = weakness.update(cx, |app, cx| app.go_page(page, cx));
        })
}

// ============================================================================
// Scrollable content
// ============================================================================

/// Everything the overview needs for one frame, so layout helpers take one
/// argument instead of six.
pub(crate) struct Overview<'a> {
    pub(crate) snap: &'a sys::Snapshot,
    extensions: &'a [ext::Extension],
    switches: &'a [toggles::Toggle],
    selected: Option<&'a str>,
    error: Option<&'a str>,
    pub(crate) page: PageRef,
    pub(crate) proc: &'a proc::Snapshot,
    pub(crate) cpu_history: &'a [f32],
    pub(crate) proc_selected: Option<u32>,
    pub(crate) table: &'a table_state::TableState,
    pub(crate) my_uid: u32,
    /// Per-row state of the gnome-fixes extension.
    pub(crate) fixes: &'a [fixes::FixState],
    pub(crate) fixes_checked: &'a str,
    pub(crate) fixes_apply_armed: bool,
    /// Verbatim stdout and parsed state of the root-align console.
    pub(crate) root_align: &'a root_align::State,
    /// Startup layouts model + probes.
    pub(crate) startup: &'a startup::State,
    /// Snapshot and action log of the openshell panel.
    pub(crate) openshell: &'a openshell::State,
    pub(crate) content_led: &'a scroll::Led,
    pub(crate) table_led: &'a scroll::Led,
}

fn content(overview: &Overview, cx: &mut Context<App>) -> impl IntoElement {
    let weak = cx.entity().downgrade();
    let table_view = overview.page.plugin == "processes" && overview.page.view == "table";
    let body = match (overview.page.plugin, overview.page.view) {
        ("core", "overview") => overview_body(overview, &weak),
        ("core", "installed") => extensions_body(overview, &weak),
        ("processes", view) => pages::processes::body(view, overview, &weak),
        ("gnome-fixes", _) => pages::gnome_fixes::body(overview, &weak),
        ("root-align", _) => pages::root_align::body(overview, &weak),
        ("startup-layouts", view) => pages::startup_layouts::body(view, overview, &weak),
        ("openshell", view) => pages::openshell::body(view, overview, &weak),
        (_, label) => placeholder(label),
    };
    // The table owns its scroller below the header; every other page keeps the
    // shared outer one. Both track an `Led`, so the tick column and the
    // keyboard always drive whatever page is on screen.
    v().relative()
        .flex_1()
        .min_h_0()
        .child(
            v().id("content")
                .flex_1()
                .min_h_0()
                .pl(px(40.))
                .pr(px(52.))
                .pt(if table_view { px(30.) } else { px(52.) })
                .pb(if table_view { px(30.) } else { px(64.) })
                .when(!table_view, |this| {
                    this.overflow_y_scroll()
                        .track_scroll(overview.content_led.handle())
                })
                .child(body),
        )
        .when(!table_view, |this| {
            this.child(led_column("led-content", overview.content_led, 16., &weak))
        })
}

// ============================================================================
// LED scroll indicator
// ============================================================================

/// Width of the tick column's hit area. The bars are 14px; the rest is margin
/// so a 14px-wide strip is still comfortable to grab.
const LED_WIDTH: f32 = 20.;

/// The tick column for one scroll area.
///
/// `right` is the distance from the container's right edge. The column's
/// vertical extent must match the tracked scroll area (that is what makes the
/// pointer→progress mapping exact), so callers place it in a `relative()`
/// wrapper whose other child is exactly that scroller.
fn led_column(
    id: &'static str,
    led: &scroll::Led,
    right: f32,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let jump_weak = weak.clone();
    let drag_weak = weak.clone();
    let mut column = div()
        .id(id)
        .absolute()
        .top(px(0.))
        .bottom(px(0.))
        .right(px(right))
        .w(px(LED_WIDTH))
        .cursor(CursorStyle::ResizeUpDown)
        .hover(|this| this.bg(alpha(0xffffff08)))
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            let _ = jump_weak.update(cx, |app, cx| app.scrub_to(event.position.y, cx));
        })
        .on_drag(scroll::DragLed, |_, _, _, cx| {
            cx.stop_propagation();
            cx.new(|_| scroll::DragLed)
        })
        .on_drag_move(move |event: &DragMoveEvent<scroll::DragLed>, _, cx| {
            // `event.bounds` is this column's own hitbox, so the mapping stays
            // exact even though the pointer is allowed to leave the column.
            let y = event.event.position.y;
            let bounds = event.bounds;
            let _ = drag_weak.update(cx, |app, cx| app.scrub_at(y, bounds, cx));
        });
    // Nothing to indicate when the content fits; the app re-renders every
    // second, so the column appears as soon as layout says it can scroll.
    if led.is_scrollable() {
        column = column.child(scroll::ticks(led.progress()).pr(px(5.)));
    }
    column
}

impl App {
    fn on_table_view(&self) -> bool {
        self.nav.page.plugin == "processes" && self.nav.page.view == "table"
    }

    /// The scroll area the indicator and the keyboard act on.
    fn active_led(&mut self) -> &mut scroll::Led {
        if self.on_table_view() {
            &mut self.table_led
        } else {
            &mut self.content_led
        }
    }

    /// Press on the tick column: jump straight to the pointer.
    fn scrub_to(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let led = self.active_led();
        let bounds = led.handle().bounds();
        let progress = led.progress_at(y, bounds);
        led.set_progress(progress);
        cx.notify();
    }

    /// Drag on the tick column: same mapping, but against the column's own
    /// bounds, which stay valid after the pointer leaves the strip.
    fn scrub_at(&mut self, y: Pixels, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let led = self.active_led();
        let progress = led.progress_at(y, bounds);
        led.set_progress(progress);
        cx.notify();
    }

    /// Wheel-free movement. Returns whether `key` was consumed.
    fn scroll_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let led = self.active_led();
        match key {
            "up" => led.scroll_by(-56.),
            "down" => led.scroll_by(56.),
            "pageup" => led.page_by(-1.),
            "pagedown" => led.page_by(1.),
            "home" => led.to_edge(false),
            "end" => led.to_edge(true),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Root shortcut dispatch: navigation keys scroll, the rest only reaches
    /// the table view. A modal dialog swallows everything except its own keys.
    fn key_down(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.proc_dialog.is_some() {
            self.table_key(key, cx);
            return;
        }
        if matches!(key, "up" | "down" | "pageup" | "pagedown" | "home" | "end")
            && self.proc_dialog.is_none()
        {
            self.scroll_key(key, cx);
            return;
        }
        if self.on_table_view() {
            self.table_key(key, cx);
        }
    }
}

fn overview_body(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    v().child(hero(overview.snap))
        .child(metrics(overview.snap))
        .child(section_head(
            "扩展与开关",
            &format!("管理全部 {} 项 →", overview.extensions.len()),
        ))
        .child(panels(overview, weak))
        .child(section_head("资源热点", "打开任务管理器 →"))
        .child(processes())
        .child(section_head("启动器 · 常用", "编辑布局 →"))
        .child(apps())
}

/// Extensions section: the same panel, without the overview chrome.
fn extensions_body(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    v().child(section_head(
        "扩展与开关",
        &format!("管理全部 {} 项 →", overview.extensions.len()),
    ))
    .child(panels(overview, weak))
}

/// Sections whose content is not designed yet.
fn placeholder(page: &str) -> Div {
    v().pt(px(30.))
        .pb(px(80.))
        .child(caps("待设计", 10.5, FontWeight::NORMAL, dim(), 0.22))
        .child(
            txt(page, 44., FontWeight::EXTRA_LIGHT, text())
                .line_height(px(46.))
                .mt(px(14.)),
        )
        .child(
            txt(
                "这个页面的内容还没实现，下一步先出 HTML demo 选型。",
                13.,
                FontWeight::NORMAL,
                muted(),
            )
            .mt(px(12.)),
        )
}

fn hero(snap: &sys::Snapshot) -> Div {
    h().items_end()
        .justify_between()
        .mb(px(56.))
        .child(
            v().child(
                caps(
                    &format!("Session · {} · {}", snap.hostname, snap.session_kind),
                    11.,
                    FontWeight::NORMAL,
                    dim(),
                    0.26,
                )
                .mb(px(14.)),
            )
            .child(txt(snap.headline, 44., FontWeight::EXTRA_LIGHT, text()).line_height(px(46.)))
            .child(txt(snap.detail.clone(), 13., FontWeight::NORMAL, muted()).mt(px(12.))),
        )
        .child(
            h().items_center()
                .gap(px(10.))
                .child(button("扫描更新", false))
                .child(button("重启会话", true)),
        )
}

fn button(label: &'static str, solid: bool) -> Stateful<Div> {
    let (bg, border, fg) = if solid {
        (accent(), accent(), white())
    } else {
        (alpha(0x00000000), line_strong(), text())
    };
    let (hover_bg, hover_border, hover_fg) = if solid {
        (accent_hover(), accent_hover(), white())
    } else {
        (white(), white(), black())
    };
    div()
        .id(label)
        .flex()
        .items_center()
        .justify_center()
        .px(px(22.))
        .py(px(11.))
        .border(px(1.))
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .hover(move |this| {
            this.bg(hover_bg)
                .border_color(hover_border)
                .text_color(hover_fg)
        })
        .child(caps_no_color(label, 11., FontWeight::NORMAL, 0.16))
}

// ============================================================================
// Metric row
// ============================================================================

struct Metric {
    label: &'static str,
    value: String,
    unit: String,
    fill: f32,
    hot: bool,
    left: String,
    right: String,
}

fn metrics(snap: &sys::Snapshot) -> Div {
    h().items_stretch()
        .border_t(px(1.))
        .border_color(line())
        .mb(px(58.))
        .child(metric(
            0,
            Metric {
                label: "CPU 负载",
                value: snap.cpu_value.clone(),
                unit: "%".into(),
                fill: snap.cpu_fill,
                hot: snap.cpu_fill >= 0.85,
                left: snap.cpu_left.clone(),
                right: snap.cpu_right.clone(),
            },
        ))
        .child(metric(
            1,
            Metric {
                label: "内存",
                value: snap.mem_value.clone(),
                unit: snap.mem_unit.clone(),
                fill: snap.mem_fill,
                hot: snap.mem_fill >= 0.85,
                left: snap.mem_left.clone(),
                right: snap.mem_right.clone(),
            },
        ))
        .child(metric(
            2,
            Metric {
                label: "存储 · 根分区",
                value: snap.disk_value.clone(),
                unit: snap.disk_unit.clone(),
                fill: snap.disk_fill,
                hot: snap.disk_fill >= 0.8,
                left: snap.disk_left.clone(),
                right: snap.disk_right.clone(),
            },
        ))
        .child(metric(
            3,
            Metric {
                label: "网络",
                value: snap.net_value.clone(),
                unit: snap.net_unit.clone(),
                fill: snap.net_fill,
                hot: false,
                left: snap.net_left.clone(),
                right: snap.net_right.clone(),
            },
        ))
}

fn metric(index: usize, m: Metric) -> Div {
    v().w(relative(0.25))
        .min_w_0()
        .pt(px(30.))
        .pb(px(26.))
        .pr(px(28.))
        .when(index > 0, |this| {
            this.pl(px(28.)).border_l(px(1.)).border_color(line())
        })
        .child(caps(m.label, 10.5, FontWeight::NORMAL, dim(), 0.22).mb(px(22.)))
        .child(
            h().items_end()
                .gap(px(8.))
                .child(txt(m.value, 52., FontWeight::EXTRA_LIGHT, text()).line_height(px(52.)))
                .child(
                    txt(m.unit, 15., FontWeight::LIGHT, muted())
                        .line_height(px(18.))
                        .mb(px(4.)),
                ),
        )
        .child(
            div()
                .h(px(1.))
                .w_full()
                .mt(px(24.))
                .bg(line_strong())
                .child(div().h_full().w(relative(m.fill)).bg(if m.hot {
                    accent()
                } else {
                    text()
                })),
        )
        .child(
            h().justify_between()
                .mt(px(11.))
                .text_size(px(10.5))
                .text_color(dim())
                .child(txt(m.left, 10.5, FontWeight::NORMAL, dim()))
                .child(txt(m.right, 10.5, FontWeight::NORMAL, dim())),
        )
}

// ============================================================================
// Section header
// ============================================================================

fn section_head(title: &str, link: &str) -> Div {
    h().items_end()
        .justify_between()
        .mb(px(22.))
        .child(caps(title, 12., FontWeight::MEDIUM, muted(), 0.24))
        .child(caps(link, 11., FontWeight::NORMAL, dim(), 0.12))
}

// ============================================================================
// Extensions + quick toggles
// ============================================================================

fn panels(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let detail = overview
        .selected
        .and_then(|uuid| overview.extensions.iter().find(|e| e.uuid == uuid));
    h().items_stretch()
        .gap(px(1.))
        .bg(line())
        .border(px(1.))
        .border_color(line())
        .mb(px(58.))
        .child(
            v().w(relative(0.574))
                .min_w_0()
                .bg(black())
                .pt(px(30.))
                .pb(px(30.))
                .px(px(28.))
                .children(ext_rows(overview.extensions, overview.selected, weak)),
        )
        .child(match detail {
            Some(extension) => details(extension, overview.error, weak),
            None => quick_toggles(overview, weak),
        })
}

/// Right-hand panel when nothing is selected: the real system switches.
fn quick_toggles(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let last = overview.switches.len().saturating_sub(1);
    v().flex_1()
        .min_w_0()
        .bg(black())
        .pt(px(30.))
        .pb(px(30.))
        .px(px(28.))
        .children(
            overview
                .switches
                .iter()
                .enumerate()
                .map(|(index, toggle)| toggle_row(index, toggle, index == last, weak)),
        )
        .when_some(overview.error, |this, error| {
            this.child(txt(error.to_string(), 10.5, FontWeight::NORMAL, warn()).mt(px(14.)))
        })
}

fn toggle_row(
    index: usize,
    toggle: &toggles::Toggle,
    last: bool,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let (on, available, id) = (toggle.on, toggle.available, toggle.id);
    let title_color = if available { text() } else { muted() };
    let weak = weak.clone();
    h().items_center()
        .id(("toggle", index))
        .justify_between()
        .py(px(16.))
        .when(!last, |this| this.border_b(px(1.)).border_color(line()))
        .when(available, |this| {
            this.cursor_pointer()
                .hover(|this| this.bg(alpha(0xffffff0a)))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_switch(id, !on, cx));
                })
        })
        .child(
            v().child(txt(toggle.title, 13., FontWeight::NORMAL, title_color))
                .child(txt(toggle.description.clone(), 11., FontWeight::NORMAL, dim()).mt(px(4.))),
        )
        .child(switch(on, available))
}

fn switch(on: bool, available: bool) -> impl IntoElement {
    let border = if !available {
        line()
    } else if on {
        alpha(0xffffff8c)
    } else {
        line_strong()
    };
    let fill = if !available {
        alpha(0xffffff03)
    } else if on {
        alpha(0xffffff14)
    } else {
        alpha(0xffffff05)
    };
    let knob = if !available {
        dim()
    } else if on {
        text()
    } else {
        muted()
    };
    div()
        .relative()
        .w(px(46.))
        .h(px(22.))
        .flex_shrink_0()
        .rounded_full()
        .border(px(1.))
        .border_color(border)
        .bg(fill)
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(px(if on { 27. } else { 3. }))
                .w(px(14.))
                .h(px(14.))
                .rounded_full()
                .bg(knob),
        )
}

/// Detail panel: what the extension is, where it came from, and its switch.
fn details(extension: &ext::Extension, error: Option<&str>, weak: &WeakEntity<App>) -> Div {
    let (_, enabled) = extension.status();
    let (uuid, want) = (extension.uuid.clone(), !enabled);
    let toggle_weak = weak.clone();
    let close_weak = weak.clone();
    let description = if extension.description.trim().is_empty() {
        "这个扩展没有提供描述。".to_string()
    } else {
        extension.description.clone()
    };
    let shell_versions = if extension.shell_versions.is_empty() {
        "—".to_string()
    } else {
        extension.shell_versions.join(" · ")
    };
    let version = if extension.version.is_empty() {
        "—".to_string()
    } else {
        format!("v{}", extension.version)
    };
    let preferences = if extension.has_prefs { "有" } else { "无" };

    v().flex_1()
        .min_w_0()
        .bg(black())
        .pt(px(30.))
        .pb(px(30.))
        .px(px(28.))
        .child(
            h().items_center()
                .justify_between()
                .child(caps("扩展详情", 10.5, FontWeight::NORMAL, dim(), 0.22))
                .child(
                    div()
                        .id("detail-close")
                        .cursor_pointer()
                        .child(caps("关闭", 10., FontWeight::NORMAL, dim(), 0.14))
                        .hover(|this| this.text_color(text()))
                        .on_click(move |_, _, cx| {
                            let _ = close_weak.update(cx, |app, cx| app.clear_selection(cx));
                        }),
                ),
        )
        .child(
            txt(extension.name.clone(), 21., FontWeight::LIGHT, text())
                .line_height(px(26.))
                .mt(px(16.)),
        )
        .child(txt(extension.uuid.clone(), 10.5, FontWeight::NORMAL, dim()).mt(px(4.)))
        .child(div().h(px(1.)).w_full().bg(line()).mt(px(18.)).mb(px(18.)))
        .child(
            txt(description, 12., FontWeight::NORMAL, muted())
                .line_height(px(19.))
                .mb(px(18.)),
        )
        .child(info_row("版本", version))
        .child(info_row("来源", extension.origin.label().to_string()))
        .child(info_row("支持 GNOME", shell_versions))
        .child(info_row("首选项", preferences.to_string()))
        .when_some(error, |this, error| {
            this.child(txt(error.to_string(), 10.5, FontWeight::NORMAL, warn()).mt(px(14.)))
        })
        .child(
            button(
                if enabled {
                    "禁用扩展"
                } else {
                    "启用扩展"
                },
                !enabled,
            )
            .mt(px(24.))
            .on_click(move |_, _, cx| {
                let uuid = uuid.clone();
                let _ = toggle_weak.update(cx, |app, cx| {
                    app.toggle_extension(&uuid, want, cx);
                });
            }),
        )
}

fn info_row(label: &str, value: String) -> Div {
    h().items_center()
        .justify_between()
        .py(px(8.))
        .border_b(px(1.))
        .border_color(line_faint())
        .child(caps(label, 9.5, FontWeight::NORMAL, dim(), 0.2))
        .child(txt(value, 11.5, FontWeight::NORMAL, text()))
}

/// Rows for the extension panel, or a hint when the machine has none.
fn ext_rows(
    extensions: &[ext::Extension],
    selected: Option<&str>,
    weak: &WeakEntity<App>,
) -> Vec<Stateful<Div>> {
    if extensions.is_empty() {
        return vec![empty_extensions()];
    }
    let last = extensions.len() - 1;
    extensions
        .iter()
        .enumerate()
        .map(|(index, extension)| {
            let open = selected == Some(extension.uuid.as_str());
            ext_row(index, index == last, extension, open, weak)
        })
        .collect()
}

fn empty_extensions() -> Stateful<Div> {
    let dir = std::env::var("HOME")
        .map(|home| format!("{home}/.local/share/gnome-shell/extensions"))
        .unwrap_or_else(|_| "~/.local/share/gnome-shell/extensions".into());
    v().id("ext-empty")
        .py(px(24.))
        .child(txt("未检测到已安装扩展", 13., FontWeight::NORMAL, muted()))
        .child(
            txt(
                format!("放入 {dir} 后 10 秒内自动出现"),
                10.5,
                FontWeight::NORMAL,
                dim(),
            )
            .mt(px(6.)),
        )
}

fn ext_row(
    index: usize,
    last: bool,
    extension: &ext::Extension,
    open: bool,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let (status, enabled) = extension.status();
    let status_color = if enabled { ok() } else { dim() };
    let uuid = extension.uuid.clone();
    let weak = weak.clone();
    h().relative()
        .items_center()
        .id(("ext", index))
        .cursor_pointer()
        .gap(px(16.))
        .py(px(15.))
        .when(open, |this| {
            this.bg(alpha(0xffffff0a)).child(
                div()
                    .absolute()
                    .left(px(-28.))
                    .top(px(10.))
                    .bottom(px(10.))
                    .w(px(2.))
                    .bg(accent()),
            )
        })
        .hover(|this| this.bg(surface_2()))
        .on_click(move |_, _, cx| {
            let uuid = uuid.clone();
            let _ = weak.update(cx, |app, cx| app.select_extension(&uuid, cx));
        })
        .when(!last, |this| this.border_b(px(1.)).border_color(line()))
        .child(
            div()
                .w(px(30.))
                .h(px(30.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .border(px(1.))
                .border_color(line())
                .child(icons::icon(icons::PUZZLE, 14., muted())),
        )
        .child(
            v().flex_1()
                .min_w_0()
                .child(txt(extension.name.clone(), 13., FontWeight::NORMAL, text()))
                .child(
                    txt(
                        format!("{} · {}", extension.meta_line(), extension.origin.label()),
                        10.5,
                        FontWeight::NORMAL,
                        dim(),
                    )
                    .mt(px(3.)),
                ),
        )
        .child(caps(status, 9.5, FontWeight::NORMAL, status_color, 0.18))
}

// ============================================================================
// Process table
// ============================================================================

fn processes() -> Div {
    v().border(px(1.))
        .border_color(line())
        .pt(px(26.))
        .pb(px(26.))
        .px(px(28.))
        .mb(px(58.))
        .child(
            h().items_end()
                .pb(px(14.))
                .border_b(px(1.))
                .border_color(line())
                .child(
                    div()
                        .flex_1()
                        .child(caps("进程", 10., FontWeight::MEDIUM, dim(), 0.2)),
                )
                .child(
                    div()
                        .w(px(160.))
                        .child(caps("状态", 10., FontWeight::MEDIUM, dim(), 0.2)),
                )
                .child(div().w(px(162.)).flex().justify_end().child(caps(
                    "CPU",
                    10.,
                    FontWeight::MEDIUM,
                    dim(),
                    0.2,
                )))
                .child(div().w(px(162.)).flex().justify_end().child(caps(
                    "内存",
                    10.,
                    FontWeight::MEDIUM,
                    dim(),
                    0.2,
                )))
                .child(div().w(px(240.)).flex().justify_end().child(caps(
                    "趋势",
                    10.,
                    FontWeight::MEDIUM,
                    dim(),
                    0.2,
                ))),
        )
        .child(process_row(
            0,
            "gnome-shell",
            "PID 1842",
            "RUNNING",
            ok(),
            "8.4%",
            "412 MB",
            SPARK_SHELL,
        ))
        .child(process_row(
            1,
            "firefox",
            "PID 9210",
            "SLEEP",
            muted(),
            "6.1%",
            "1.8 GB",
            SPARK_FIREFOX,
        ))
        .child(process_row(
            2,
            "btrfs-scrub",
            "PID 44102",
            "IO WAIT",
            accent(),
            "2.2%",
            "86 MB",
            SPARK_SCRUB,
        ))
        .child(process_row_flat(
            3,
            "pipewire",
            "PID 2103",
            "SLEEP",
            muted(),
            "0.4%",
            "62 MB",
        ))
}

fn process_row(
    index: usize,
    name: &'static str,
    pid: &'static str,
    state: &'static str,
    state_color: Hsla,
    cpu: &'static str,
    mem: &'static str,
    spark: &'static [(f32, f32)],
) -> Div {
    process_row_base(index)
        .child(name_cell(name, pid))
        .child(
            div()
                .w(px(160.))
                .child(caps(state, 10.5, FontWeight::NORMAL, state_color, 0.14)),
        )
        .child(div().w(px(162.)).flex().justify_end().child(txt(
            cpu,
            12.5,
            FontWeight::NORMAL,
            muted(),
        )))
        .child(div().w(px(162.)).flex().justify_end().child(txt(
            mem,
            12.5,
            FontWeight::NORMAL,
            muted(),
        )))
        .child(div().w(px(240.)).flex().justify_end().child(sparkline(
            spark,
            64.,
            16.,
            rgba(0xffffffb3),
        )))
}

fn process_row_flat(
    index: usize,
    name: &'static str,
    pid: &'static str,
    state: &'static str,
    state_color: Hsla,
    cpu: &'static str,
    mem: &'static str,
) -> Div {
    process_row_base(index)
        .child(name_cell(name, pid))
        .child(
            div()
                .w(px(160.))
                .child(caps(state, 10.5, FontWeight::NORMAL, state_color, 0.14)),
        )
        .child(div().w(px(162.)).flex().justify_end().child(txt(
            cpu,
            12.5,
            FontWeight::NORMAL,
            muted(),
        )))
        .child(div().w(px(162.)).flex().justify_end().child(txt(
            mem,
            12.5,
            FontWeight::NORMAL,
            muted(),
        )))
        .child(
            div()
                .w(px(240.))
                .flex()
                .justify_end()
                .child(div().w(px(64.)).h(px(1.)).bg(rgba(0xffffff59))),
        )
}

fn process_row_base(index: usize) -> Div {
    h().items_center().py(px(14.)).when(index < 3, |this| {
        this.border_b(px(1.)).border_color(line_faint())
    })
}

fn name_cell(name: &'static str, pid: &'static str) -> Div {
    h().flex_1()
        .min_w_0()
        .items_center()
        .gap(px(8.))
        .child(txt(name, 12.5, FontWeight::NORMAL, text()))
        .child(txt(pid, 10.5, FontWeight::NORMAL, dim()))
}

// ============================================================================
// Sparkline
// ============================================================================

const SPARK_SHELL: &[(f32, f32)] = &[
    (0., 12.),
    (8., 10.),
    (16., 11.),
    (24., 7.),
    (32., 9.),
    (40., 6.),
    (48., 8.),
    (56., 5.),
    (64., 6.),
];
const SPARK_FIREFOX: &[(f32, f32)] = &[
    (0., 4.),
    (8., 6.),
    (16., 3.),
    (24., 5.),
    (32., 8.),
    (40., 7.),
    (48., 9.),
    (56., 8.),
    (64., 10.),
];
const SPARK_SCRUB: &[(f32, f32)] = &[
    (0., 14.),
    (8., 13.),
    (16., 14.),
    (24., 11.),
    (32., 13.),
    (40., 10.),
    (48., 12.),
    (56., 9.),
    (64., 11.),
];

fn sparkline(
    points: &'static [(f32, f32)],
    width: f32,
    height: f32,
    color: gpui_kit::Rgba,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window: &mut Window, _cx: &mut gpui_kit::App| {
            let sx = bounds.size.width.as_f32() / width;
            let sy = bounds.size.height.as_f32() / height;
            let mut builder = PathBuilder::stroke(px(1.));
            for (i, (x, y)) in points.iter().enumerate() {
                let p = point(bounds.origin.x + px(x * sx), bounds.origin.y + px(y * sy));
                if i == 0 {
                    builder.move_to(p);
                } else {
                    builder.line_to(p);
                }
            }
            if let Ok(path) = builder.build() {
                window.paint_path(path, color);
            }
        },
    )
    .w(px(width))
    .h(px(height))
}

// ============================================================================
// Launcher tiles
// ============================================================================

const APPS: [(&str, &str); 6] = [
    ("FF", "Firefox"),
    ("TM", "Terminal"),
    ("CD", "Code"),
    ("FI", "Files"),
    ("SP", "Spotify"),
    ("＋", "添加"),
];

fn apps() -> Div {
    h().items_stretch()
        .gap(px(1.))
        .bg(line())
        .border(px(1.))
        .border_color(line())
        .children(APPS.iter().enumerate().map(|(i, (mark, name))| {
            v().id(("app", i))
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(12.))
                .py(px(24.))
                .px(px(18.))
                .bg(black())
                .hover(|this| this.bg(surface_2()))
                .child(
                    div()
                        .w(px(42.))
                        .h(px(42.))
                        .flex_shrink_0()
                        .rounded_full()
                        .border(px(1.))
                        .border_color(line_strong())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(txt(*mark, 13., FontWeight::NORMAL, muted())),
                )
                .child(caps(name, 10.5, FontWeight::NORMAL, muted(), 0.1))
        }))
}

// ============================================================================
// Tracked text without a baked color (inherits from the parent)
// ============================================================================

fn caps_no_color(raw: &str, size: f32, weight: FontWeight, em: f32) -> Div {
    let gap = size * em;
    let mut row = h().items_center().flex_shrink_0();
    for ch in raw.to_uppercase().chars() {
        row = row.child(
            div()
                .mr(px(gap))
                .text_size(px(size))
                .line_height(px(size * 1.538))
                .font_weight(weight)
                .child(ch.to_string()),
        );
    }
    row
}

// ============================================================================

/// `CARGO_PKG_VERSION` is the single source of truth for `--version`; no
/// release tag or manifest needs to be duplicated in the UI.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("gnous {VERSION}");
        return;
    }
    if let Some(arg) = args.iter().find(|arg| arg.starts_with('-')) {
        eprintln!("gnous {VERSION}\n未知参数: {arg}\n用法: gnous [--version]");
        std::process::exit(2);
    }
    let bounds = Bounds {
        origin: point(px(120.), px(80.)),
        size: size(px(1440.), px(1000.)),
    };

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_decorations: Some(WindowDecorations::Client),
                titlebar: Some(TitlebarOptions {
                    title: Some("GNOUS".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(App::new),
        )
        .expect("failed to open window");
    });
}
