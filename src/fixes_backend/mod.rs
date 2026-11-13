//! Versioned, in-process backend for the two GNOUS-owned desktop fixes.
//!
//! `clipfix` and `root-align` used to be thin shells around two Rust CLIs
//! under `/home/root/GNOME`. Their behaviour now lives here: one small
//! `run(tool, op)` interface, structured facts, JSON state/audit, and no
//! dependency on those source directories. The remaining three
//! `gnome-fixes` tools are still external adapters and are deliberately
//! outside this module.

pub mod audit;
pub mod clipfix;
pub mod process;
pub mod root_align;
pub mod state;
mod wayland;
mod x11;

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    Clipfix,
    RootAlign,
}

impl Tool {
    pub fn id(self) -> &'static str {
        match self {
            Tool::Clipfix => "clipfix",
            Tool::RootAlign => "root-align",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Op {
    Check,
    Status,
    Doctor,
    Apply,
    Revert,
    RestartShell,
}

impl Op {
    fn id(self) -> &'static str {
        match self {
            Op::Check => "check",
            Op::Status => "status",
            Op::Doctor => "doctor",
            Op::Apply => "apply",
            Op::Revert => "revert",
            Op::RestartShell => "restart-shell",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub tool: Tool,
    pub op: Op,
    pub ok: bool,
    pub output: String,
    pub command: String,
    /// Structured source of truth for UI state. Renderers display `output`
    /// but never derive health by parsing it.
    pub facts: Value,
}

impl Outcome {
    fn finish(mut self) -> Self {
        let audit_file = state::audit_file().to_string_lossy().into_owned();
        if let Some(map) = self.facts.as_object_mut() {
            map.insert("backend".into(), json!("gnous-builtin"));
            map.insert("audit_file".into(), json!(audit_file));
        }
        if let Err(error) = audit::append(&self) {
            self.output
                .push_str(&format!("\n⚠ 审计日志写入失败：{error}"));
        }
        self
    }

    fn invalid(tool: Tool, op: Op) -> Self {
        let error = format!("内建后端不支持组合：{} / {}", tool.id(), op.id());
        outcome(
            tool,
            op,
            false,
            format!("✗ {error}"),
            json!({"status": "failed", "error": error}),
        )
    }
}

pub fn command_for(tool: Tool, op: Op) -> String {
    format!("gnous builtin {} / {}", tool.id(), op.id())
}

pub(crate) fn outcome(tool: Tool, op: Op, ok: bool, output: String, facts: Value) -> Outcome {
    Outcome {
        tool,
        op,
        ok,
        output,
        command: command_for(tool, op),
        facts,
    }
}

/// The shared seam. Callers learn six operation names and get real, structured
/// results; all command invocation, migration, rollback and auditing stays
/// behind this interface.
pub fn run(tool: Tool, op: Op) -> Outcome {
    match (tool, op) {
        (Tool::Clipfix, Op::Check | Op::Apply | Op::Revert) => clipfix::run(op).finish(),
        (
            Tool::RootAlign,
            Op::Check | Op::Status | Op::Doctor | Op::Apply | Op::Revert | Op::RestartShell,
        ) => root_align::run(op).finish(),
        _ => Outcome::invalid(tool, op).finish(),
    }
}

pub fn backend_dir() -> PathBuf {
    state::state_dir()
}

pub fn path_for(name: &str) -> PathBuf {
    state::path_for(name)
}

pub fn audit_file() -> PathBuf {
    state::audit_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_commands_never_reference_external_tool_directories() {
        for tool in [Tool::Clipfix, Tool::RootAlign] {
            for op in [
                Op::Check,
                Op::Status,
                Op::Doctor,
                Op::Apply,
                Op::Revert,
                Op::RestartShell,
            ] {
                let command = command_for(tool, op);
                assert!(command.starts_with("gnous builtin "));
                assert!(!command.contains("/home/root/GNOME"));
                assert!(!command.contains(".sh"));
                assert!(!command.contains("--now"));
            }
        }
    }
}
