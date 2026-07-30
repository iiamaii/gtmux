//! `gtmux layout|terminal|workspace|fs` + online `session create|delete` —
//! the ADR-0053 remote-control surface (Batch C, plan-0023).
//!
//! Every command here talks to the *live* server over HTTP (ADR-0053 D5/D6:
//! layout mutation goes through `POST /api/sessions/{name}/layout/ops`,
//! never the layout file on disk — the offline `gtmux session ls|export|
//! import` family in `main.rs` stays the only direct-file reader per
//! ADR-0044 D-C2). Wire op names are snake_case (`raise_top`,
//! `group_create` — `crates/http-api/src/layout_ops.rs` is canonical);
//! the hyphenated CLI verbs map onto them here (ADR-0053 D2 / impl notes).
//!
//! Shared conventions (D2/D3/D4):
//! * `--session` falls back to `$GTMUX_CANVAS_SESSION` (injected into every
//!   gtmux-spawned shell — D4). Session-*lifecycle* commands (`workspace`,
//!   `session create|delete`) require an explicit session and have no env
//!   fallback (user decision — no implicit session target).
//! * Targets resolve as UUID first, else exact-label match with ambiguity
//!   listing (D3).
//! * Plain text by default, `--json` for machine output; errors are one
//!   stderr line with the server's machine-readable code + non-zero exit.

use std::io::IsTerminal;
use std::process::ExitCode;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use clap::{Args, Subcommand};
use gtmux_http_api::{Item, Layout, PathEndpoint};
use serde_json::{json, Map, Value};

use crate::align::{compute_deltas, AlignBox, AlignMode};
use crate::http::{connect, Client, CliError, MultipartBody};

// ─────────────────────────────────────────────────────────────────────────────
// Shared clap fragments
// ─────────────────────────────────────────────────────────────────────────────

/// Instance + canvas-session selectors shared by the item-scoped commands.
#[derive(Debug, Args)]
pub struct Ctx {
    /// Server instance (default: $GTMUX_SERVER_INSTANCE, else the single
    /// running instance).
    #[arg(long, env = "GTMUX_SERVER_INSTANCE", value_name = "INSTANCE")]
    pub instance: Option<String>,
    /// Canvas session (default: $GTMUX_CANVAS_SESSION — injected into
    /// gtmux-spawned terminals, ADR-0053 D4).
    #[arg(long, env = "GTMUX_CANVAS_SESSION", value_name = "SESSION")]
    pub session: Option<String>,
}

/// Instance selector for commands that are not session-scoped
/// (`terminal kill|ls`, `workspace`, `session create|delete`, `fs`).
#[derive(Debug, Args)]
pub struct InstanceOpt {
    /// Server instance (default: $GTMUX_SERVER_INSTANCE, else the single
    /// running instance).
    #[arg(long, env = "GTMUX_SERVER_INSTANCE", value_name = "INSTANCE")]
    pub instance: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// `gtmux layout …`
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Subcommand)]
pub enum LayoutCmd {
    /// List items (and groups) of a session layout.
    List {
        #[command(flatten)]
        ctx: Ctx,
        /// Emit the full layout JSON instead of the summary table.
        #[arg(long)]
        json: bool,
    },
    /// Show one item in detail.
    Get {
        /// Item UUID or exact label.
        target: String,
        #[command(flatten)]
        ctx: Ctx,
        /// Emit the item's full JSON payload.
        #[arg(long)]
        json: bool,
    },
    /// List path connections touching an item (ADR-0053 D13 — CLI-side).
    Connections {
        /// Item UUID or exact label (a path target lists its own endpoints).
        target: String,
        #[command(flatten)]
        ctx: Ctx,
        #[arg(long)]
        json: bool,
    },
    /// Move an item to an absolute canvas position.
    Move {
        target: String,
        #[arg(long, allow_hyphen_values = true)]
        x: f64,
        #[arg(long, allow_hyphen_values = true)]
        y: f64,
        /// Apply to a locked item (ADR-0053 D6).
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Resize an item (line/path/free_draw are endpoint-derived — rejected).
    Resize {
        target: String,
        #[arg(long, allow_hyphen_values = true)]
        w: f64,
        #[arg(long, allow_hyphen_values = true)]
        h: f64,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Make an item or group visible.
    Show {
        target: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Hide an item or group.
    Hide {
        target: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Minimize an item (terminal/note/document/snippets/web_view only).
    Minimize {
        target: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Restore a minimized item.
    Restore {
        target: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Set (or clear) an item/group label.
    Label {
        target: String,
        /// New label text. Omit together with --clear to clear.
        text: Option<String>,
        /// Clear the label.
        #[arg(long, conflicts_with = "text")]
        clear: bool,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Raise one z step (ADR-0024 forward).
    Raise {
        target: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Raise to the top of the sibling level (ADR-0024 front).
    RaiseTop {
        target: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Lower one z step (ADR-0024 backward).
    Lower {
        target: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Lower to the bottom of the sibling level (ADR-0024 back).
    LowerBottom {
        target: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Edit an item's type payload (partial merge — ADR-0053 D10).
    /// Geometry/visibility/lock/label/z have dedicated commands.
    Edit {
        target: String,
        /// Payload field as K=V (repeatable). V is parsed as JSON, falling
        /// back to a plain string (`--set text=hello --set font_size=20`).
        #[arg(long = "set", value_name = "K=V")]
        set: Vec<String>,
        /// Raw JSON object of payload fields (for nested values).
        #[arg(long = "json", value_name = "FIELDS", conflicts_with = "set")]
        json: Option<String>,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Create a non-terminal item (id is server-issued; terminals use
    /// `gtmux terminal spawn`).
    Create {
        /// Item type: text|note|rect|ellipse|line|free_draw|image|document|
        /// web_view|file_path|path|snippets.
        #[arg(value_name = "TYPE")]
        item_type: String,
        #[arg(long, allow_hyphen_values = true)]
        x: Option<f64>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<f64>,
        #[arg(long, allow_hyphen_values = true)]
        w: Option<f64>,
        #[arg(long, allow_hyphen_values = true)]
        h: Option<f64>,
        /// Payload field as K=V (repeatable).
        #[arg(long = "set", value_name = "K=V")]
        set: Vec<String>,
        /// Raw JSON object of payload fields.
        #[arg(long = "json", value_name = "FIELDS", conflicts_with = "set")]
        json: Option<String>,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Delete an item (panel-only by default — a terminal's PTY returns to
    /// the pool; --kill-terminal also SIGTERMs it).
    Delete {
        target: String,
        #[arg(long = "kill-terminal")]
        kill_terminal: bool,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// POST a raw ops array (ADR-0053 D5 batch — atomic).
    Batch {
        /// JSON array of wire ops, e.g.
        /// '[{"op":"move","id":"…","x":0,"y":0}]'.
        #[arg(long = "json", value_name = "OPS_JSON")]
        json: String,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Align or distribute items (CLI-side math → one atomic batch of
    /// move ops — ADR-0053 D13 / ADR-0027).
    Align {
        /// left|right|top|bottom|center-h|center-v|distribute-h|distribute-v
        mode: AlignMode,
        /// Two or more item targets (three or more for distribute).
        #[arg(required = true, num_args = 1..)]
        targets: Vec<String>,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Group operations (create/ungroup/reparent — ADR-0053 D13).
    Group {
        #[command(subcommand)]
        command: GroupCmd,
    },
}

#[derive(Debug, Subcommand)]
pub enum GroupCmd {
    /// Group one or more items/groups under a new group.
    Create {
        #[arg(required = true, num_args = 1..)]
        targets: Vec<String>,
        /// Group label (default: "Group N").
        #[arg(long)]
        label: Option<String>,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Dissolve a group, promoting its children (non-destructive).
    Ungroup {
        /// Group UUID or exact label.
        group: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Move an item/group under another group (or back to the canvas root).
    Reparent {
        target: String,
        /// Target group (UUID or exact label), or the literal `root`.
        #[arg(long, value_name = "GROUP|root")]
        parent: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// `gtmux terminal …`
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Subcommand)]
pub enum TerminalCmd {
    /// Spawn a fresh PTY terminal and persist its panel (headless-complete —
    /// ADR-0053 D11).
    Spawn {
        #[arg(long, allow_hyphen_values = true)]
        x: Option<f64>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<f64>,
        #[arg(long, allow_hyphen_values = true)]
        w: Option<f64>,
        #[arg(long, allow_hyphen_values = true)]
        h: Option<f64>,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Mount an existing alive pool terminal as a panel (no spawn).
    Mount {
        /// Pool terminal UUID (`gtmux terminal ls`).
        uuid: String,
        #[arg(long, allow_hyphen_values = true)]
        x: Option<f64>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<f64>,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// Remove a terminal panel; the PTY stays alive in the pool
    /// (= `layout delete` with kill_terminal=false).
    Unmount {
        /// Terminal panel UUID or exact label.
        target: String,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ctx: Ctx,
    },
    /// SIGTERM a pool terminal (POST /api/terminals/:id/kill).
    Kill {
        /// Pool terminal UUID or exact label.
        target: String,
        #[command(flatten)]
        instance: InstanceOpt,
    },
    /// List the alive terminal pool (GET /api/terminals).
    Ls {
        #[command(flatten)]
        instance: InstanceOpt,
        #[arg(long)]
        json: bool,
    },
    /// Read a pool terminal's recent output (GET /api/terminals/:id/output).
    /// Default output is ANSI-stripped text (LLM-readable); the raw ring is
    /// lossy (128 KiB drop-oldest) and may contain escape sequences.
    Read {
        /// Pool terminal UUID or exact label (`gtmux terminal ls`).
        target: String,
        /// Return only the last N bytes of the ring snapshot.
        #[arg(long)]
        tail: Option<usize>,
        /// Emit the raw PTY bytes verbatim instead of ANSI-stripped text.
        #[arg(long)]
        raw: bool,
        #[command(flatten)]
        instance: InstanceOpt,
    },
    /// Send input to a pool terminal (POST /api/terminals/:id/input). Raw
    /// stdin injection — no shell escaping. Beware self-injection: sending to
    /// `$GTMUX_TERMINAL_ID` pollutes your own input stream.
    Send {
        /// Pool terminal UUID or exact label.
        target: String,
        /// Text to send. Submitted by a separate CR (Enter) write after a short
        /// delay (ADR-0054 D4 amend — reliable for raw-mode agent TUIs) unless
        /// `--no-enter`. Omit when using `--bytes`.
        text: Option<String>,
        /// Do not submit — send the keystrokes only (no trailing CR).
        #[arg(long = "no-enter")]
        no_enter: bool,
        /// Send raw control bytes as hex (e.g. `03` = Ctrl-C, `1b5b41` = Up).
        /// Mutually exclusive with the positional text.
        #[arg(long, value_name = "HEX")]
        bytes: Option<String>,
        #[command(flatten)]
        instance: InstanceOpt,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// `gtmux workspace …`
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Subcommand)]
pub enum WorkspaceCmd {
    /// Re-point a session's Workspace(B) root (PUT /workspace — ADR-0046 D8).
    Set {
        /// New workspace directory (must exist; resolved to an absolute path).
        path: String,
        /// Canvas session (explicit — no env fallback for session-level
        /// commands, ADR-0053 D2).
        #[arg(long, value_name = "SESSION")]
        session: String,
        #[command(flatten)]
        instance: InstanceOpt,
    },
    /// Print a session's effective Workspace(B) root.
    Get {
        #[arg(long, value_name = "SESSION")]
        session: String,
        #[command(flatten)]
        instance: InstanceOpt,
        #[arg(long)]
        json: bool,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// `gtmux fs …`
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OnConflict {
    Rename,
    Overwrite,
}

impl OnConflict {
    fn wire(self) -> &'static str {
        match self {
            OnConflict::Rename => "rename",
            OnConflict::Overwrite => "overwrite",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum FsCmd {
    /// Upload a local file into the workspace (POST /api/fs/upload —
    /// ADR-0053 D14: agent-visible filesystem → workspace import).
    Upload {
        /// Source file on this host's filesystem.
        src: std::path::PathBuf,
        /// Destination directory — session-workspace-relative, or absolute
        /// (inside the Server Workspace).
        #[arg(long, value_name = "DIR")]
        dir: String,
        /// Conflict policy (default: reject with 409 name_conflict).
        #[arg(long = "on-conflict", value_enum)]
        on_conflict: Option<OnConflict>,
        #[command(flatten)]
        ctx: Ctx,
        #[arg(long)]
        json: bool,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry points (ExitCode wrappers)
// ─────────────────────────────────────────────────────────────────────────────

fn finish_cmd(ctx_label: &str, result: Result<(), CliError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            e.print(ctx_label);
            ExitCode::from(e.exit_code())
        }
    }
}

pub fn run_layout(cmd: LayoutCmd) -> ExitCode {
    let label = layout_cmd_label(&cmd);
    finish_cmd(label, layout_dispatch(cmd))
}

pub fn run_terminal(cmd: TerminalCmd) -> ExitCode {
    let label = match &cmd {
        TerminalCmd::Spawn { .. } => "terminal spawn",
        TerminalCmd::Mount { .. } => "terminal mount",
        TerminalCmd::Unmount { .. } => "terminal unmount",
        TerminalCmd::Kill { .. } => "terminal kill",
        TerminalCmd::Ls { .. } => "terminal ls",
        TerminalCmd::Read { .. } => "terminal read",
        TerminalCmd::Send { .. } => "terminal send",
    };
    finish_cmd(label, terminal_dispatch(cmd))
}

pub fn run_workspace(cmd: WorkspaceCmd) -> ExitCode {
    let label = match &cmd {
        WorkspaceCmd::Set { .. } => "workspace set",
        WorkspaceCmd::Get { .. } => "workspace get",
    };
    finish_cmd(label, workspace_dispatch(cmd))
}

pub fn run_fs(cmd: FsCmd) -> ExitCode {
    finish_cmd("fs upload", fs_dispatch(cmd))
}

/// `gtmux session create` (online — ADR-0053 D12; gate = D6).
pub fn run_session_create(
    name: String,
    workspace: Option<String>,
    yes: bool,
    password: Option<String>,
    instance: Option<String>,
) -> ExitCode {
    finish_cmd(
        "session create",
        session_create(name, workspace, yes, password, instance),
    )
}

/// `gtmux session delete` (online — ADR-0053 D12; gate = D6).
pub fn run_session_delete(
    name: String,
    yes: bool,
    password: Option<String>,
    instance: Option<String>,
) -> ExitCode {
    finish_cmd("session delete", session_delete(name, yes, password, instance))
}

fn layout_cmd_label(cmd: &LayoutCmd) -> &'static str {
    match cmd {
        LayoutCmd::List { .. } => "layout list",
        LayoutCmd::Get { .. } => "layout get",
        LayoutCmd::Connections { .. } => "layout connections",
        LayoutCmd::Move { .. } => "layout move",
        LayoutCmd::Resize { .. } => "layout resize",
        LayoutCmd::Show { .. } => "layout show",
        LayoutCmd::Hide { .. } => "layout hide",
        LayoutCmd::Minimize { .. } => "layout minimize",
        LayoutCmd::Restore { .. } => "layout restore",
        LayoutCmd::Label { .. } => "layout label",
        LayoutCmd::Raise { .. } => "layout raise",
        LayoutCmd::RaiseTop { .. } => "layout raise-top",
        LayoutCmd::Lower { .. } => "layout lower",
        LayoutCmd::LowerBottom { .. } => "layout lower-bottom",
        LayoutCmd::Edit { .. } => "layout edit",
        LayoutCmd::Create { .. } => "layout create",
        LayoutCmd::Delete { .. } => "layout delete",
        LayoutCmd::Batch { .. } => "layout batch",
        LayoutCmd::Align { .. } => "layout align",
        LayoutCmd::Group {
            command: GroupCmd::Create { .. },
        } => "layout group create",
        LayoutCmd::Group {
            command: GroupCmd::Ungroup { .. },
        } => "layout group ungroup",
        LayoutCmd::Group {
            command: GroupCmd::Reparent { .. },
        } => "layout group reparent",
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layout dispatch
// ─────────────────────────────────────────────────────────────────────────────

fn layout_dispatch(cmd: LayoutCmd) -> Result<(), CliError> {
    match cmd {
        LayoutCmd::List { ctx, json } => {
            let (client, session) = open(ctx)?;
            let layout = fetch_layout(&client, &session)?;
            if json {
                print_json(&serde_json::to_value(&layout).unwrap_or(Value::Null));
            } else {
                print_layout_table(&layout);
            }
            Ok(())
        }
        LayoutCmd::Get { target, ctx, json } => {
            let (client, session) = open(ctx)?;
            let layout = fetch_layout(&client, &session)?;
            let id = resolve_target(&layout, &target, TargetDomain::Item)?;
            let item = find_item(&layout, &id).expect("resolved id exists");
            if json {
                print_json(&serde_json::to_value(item).unwrap_or(Value::Null));
            } else {
                print_item_detail(item);
            }
            Ok(())
        }
        LayoutCmd::Connections { target, ctx, json } => {
            let (client, session) = open(ctx)?;
            let layout = fetch_layout(&client, &session)?;
            let id = resolve_target(&layout, &target, TargetDomain::Item)?;
            let rows = connections_for(&layout, &id);
            if json {
                print_json(&Value::Array(rows));
            } else if rows.is_empty() {
                println!("no path connections for {id}");
            } else {
                for row in &rows {
                    println!("{}", connection_row_text(row));
                }
            }
            Ok(())
        }
        LayoutCmd::Move {
            target,
            x,
            y,
            force,
            ctx,
        } => single_op(ctx, TargetDomain::Item, &target, move |id| {
            json!({ "op": "move", "id": id, "x": x, "y": y, "force": force })
        }),
        LayoutCmd::Resize {
            target,
            w,
            h,
            force,
            ctx,
        } => single_op(ctx, TargetDomain::Item, &target, move |id| {
            json!({ "op": "resize", "id": id, "w": w, "h": h, "force": force })
        }),
        LayoutCmd::Show { target, force, ctx } => {
            single_op(ctx, TargetDomain::ItemOrGroup, &target, move |id| {
                json!({ "op": "show", "id": id, "force": force })
            })
        }
        LayoutCmd::Hide { target, force, ctx } => {
            single_op(ctx, TargetDomain::ItemOrGroup, &target, move |id| {
                json!({ "op": "hide", "id": id, "force": force })
            })
        }
        LayoutCmd::Minimize { target, force, ctx } => {
            single_op(ctx, TargetDomain::Item, &target, move |id| {
                json!({ "op": "minimize", "id": id, "force": force })
            })
        }
        LayoutCmd::Restore { target, force, ctx } => {
            single_op(ctx, TargetDomain::Item, &target, move |id| {
                json!({ "op": "restore", "id": id, "force": force })
            })
        }
        LayoutCmd::Label {
            target,
            text,
            clear,
            force,
            ctx,
        } => {
            if text.is_none() && !clear {
                return Err(CliError::local("provide <text> or --clear"));
            }
            let label = if clear { Value::Null } else { json!(text) };
            single_op(ctx, TargetDomain::ItemOrGroup, &target, move |id| {
                json!({ "op": "label", "id": id, "label": label, "force": force })
            })
        }
        LayoutCmd::Raise { target, force, ctx } => z_op(ctx, &target, "raise", force),
        LayoutCmd::RaiseTop { target, force, ctx } => z_op(ctx, &target, "raise_top", force),
        LayoutCmd::Lower { target, force, ctx } => z_op(ctx, &target, "lower", force),
        LayoutCmd::LowerBottom { target, force, ctx } => z_op(ctx, &target, "lower_bottom", force),
        LayoutCmd::Edit {
            target,
            set,
            json,
            force,
            ctx,
        } => {
            let fields = fields_from_args(&set, json.as_deref())?.ok_or_else(|| {
                CliError::local("provide --set K=V and/or --json '<fields>'")
            })?;
            single_op(ctx, TargetDomain::Item, &target, move |id| {
                json!({ "op": "edit", "id": id, "fields": fields, "force": force })
            })
        }
        LayoutCmd::Create {
            item_type,
            x,
            y,
            w,
            h,
            set,
            json,
            ctx,
        } => {
            let fields = fields_from_args(&set, json.as_deref())?;
            let op = build_create_op(&item_type, x, y, w, h, fields);
            let (client, session) = open(ctx)?;
            let resp = post_ops(&client, &session, vec![op])?;
            print_ops_result(&resp);
            Ok(())
        }
        LayoutCmd::Delete {
            target,
            kill_terminal,
            force,
            ctx,
        } => single_op(ctx, TargetDomain::Item, &target, move |id| {
            json!({
                "op": "delete", "id": id,
                "kill_terminal": kill_terminal, "force": force,
            })
        }),
        LayoutCmd::Batch { json, ctx } => {
            let ops: Value = serde_json::from_str(&json)
                .map_err(|e| CliError::Local(format!("--json is not valid JSON: {e}")))?;
            let Value::Array(ops) = ops else {
                return Err(CliError::local("--json must be a JSON *array* of ops"));
            };
            let (client, session) = open(ctx)?;
            let resp = post_ops(&client, &session, ops)?;
            print_ops_result(&resp);
            Ok(())
        }
        LayoutCmd::Align { mode, targets, ctx } => {
            let (client, session) = open(ctx)?;
            let layout = fetch_layout(&client, &session)?;
            let ops = build_align_ops(&layout, &targets, mode)?;
            if ops.is_empty() {
                println!("already aligned — nothing to move");
                return Ok(());
            }
            let resp = post_ops(&client, &session, ops)?;
            print_ops_result(&resp);
            Ok(())
        }
        LayoutCmd::Group { command } => group_dispatch(command),
    }
}

fn group_dispatch(cmd: GroupCmd) -> Result<(), CliError> {
    match cmd {
        GroupCmd::Create {
            targets,
            label,
            ctx,
        } => {
            let (client, session) = open(ctx)?;
            let layout = fetch_layout(&client, &session)?;
            let ids = resolve_many(&layout, &targets, TargetDomain::ItemOrGroup)?;
            let mut op = obj(&[("op", json!("group_create")), ("ids", json!(ids))]);
            if let Some(l) = label {
                op.insert("label".into(), json!(l));
            }
            let resp = post_ops(&client, &session, vec![Value::Object(op)])?;
            print_ops_result(&resp);
            Ok(())
        }
        GroupCmd::Ungroup { group, force, ctx } => {
            let (client, session) = open(ctx)?;
            let layout = fetch_layout(&client, &session)?;
            let gid = resolve_target(&layout, &group, TargetDomain::Group)?;
            let resp = post_ops(
                &client,
                &session,
                vec![json!({ "op": "ungroup", "group_id": gid, "force": force })],
            )?;
            print_ops_result(&resp);
            Ok(())
        }
        GroupCmd::Reparent {
            target,
            parent,
            force,
            ctx,
        } => {
            let (client, session) = open(ctx)?;
            let layout = fetch_layout(&client, &session)?;
            let id = resolve_target(&layout, &target, TargetDomain::ItemOrGroup)?;
            let parent_id = if parent == "root" {
                Value::Null
            } else {
                json!(resolve_target(&layout, &parent, TargetDomain::Group)?)
            };
            let resp = post_ops(
                &client,
                &session,
                vec![json!({
                    "op": "reparent", "id": id,
                    "parent_id": parent_id, "force": force,
                })],
            )?;
            print_ops_result(&resp);
            Ok(())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Terminal dispatch
// ─────────────────────────────────────────────────────────────────────────────

fn terminal_dispatch(cmd: TerminalCmd) -> Result<(), CliError> {
    match cmd {
        TerminalCmd::Spawn { x, y, w, h, ctx } => {
            let (client, session) = open(ctx)?;
            let mut op = obj(&[("op", json!("spawn"))]);
            insert_opt_f64(&mut op, "x", x);
            insert_opt_f64(&mut op, "y", y);
            insert_opt_f64(&mut op, "w", w);
            insert_opt_f64(&mut op, "h", h);
            let resp = post_ops(&client, &session, vec![Value::Object(op)])?;
            print_ops_result(&resp);
            Ok(())
        }
        TerminalCmd::Mount { uuid, x, y, ctx } => {
            let (client, session) = open(ctx)?;
            let mut op = obj(&[("op", json!("mount")), ("uuid", json!(uuid))]);
            insert_opt_f64(&mut op, "x", x);
            insert_opt_f64(&mut op, "y", y);
            let resp = post_ops(&client, &session, vec![Value::Object(op)])?;
            print_ops_result(&resp);
            Ok(())
        }
        TerminalCmd::Unmount { target, force, ctx } => {
            single_op(ctx, TargetDomain::TerminalItem, &target, move |id| {
                json!({
                    "op": "delete", "id": id,
                    "kill_terminal": false, "force": force,
                })
            })
        }
        TerminalCmd::Kill { target, instance } => {
            let client = connect(instance.instance)?;
            let uuid = resolve_pool_terminal(&client, &target)?;
            client.send_empty("POST", &format!("/api/terminals/{uuid}/kill"), &[])?;
            println!("killed {uuid}");
            Ok(())
        }
        TerminalCmd::Ls { instance, json } => {
            let client = connect(instance.instance)?;
            let rows = client.get_json("/api/terminals")?;
            if json {
                print_json(&rows);
                return Ok(());
            }
            let Some(rows) = rows.as_array() else {
                return Err(CliError::local("unexpected /api/terminals response shape"));
            };
            if rows.is_empty() {
                println!("(no alive terminals)");
                return Ok(());
            }
            println!("{:<38}{:<18}{:<8}SESSIONS", "UUID", "LABEL", "ATTACH");
            for row in rows {
                let id = row.get("id").and_then(Value::as_str).unwrap_or("?");
                let label = row.get("label").and_then(Value::as_str).unwrap_or("");
                let attach = row.get("attach_count").and_then(Value::as_u64).unwrap_or(0);
                let sessions: Vec<&str> = row
                    .get("attached_sessions")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                println!(
                    "{:<38}{:<18}{:<8}{}",
                    id,
                    crate::truncate(label, 17),
                    attach,
                    sessions.join(",")
                );
            }
            Ok(())
        }
        TerminalCmd::Read {
            target,
            tail,
            raw,
            instance,
        } => {
            let client = connect(instance.instance)?;
            let uuid = resolve_pool_terminal(&client, &target)?;
            let mut path = format!("/api/terminals/{uuid}/output");
            if let Some(n) = tail {
                path.push_str(&format!("?tail={n}"));
            }
            let resp = client.get_json(&path)?;
            let b64 = resp
                .get("bytes_base64")
                .and_then(Value::as_str)
                .ok_or_else(|| CliError::local("unexpected output response shape"))?;
            let bytes = BASE64
                .decode(b64.as_bytes())
                .map_err(|e| CliError::Local(format!("server returned invalid base64: {e}")))?;
            if raw {
                use std::io::Write;
                std::io::stdout().write_all(&bytes).ok();
            } else {
                // ANSI strip is client-side (server keeps raw bytes). Lossy
                // bytes are rendered leniently so a mid-sequence ring cut
                // doesn't abort the read.
                print!("{}", crate::ansi::strip_ansi(&String::from_utf8_lossy(&bytes)));
            }
            Ok(())
        }
        TerminalCmd::Send {
            target,
            text,
            no_enter,
            bytes,
            instance,
        } => {
            let plan = plan_send(text, bytes, no_enter).map_err(CliError::Local)?;
            let client = connect(instance.instance)?;
            let uuid = resolve_pool_terminal(&client, &target)?;
            match plan {
                SendPlan::Single(payload) => {
                    let sent = post_input(&client, &uuid, &payload)?;
                    println!("sent {sent} bytes to {uuid}");
                }
                SendPlan::Submit(text_bytes) => {
                    // ADR-0054 D4 amend (2026-07-30): submit as a 2-write —
                    // text (no trailing newline) then, after a fixed delay, a
                    // lone CR. A single text+LF write lands in one read() and
                    // raw-mode TUIs (claude/codex, ink) treat it as a paste, so
                    // the newline is a literal line-break, not a submit. The gap
                    // lets the terminal close its paste-detection window before
                    // the CR arrives as its own keystroke.
                    let mut sent = 0u64;
                    // Empty text (`send <t> ''`): the server 400s on 0-byte
                    // input, so skip the text write and submit only the CR.
                    if !text_bytes.is_empty() {
                        sent += post_input(&client, &uuid, &text_bytes)?;
                        std::thread::sleep(std::time::Duration::from_millis(
                            SEND_SUBMIT_DELAY_MS,
                        ));
                    }
                    match post_input(&client, &uuid, &[CR]) {
                        Ok(n) => sent += n,
                        Err(e) => {
                            // Partial failure: the text is already in the input
                            // buffer. Re-running would duplicate it, so guide the
                            // caller to submit the CR alone instead.
                            return Err(CliError::Local(format!(
                                "text delivered to {uuid} but the Enter (CR) write \
                                 failed: {e:?}. The text is in the input buffer — \
                                 do NOT re-run this command (it would duplicate the \
                                 text). Recover by sending just the Enter: \
                                 `gtmux terminal send {uuid} --bytes 0d`.",
                            )));
                        }
                    }
                    println!("sent {sent} bytes to {uuid}");
                }
            }
            Ok(())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Workspace / fs / session dispatch
// ─────────────────────────────────────────────────────────────────────────────

fn workspace_dispatch(cmd: WorkspaceCmd) -> Result<(), CliError> {
    match cmd {
        WorkspaceCmd::Set {
            path,
            session,
            instance,
        } => {
            let client = connect(instance.instance)?;
            let abs = absolutize(&path)?;
            let resp = client.send_json(
                "PUT",
                &format!("/api/sessions/{session}/workspace"),
                &json!({ "workspace_root": abs }),
                &[],
            )?;
            let root = resp
                .get("workspace_root")
                .and_then(Value::as_str)
                .unwrap_or(&abs);
            println!("workspace of '{session}' → {root}");
            Ok(())
        }
        WorkspaceCmd::Get {
            session,
            instance,
            json,
        } => {
            let client = connect(instance.instance)?;
            let entry = session_list_entry(&client, &session)?;
            if json {
                print_json(&entry);
            } else {
                let root = entry
                    .get("workspace_root")
                    .and_then(Value::as_str)
                    .unwrap_or("(unset)");
                println!("{root}");
            }
            Ok(())
        }
    }
}

fn fs_dispatch(cmd: FsCmd) -> Result<(), CliError> {
    match cmd {
        FsCmd::Upload {
            src,
            dir,
            on_conflict,
            ctx,
            json,
        } => {
            let bytes = std::fs::read(&src)
                .map_err(|e| CliError::Local(format!("reading {}: {e}", src.display())))?;
            let filename = src
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| CliError::local("source path has no usable file name"))?
                .to_string();
            let client = connect(ctx.instance)?;
            // `--dir` is session-workspace-relative unless absolute — the
            // server contract wants an absolute in-A directory (ADR-0047 D2),
            // so a relative dir resolves against the session's effective
            // Workspace(B) root (ADR-0053 D14 workspace-import semantics).
            let dir_abs = if dir.starts_with('/') {
                dir
            } else {
                let session = require_session(ctx.session)?;
                let entry = session_list_entry(&client, &session)?;
                let root = entry
                    .get("workspace_root")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        CliError::local(format!("session '{session}' has no workspace_root"))
                    })?;
                if dir.is_empty() || dir == "." {
                    root.to_string()
                } else {
                    format!("{}/{}", root.trim_end_matches('/'), dir)
                }
            };
            let mut body = MultipartBody::new();
            body.text("dir", &dir_abs);
            if let Some(oc) = on_conflict {
                body.text("on_conflict", oc.wire());
            }
            body.file("file", &filename, &bytes);
            let resp = client.post_multipart("/api/fs/upload", body)?;
            if json {
                print_json(&resp);
                return Ok(());
            }
            if let Some(files) = resp.get("files").and_then(Value::as_array) {
                for f in files {
                    let path = f.get("path").and_then(Value::as_str).unwrap_or("?");
                    let conflict = f.get("conflict").and_then(Value::as_bool).unwrap_or(false);
                    let mark = if conflict { " (renamed/overwrote)" } else { "" };
                    println!("uploaded: {path}{mark}");
                }
            }
            Ok(())
        }
    }
}

fn session_create(
    name: String,
    workspace: Option<String>,
    yes: bool,
    password: Option<String>,
    instance: Option<String>,
) -> Result<(), CliError> {
    // The server mandates workspace_root (ADR-0046 D5) — fail fast with the
    // contract spelled out instead of bouncing a 400 invalid_workspace.
    let Some(workspace) = workspace else {
        return Err(CliError::local(
            "--workspace <path> is required (every session binds a Workspace(B) root — ADR-0046 D5)",
        ));
    };
    let abs = absolutize(&workspace)?;
    let client = connect(instance)?;
    let body = json!({ "name": name, "workspace_root": abs, "confirm": yes });
    let resp = with_session_gate(password, |pw| {
        let headers: Vec<(&str, &str)> = pw
            .map(|p| vec![("X-Gtmux-Password", p)])
            .unwrap_or_default();
        client.send_json("POST", "/api/sessions", &body, &headers)
    })?;
    let created = resp.get("name").and_then(Value::as_str).unwrap_or(&name);
    println!("created session '{created}' (workspace {abs})");
    Ok(())
}

fn session_delete(
    name: String,
    yes: bool,
    password: Option<String>,
    instance: Option<String>,
) -> Result<(), CliError> {
    let client = connect(instance)?;
    let path = if yes {
        format!("/api/sessions/{name}?confirm=true")
    } else {
        format!("/api/sessions/{name}")
    };
    with_session_gate(password, |pw| {
        let headers: Vec<(&str, &str)> = pw
            .map(|p| vec![("X-Gtmux-Password", p)])
            .unwrap_or_default();
        client.send_empty("DELETE", &path, &headers)
    })?;
    println!("deleted session '{name}' (mounted terminals stay in the pool)");
    Ok(())
}

/// ADR-0053 D6 session-lifecycle gate, client side. Password precedence:
/// `--password` flag → `$GTMUX_PASSWORD` → interactive prompt (TTY only,
/// and only after the server answered 401 `credential_required` — so the
/// no-password local `--yes` path never prompts).
fn with_session_gate(
    password_flag: Option<String>,
    send: impl Fn(Option<&str>) -> Result<Value, CliError>,
) -> Result<Value, CliError> {
    let password = password_flag.or_else(|| {
        std::env::var("GTMUX_PASSWORD")
            .ok()
            .filter(|s| !s.is_empty())
    });
    match send(password.as_deref()) {
        Err(CliError::Api {
            status: 401,
            code,
            message,
        }) if code == "credential_required" && password.is_none() => {
            if std::io::stdin().is_terminal() {
                let typed = rpassword::prompt_password("gtmux password: ")
                    .map_err(|e| CliError::Local(format!("reading password: {e}")))?;
                send(Some(&typed))
            } else {
                Err(CliError::Api {
                    status: 401,
                    code,
                    message: format!(
                        "{message} — pass --password <p> or set GTMUX_PASSWORD (non-interactive)"
                    ),
                })
            }
        }
        Err(CliError::Api {
            status: 400,
            code,
            message,
        }) if code == "confirm_required" => Err(CliError::Api {
            status: 400,
            code,
            message: format!("{message} — re-run with --yes"),
        }),
        other => other,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared plumbing
// ─────────────────────────────────────────────────────────────────────────────

fn open(ctx: Ctx) -> Result<(Client, String), CliError> {
    let client = connect(ctx.instance)?;
    let session = require_session(ctx.session)?;
    Ok((client, session))
}

fn require_session(opt: Option<String>) -> Result<String, CliError> {
    opt.ok_or_else(|| {
        CliError::local(
            "no canvas session: pass --session <name> or run inside a gtmux terminal \
             (GTMUX_CANVAS_SESSION)",
        )
    })
}

fn fetch_layout(client: &Client, session: &str) -> Result<Layout, CliError> {
    let v = client.get_json(&format!("/api/sessions/{session}/layout"))?;
    serde_json::from_value(v).map_err(|e| CliError::Local(format!("parsing layout: {e}")))
}

fn post_ops(client: &Client, session: &str, ops: Vec<Value>) -> Result<Value, CliError> {
    client.send_json(
        "POST",
        &format!("/api/sessions/{session}/layout/ops"),
        &json!({ "ops": ops }),
        &[],
    )
}

/// Resolve one item target, build one op, POST it — the shape shared by all
/// single-target mutations.
fn single_op(
    ctx: Ctx,
    domain: TargetDomain,
    target: &str,
    build: impl FnOnce(&str) -> Value,
) -> Result<(), CliError> {
    let (client, session) = open(ctx)?;
    let layout = fetch_layout(&client, &session)?;
    let id = resolve_target(&layout, target, domain)?;
    let resp = post_ops(&client, &session, vec![build(&id)])?;
    print_ops_result(&resp);
    Ok(())
}

fn z_op(ctx: Ctx, target: &str, op: &'static str, force: bool) -> Result<(), CliError> {
    single_op(ctx, TargetDomain::ItemOrGroup, target, move |id| {
        json!({ "op": op, "id": id, "force": force })
    })
}

fn print_ops_result(resp: &Value) {
    let etag = resp.get("etag").and_then(Value::as_str).unwrap_or("?");
    let applied = resp.get("applied").and_then(Value::as_u64).unwrap_or(0);
    println!("ok: applied={applied} etag={etag}");
    if let Some(ids) = resp.get("created_ids").and_then(Value::as_array) {
        for id in ids.iter().filter_map(Value::as_str) {
            println!("created: {id}");
        }
    }
    if let Some(fails) = resp.get("spawn_failures").and_then(Value::as_array) {
        for f in fails {
            eprintln!("warning: spawn failure: {f}");
        }
    }
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn absolutize(path: &str) -> Result<String, CliError> {
    let p = std::path::Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| CliError::Local(format!("resolving cwd: {e}")))?
            .join(p)
    };
    // Canonicalize so `..`/symlink shapes don't trip the server's A-internal
    // guard in surprising ways; the target must exist anyway (server stat).
    abs.canonicalize()
        .map(|c| c.to_string_lossy().into_owned())
        .map_err(|e| CliError::Local(format!("{path}: {e} (the directory must exist)")))
}

/// One session's row out of `GET /api/sessions` (workspace get / fs upload).
fn session_list_entry(client: &Client, session: &str) -> Result<Value, CliError> {
    let v = client.get_json("/api/sessions")?;
    let rows = v
        .get("sessions")
        .and_then(Value::as_array)
        .ok_or_else(|| CliError::local("unexpected /api/sessions response shape"))?;
    rows.iter()
        .find(|s| s.get("name").and_then(Value::as_str) == Some(session))
        .cloned()
        .ok_or_else(|| CliError::Api {
            status: 404,
            code: "session_not_found".into(),
            message: format!("no session named '{session}' on instance"),
        })
}

/// Fixed delay between the text write and the CR write in `terminal send`'s
/// 2-write submit (ADR-0054 D4 amend). Wide enough to clear a raw-mode TUI's
/// paste-detection window (same tick / tens of ms) so the CR reads as its own
/// Enter keystroke rather than being coalesced with the text.
const SEND_SUBMIT_DELAY_MS: u64 = 150;

/// Carriage return — the byte a keyboard Enter emits in raw mode. Shells map it
/// CR→NL via termios `icrnl`, so canonical command use is unaffected.
const CR: u8 = 0x0d;

/// Wire plan for `terminal send`, kept pure so the 2-write submit contract
/// (ADR-0054 D4 amend) is unit-testable without a live server.
enum SendPlan {
    /// One POST of these exact bytes: `--bytes <hex>`, or `--no-enter` text.
    Single(Vec<u8>),
    /// Submit these text bytes (no trailing newline) then, after a delay, a
    /// lone CR. Empty text ⇒ the text write is skipped and only the CR is sent.
    Submit(Vec<u8>),
}

/// Derive the send plan from the CLI args. Errors mirror the old inline
/// validation (mutual exclusion / nothing-to-send / bad hex).
fn plan_send(
    text: Option<String>,
    bytes: Option<String>,
    no_enter: bool,
) -> Result<SendPlan, String> {
    match (text, bytes) {
        (Some(_), Some(_)) => Err("pass either the text argument or --bytes, not both".into()),
        (None, None) => Err("nothing to send: provide text or --bytes <hex>".into()),
        // Text with no explicit Enter suppression submits via the 2-write path;
        // `--no-enter` sends the keystrokes only (single write, no CR).
        (Some(t), None) if no_enter => Ok(SendPlan::Single(t.into_bytes())),
        (Some(t), None) => Ok(SendPlan::Submit(t.into_bytes())),
        // --no-enter does not apply to raw control bytes.
        (None, Some(hex)) => Ok(SendPlan::Single(crate::ansi::parse_hex(&hex)?)),
    }
}

/// POST one raw byte chunk to a terminal's input; returns the server's `sent`
/// count. The 2-write submit waits on each response before the next write, so
/// PTY write order is guaranteed.
fn post_input(client: &Client, uuid: &str, payload: &[u8]) -> Result<u64, CliError> {
    let b64 = BASE64.encode(payload);
    let resp = client.send_json(
        "POST",
        &format!("/api/terminals/{uuid}/input"),
        &json!({ "bytes_base64": b64 }),
        &[],
    )?;
    Ok(resp.get("sent").and_then(Value::as_u64).unwrap_or(0))
}

fn resolve_pool_terminal(client: &Client, needle: &str) -> Result<String, CliError> {
    let v = client.get_json("/api/terminals")?;
    let rows = v
        .as_array()
        .ok_or_else(|| CliError::local("unexpected /api/terminals response shape"))?;
    if is_uuid_shape(needle) {
        return if rows
            .iter()
            .any(|r| r.get("id").and_then(Value::as_str) == Some(needle))
        {
            Ok(needle.to_string())
        } else {
            Err(CliError::Api {
                status: 404,
                code: "terminal_not_found".into(),
                message: format!("terminal {needle:?} is not in the alive pool"),
            })
        };
    }
    let matches: Vec<&Value> = rows
        .iter()
        .filter(|r| r.get("label").and_then(Value::as_str) == Some(needle))
        .collect();
    match matches.len() {
        0 => Err(CliError::Api {
            status: 404,
            code: "terminal_not_found".into(),
            message: format!("no pool terminal labelled {needle:?} (see `gtmux terminal ls`)"),
        }),
        1 => Ok(matches[0]
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()),
        n => {
            let mut msg = format!("label {needle:?} is ambiguous ({n} pool terminals) — use the UUID:");
            for m in matches {
                let id = m.get("id").and_then(Value::as_str).unwrap_or("?");
                msg.push_str(&format!("\n  {id}  terminal  {needle:?}"));
            }
            Err(CliError::Local(msg))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Target resolution (ADR-0053 D3)
// ─────────────────────────────────────────────────────────────────────────────

/// Which entity kinds a command may target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetDomain {
    /// Any canvas item.
    Item,
    /// Item or group (visibility / label / z / reparent / group members).
    ItemOrGroup,
    /// Group only (ungroup / reparent --parent).
    Group,
    /// Terminal panels only (`terminal unmount`).
    TerminalItem,
}

impl TargetDomain {
    fn word(self) -> &'static str {
        match self {
            TargetDomain::Item => "item",
            TargetDomain::ItemOrGroup => "item or group",
            TargetDomain::Group => "group",
            TargetDomain::TerminalItem => "terminal panel",
        }
    }
}

/// Canonical lowercase-or-uppercase-hex 8-4-4-4-12 shape. A UUID-shaped
/// target always resolves as an id (never as a label — ADR-0053 D3).
pub fn is_uuid_shape(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| match i {
            8 | 13 | 18 | 23 => c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn item_type_name(it: &Item) -> &'static str {
    match it {
        Item::Terminal { .. } => "terminal",
        Item::Text { .. } => "text",
        Item::Note { .. } => "note",
        Item::Rect { .. } => "rect",
        Item::Ellipse { .. } => "ellipse",
        Item::Line { .. } => "line",
        Item::FreeDraw { .. } => "free_draw",
        Item::Image { .. } => "image",
        Item::Document { .. } => "document",
        Item::FilePath { .. } => "file_path",
        Item::Path { .. } => "path",
        Item::Snippets { .. } => "snippets",
        // ADR-0059 — schema variant added in plan 0025 Batch A; the agent-
        // facing CLI surface (`layout create web_view`, help, SKILL.md) landed
        // in Batch C. Generic create/edit plumbing carries the `url` field.
        Item::WebView { .. } => "web_view",
    }
}

fn find_item<'a>(layout: &'a Layout, id: &str) -> Option<&'a Item> {
    layout.items.iter().find(|it| it.common().id == id)
}

fn domain_allows_item(domain: TargetDomain, it: &Item) -> bool {
    match domain {
        TargetDomain::Item | TargetDomain::ItemOrGroup => true,
        TargetDomain::Group => false,
        TargetDomain::TerminalItem => matches!(it, Item::Terminal { .. }),
    }
}

fn domain_allows_groups(domain: TargetDomain) -> bool {
    matches!(domain, TargetDomain::ItemOrGroup | TargetDomain::Group)
}

/// Resolve `needle` to a canonical id (ADR-0053 D3): UUID shape → exact id
/// lookup; otherwise exact-label match, failing on 0 or 2+ hits with the
/// candidates listed (id / kind / label) so the caller can retry by UUID.
pub fn resolve_target(
    layout: &Layout,
    needle: &str,
    domain: TargetDomain,
) -> Result<String, CliError> {
    if is_uuid_shape(needle) {
        if let Some(it) = find_item(layout, needle) {
            if domain_allows_item(domain, it) {
                return Ok(needle.to_string());
            }
            return Err(CliError::Local(format!(
                "{needle} is a {} item, but this command targets a {}",
                item_type_name(it),
                domain.word()
            )));
        }
        if layout.groups.iter().any(|g| g.id == needle) {
            if domain_allows_groups(domain) {
                return Ok(needle.to_string());
            }
            return Err(CliError::Local(format!(
                "{needle} is a group, but this command targets a {}",
                domain.word()
            )));
        }
        return Err(CliError::Api {
            status: 404,
            code: "item_not_found".into(),
            message: format!("no {} with id {needle} in this session layout", domain.word()),
        });
    }

    // (id, kind, label) candidates by exact label match.
    let mut matches: Vec<(String, &'static str, String)> = Vec::new();
    for it in &layout.items {
        if domain_allows_item(domain, it) && it.common().label == needle {
            matches.push((
                it.common().id.clone(),
                item_type_name(it),
                it.common().label.clone(),
            ));
        }
    }
    if domain_allows_groups(domain) {
        for g in &layout.groups {
            if g.label == needle {
                matches.push((g.id.clone(), "group", g.label.clone()));
            }
        }
    }
    match matches.len() {
        0 => Err(CliError::Api {
            status: 404,
            code: "item_not_found".into(),
            message: format!(
                "no {} labelled {needle:?} in this session layout (targets are a UUID or an \
                 exact label — see `gtmux layout list`)",
                domain.word()
            ),
        }),
        1 => Ok(matches.remove(0).0),
        n => {
            let mut msg =
                format!("label {needle:?} is ambiguous ({n} matches) — use the UUID instead:");
            for (id, kind, label) in &matches {
                msg.push_str(&format!("\n  {id}  {kind}  {label:?}"));
            }
            Err(CliError::Local(msg))
        }
    }
}

/// Resolve many targets, deduplicating while preserving order.
fn resolve_many(
    layout: &Layout,
    targets: &[String],
    domain: TargetDomain,
) -> Result<Vec<String>, CliError> {
    let mut out: Vec<String> = Vec::with_capacity(targets.len());
    for t in targets {
        let id = resolve_target(layout, t, domain)?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Op builders (wire canonical: crates/http-api/src/layout_ops.rs)
// ─────────────────────────────────────────────────────────────────────────────

fn obj(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn insert_opt_f64(map: &mut Map<String, Value>, key: &str, v: Option<f64>) {
    if let Some(v) = v {
        map.insert(key.to_string(), json!(v));
    }
}

fn build_create_op(
    item_type: &str,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    fields: Option<Value>,
) -> Value {
    let mut op = obj(&[("op", json!("create")), ("item_type", json!(item_type))]);
    insert_opt_f64(&mut op, "x", x);
    insert_opt_f64(&mut op, "y", y);
    insert_opt_f64(&mut op, "w", w);
    insert_opt_f64(&mut op, "h", h);
    if let Some(f) = fields {
        op.insert("fields".into(), f);
    }
    Value::Object(op)
}

/// Build the payload-fields object from `--set K=V` pairs and/or `--json`.
/// Values in `--set` are parsed as JSON first (numbers / booleans / null /
/// arrays / objects) and fall back to plain strings — nested edits should
/// use `--json` (ADR-0053 D10).
fn fields_from_args(set: &[String], json_arg: Option<&str>) -> Result<Option<Value>, CliError> {
    if let Some(raw) = json_arg {
        let v: Value = serde_json::from_str(raw)
            .map_err(|e| CliError::Local(format!("--json is not valid JSON: {e}")))?;
        if !v.is_object() {
            return Err(CliError::local("--json must be a JSON object of fields"));
        }
        return Ok(Some(v));
    }
    if set.is_empty() {
        return Ok(None);
    }
    let mut map = Map::new();
    for pair in set {
        let Some((k, raw)) = pair.split_once('=') else {
            return Err(CliError::Local(format!(
                "--set expects K=V, got {pair:?}"
            )));
        };
        if k.is_empty() {
            return Err(CliError::Local(format!("--set has an empty key: {pair:?}")));
        }
        let v = serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_string()));
        map.insert(k.to_string(), v);
    }
    Ok(Some(Value::Object(map)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Align (ADR-0053 D13 — CLI-side math, batch move ops)
// ─────────────────────────────────────────────────────────────────────────────

/// Display BBox of an item — line items span their endpoints; everything
/// else uses common x/y/w/h (path/free_draw x/y/w/h is the server-maintained
/// BBox cache). FE `itemBBox` parity.
fn item_align_box(it: &Item) -> AlignBox {
    let c = it.common();
    if let Item::Line { x2, y2, .. } = it {
        let x = c.x.min(*x2);
        let y = c.y.min(*y2);
        AlignBox {
            id: c.id.clone(),
            x,
            y,
            w: (x2 - c.x).abs(),
            h: (y2 - c.y).abs(),
            locked: c.locked,
        }
    } else {
        AlignBox {
            id: c.id.clone(),
            x: c.x,
            y: c.y,
            w: c.w,
            h: c.h,
            locked: c.locked,
        }
    }
}

/// Resolve targets → compute deltas → emit `move` ops carrying the new
/// absolute `common.x/y` (the server translates line endpoints / free-draw
/// points / path free endpoints by the same delta — FE `moveItem` parity).
fn build_align_ops(
    layout: &Layout,
    targets: &[String],
    mode: AlignMode,
) -> Result<Vec<Value>, CliError> {
    let ids = resolve_many(layout, targets, TargetDomain::Item)?;
    let boxes: Vec<AlignBox> = ids
        .iter()
        .map(|id| item_align_box(find_item(layout, id).expect("resolved id exists")))
        .collect();
    let deltas = compute_deltas(&boxes, mode).map_err(CliError::Local)?;
    let mut ops = Vec::with_capacity(deltas.len());
    for (id, dx, dy) in deltas {
        let c = find_item(layout, &id).expect("delta id came from boxes").common();
        ops.push(json!({
            "op": "move", "id": id,
            "x": c.x + dx, "y": c.y + dy,
            "force": false,
        }));
    }
    Ok(ops)
}

// ─────────────────────────────────────────────────────────────────────────────
// Connections (ADR-0053 D13 — CLI-side parse of the stored layout)
// ─────────────────────────────────────────────────────────────────────────────

fn anchor_str(anchor: &gtmux_http_api::Anchor) -> String {
    serde_json::to_value(anchor)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "?".to_string())
}

fn endpoint_json(layout: &Layout, ep: &PathEndpoint) -> Value {
    match ep {
        PathEndpoint::Free { point } => json!({
            "kind": "free", "x": point.x, "y": point.y,
        }),
        PathEndpoint::Connected {
            item_id, anchor, ..
        } => {
            let label = find_item(layout, item_id)
                .map(|it| it.common().label.clone())
                .unwrap_or_default();
            let kind = find_item(layout, item_id)
                .map(item_type_name)
                .unwrap_or("?");
            json!({
                "kind": "connected", "item_id": item_id,
                "item_type": kind, "item_label": label,
                "anchor": anchor_str(anchor),
            })
        }
    }
}

/// Rows describing the path connectivity of `target_id`.
///
/// * target is a **path** → its own `from`/`to` endpoints.
/// * otherwise → every path with a connected endpoint referencing the
///   target; the row carries the near anchor and the far endpoint. The
///   stored layout is always degrade-complete (PUT/ops pipeline), so no
///   dangling reference handling is needed (ADR-0053 D13).
pub fn connections_for(layout: &Layout, target_id: &str) -> Vec<Value> {
    let mut rows = Vec::new();
    if let Some(Item::Path { from, to, .. }) = find_item(layout, target_id) {
        for (end, ep) in [("from", from), ("to", to)] {
            rows.push(json!({
                "path_id": target_id,
                "endpoint": end,
                "connection": endpoint_json(layout, ep),
            }));
        }
        return rows;
    }
    for it in &layout.items {
        let Item::Path {
            common, from, to, ..
        } = it
        else {
            continue;
        };
        for (end, ep, other) in [("from", from, to), ("to", to, from)] {
            let PathEndpoint::Connected {
                item_id, anchor, ..
            } = ep
            else {
                continue;
            };
            if item_id != target_id {
                continue;
            }
            rows.push(json!({
                "path_id": common.id,
                "path_label": common.label,
                "endpoint": end,
                "anchor": anchor_str(anchor),
                "other": endpoint_json(layout, other),
            }));
        }
    }
    rows
}

fn connection_row_text(row: &Value) -> String {
    let path_id = row.get("path_id").and_then(Value::as_str).unwrap_or("?");
    let end = row.get("endpoint").and_then(Value::as_str).unwrap_or("?");
    // Path-target rows carry `connection`; item-target rows carry `other`.
    if let Some(conn) = row.get("connection") {
        return format!("{end}: {}", endpoint_text(conn));
    }
    let label = row.get("path_label").and_then(Value::as_str).unwrap_or("");
    let anchor = row.get("anchor").and_then(Value::as_str).unwrap_or("?");
    let other = row.get("other").map(endpoint_text).unwrap_or_default();
    let label_part = if label.is_empty() {
        String::new()
    } else {
        format!(" {label:?}")
    };
    format!("path {path_id}{label_part}: {end}@{anchor} -> {other}")
}

fn endpoint_text(ep: &Value) -> String {
    match ep.get("kind").and_then(Value::as_str) {
        Some("connected") => {
            let id = ep.get("item_id").and_then(Value::as_str).unwrap_or("?");
            let kind = ep.get("item_type").and_then(Value::as_str).unwrap_or("?");
            let label = ep.get("item_label").and_then(Value::as_str).unwrap_or("");
            let anchor = ep.get("anchor").and_then(Value::as_str).unwrap_or("?");
            if label.is_empty() {
                format!("{kind} {id} @{anchor}")
            } else {
                format!("{kind} {id} {label:?} @{anchor}")
            }
        }
        Some("free") => {
            let x = ep.get("x").and_then(Value::as_f64).unwrap_or(0.0);
            let y = ep.get("y").and_then(Value::as_f64).unwrap_or(0.0);
            format!("free ({x}, {y})")
        }
        _ => "?".to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Plain-text rendering
// ─────────────────────────────────────────────────────────────────────────────

fn print_layout_table(layout: &Layout) {
    if layout.items.is_empty() && layout.groups.is_empty() {
        println!("(empty layout)");
        return;
    }
    if !layout.items.is_empty() {
        println!(
            "{:<38}{:<11}{:<22}{:>9}{:>9}{:>8}{:>8}  {:<9}{:<5}{:<5}{:>4}",
            "ID", "TYPE", "LABEL", "X", "Y", "W", "H", "VIS", "LOCK", "MIN", "Z"
        );
        for it in &layout.items {
            let c = it.common();
            println!(
                "{:<38}{:<11}{:<22}{:>9.1}{:>9.1}{:>8.1}{:>8.1}  {:<9}{:<5}{:<5}{:>4}",
                c.id,
                item_type_name(it),
                crate::truncate(&c.label, 21),
                c.x,
                c.y,
                c.w,
                c.h,
                visibility_str(c.visibility),
                if c.locked { "yes" } else { "-" },
                if c.minimized { "yes" } else { "-" },
                c.z,
            );
        }
    }
    if !layout.groups.is_empty() {
        println!();
        println!("GROUPS:");
        for g in &layout.groups {
            println!(
                "  {}  {:?}  parent={}  {}{}",
                g.id,
                g.label,
                g.parent_id.as_deref().unwrap_or("root"),
                visibility_str(g.visibility),
                if g.locked { "  locked" } else { "" },
            );
        }
    }
}

fn visibility_str(v: gtmux_http_api::Visibility) -> &'static str {
    match v {
        gtmux_http_api::Visibility::Visible => "visible",
        gtmux_http_api::Visibility::Hidden => "hidden",
    }
}

fn print_item_detail(item: &Item) {
    let c = item.common();
    println!("id:          {}", c.id);
    println!("type:        {}", item_type_name(item));
    println!("label:       {:?}", c.label);
    println!("parent:      {}", c.parent_id.as_deref().unwrap_or("root"));
    println!("x/y:         {} / {}", c.x, c.y);
    println!("w/h:         {} / {}", c.w, c.h);
    println!("z:           {}", c.z);
    println!("visibility:  {}", visibility_str(c.visibility));
    println!("locked:      {}", c.locked);
    println!("minimized:   {}", c.minimized);
    if !c.description.is_empty() {
        println!("description: {:?}", c.description);
    }
    println!("(--json for the full type payload)");
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — pure logic only (target resolve / op shapes / align / connections).
// HTTP integration is Batch E's live-server verification (plan-0023).
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const ID_TERM: &str = "11111111-1111-4111-8111-111111111111";
    const ID_NOTE_A: &str = "22222222-2222-4222-8222-222222222222";
    const ID_NOTE_B: &str = "33333333-3333-4333-8333-333333333333";
    const ID_LINE: &str = "44444444-4444-4444-8444-444444444444";
    const ID_PATH: &str = "55555555-5555-4555-8555-555555555555";
    const ID_GROUP: &str = "66666666-6666-4666-8666-666666666666";

    fn common(id: &str, label: &str, x: f64, y: f64, w: f64, h: f64) -> Value {
        json!({
            "id": id, "parent_id": null,
            "x": x, "y": y, "w": w, "h": h, "z": 0,
            "visibility": "visible", "locked": false, "label": label,
        })
    }

    fn merged(base: Value, extra: Value) -> Value {
        let (Value::Object(mut base), Value::Object(extra)) = (base, extra) else {
            panic!("merge expects objects");
        };
        base.extend(extra);
        Value::Object(base)
    }

    fn fixture() -> Layout {
        let items = json!([
            merged(common(ID_TERM, "term", 0.0, 0.0, 480.0, 320.0), json!({"type": "terminal"})),
            merged(
                common(ID_NOTE_A, "dup", 100.0, 0.0, 300.0, 96.0),
                json!({"type": "note", "title": "", "body": "", "color": "c"})
            ),
            merged(
                common(ID_NOTE_B, "dup", 200.0, 50.0, 300.0, 96.0),
                json!({"type": "note", "title": "", "body": "", "color": "c"})
            ),
            merged(
                common(ID_LINE, "wire", 10.0, 10.0, 256.0, 96.0),
                json!({"type": "line", "stroke": "s", "stroke_width": 2, "x2": 250.0, "y2": 90.0})
            ),
            merged(
                common(ID_PATH, "arrow", 0.0, 0.0, 1.0, 1.0),
                json!({
                    "type": "path",
                    "from": {
                        "kind": "connected", "item_id": ID_TERM, "anchor": "E",
                        "fallback_point": {"x": 480.0, "y": 160.0}
                    },
                    "to": {"kind": "free", "point": {"x": 700.0, "y": 200.0}},
                    "routing": "orthogonal", "head_from": "none", "head_to": "none",
                    "stroke": "s", "stroke_width": 2
                })
            ),
        ]);
        let layout = json!({
            "schema_version": 2,
            "groups": [{
                "id": ID_GROUP, "parent_id": null, "label": "grp",
                "color": null, "visibility": "visible", "locked": false, "order": 1
            }],
            "items": items,
            "viewport": {"x": 0.0, "y": 0.0, "zoom": 1.0},
        });
        serde_json::from_value(layout).expect("fixture layout parses")
    }

    #[test]
    fn uuid_shape_detection() {
        assert!(is_uuid_shape(ID_TERM));
        assert!(!is_uuid_shape("not-a-uuid"));
        assert!(!is_uuid_shape("11111111-1111-4111-8111-11111111111")); // 35 chars
        assert!(!is_uuid_shape("g1111111-1111-4111-8111-111111111111")); // non-hex
    }

    #[test]
    fn resolve_by_uuid_and_label() {
        let layout = fixture();
        assert_eq!(
            resolve_target(&layout, ID_TERM, TargetDomain::Item).unwrap(),
            ID_TERM
        );
        assert_eq!(
            resolve_target(&layout, "term", TargetDomain::Item).unwrap(),
            ID_TERM
        );
        // Group by label needs a group-inclusive domain.
        assert_eq!(
            resolve_target(&layout, "grp", TargetDomain::Group).unwrap(),
            ID_GROUP
        );
        assert_eq!(
            resolve_target(&layout, ID_GROUP, TargetDomain::ItemOrGroup).unwrap(),
            ID_GROUP
        );
    }

    #[test]
    fn resolve_ambiguous_label_lists_candidates() {
        let layout = fixture();
        let err = resolve_target(&layout, "dup", TargetDomain::Item).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("ambiguous"));
        assert!(msg.contains(ID_NOTE_A));
        assert!(msg.contains(ID_NOTE_B));
        assert!(msg.contains("note"));
    }

    #[test]
    fn resolve_missing_label_is_not_found() {
        let layout = fixture();
        let err = resolve_target(&layout, "nope", TargetDomain::Item).unwrap_err();
        assert!(matches!(err, CliError::Api { status: 404, .. }));
    }

    #[test]
    fn resolve_uuid_never_falls_back_to_label() {
        let layout = fixture();
        // UUID-shaped but absent → not found even if some label matched.
        let ghost = "99999999-9999-4999-8999-999999999999";
        let err = resolve_target(&layout, ghost, TargetDomain::Item).unwrap_err();
        assert!(matches!(err, CliError::Api { status: 404, .. }));
    }

    #[test]
    fn resolve_domain_gating() {
        let layout = fixture();
        // A group id is rejected for item-only commands…
        assert!(resolve_target(&layout, ID_GROUP, TargetDomain::Item).is_err());
        // …and a non-terminal item for `terminal unmount`.
        assert!(resolve_target(&layout, ID_NOTE_A, TargetDomain::TerminalItem).is_err());
        assert_eq!(
            resolve_target(&layout, ID_TERM, TargetDomain::TerminalItem).unwrap(),
            ID_TERM
        );
    }

    // ── terminal send: 2-write CR submit (ADR-0054 D4 amend) ──────────────

    /// Text without `--no-enter` = text bytes (NO trailing newline) followed by
    /// a lone CR (0x0d), sent as two separate writes.
    #[test]
    fn send_text_submits_with_separate_cr() {
        let plan = plan_send(Some("echo hi".into()), None, false).unwrap();
        let SendPlan::Submit(text) = plan else {
            panic!("text should submit via 2-write");
        };
        assert_eq!(text, b"echo hi", "text carries no trailing newline");
        assert!(!text.ends_with(b"\n"), "no LF appended to the text write");
        // The submit write is a single CR byte.
        assert_eq!([CR], [0x0d]);
    }

    /// A multiline prompt keeps its internal `\n` (TUI multiline paste); the CR
    /// is still the only submit byte.
    #[test]
    fn send_multiline_text_keeps_internal_newlines() {
        let plan = plan_send(Some("line1\nline2".into()), None, false).unwrap();
        let SendPlan::Submit(text) = plan else {
            panic!("expected submit");
        };
        assert_eq!(text, b"line1\nline2");
        assert!(!text.ends_with(b"\n"), "no submit LF appended");
    }

    /// `--no-enter` = a single write of exactly the text, no CR, no LF.
    #[test]
    fn send_no_enter_is_single_write_without_newline() {
        let plan = plan_send(Some("partial".into()), None, true).unwrap();
        let SendPlan::Single(payload) = plan else {
            panic!("--no-enter should be a single write");
        };
        assert_eq!(payload, b"partial");
        assert!(!payload.ends_with(b"\n"));
        assert!(!payload.ends_with(&[CR]));
    }

    /// `--bytes` is unchanged: a single write of the decoded hex, unaffected by
    /// `--no-enter`.
    #[test]
    fn send_bytes_is_unchanged_single_write() {
        let plan = plan_send(None, Some("03".into()), false).unwrap();
        let SendPlan::Single(payload) = plan else {
            panic!("--bytes should be a single write");
        };
        assert_eq!(payload, vec![0x03]);
        // --no-enter has no effect on raw bytes.
        let plan2 = plan_send(None, Some("1b5b41".into()), true).unwrap();
        assert!(matches!(plan2, SendPlan::Single(ref p) if *p == vec![0x1b, 0x5b, 0x41]));
    }

    /// Empty text (not `--no-enter`) → the text write is skipped (server 400s on
    /// 0 bytes); only the CR is submitted. The plan carries empty text; the
    /// dispatch elides the first POST.
    #[test]
    fn send_empty_text_submits_cr_only() {
        let plan = plan_send(Some(String::new()), None, false).unwrap();
        let SendPlan::Submit(text) = plan else {
            panic!("empty text still submits");
        };
        assert!(text.is_empty(), "no text write for empty input");
    }

    /// Argument validation is preserved.
    #[test]
    fn send_rejects_conflicting_and_empty_args() {
        assert!(plan_send(Some("x".into()), Some("03".into()), false).is_err());
        assert!(plan_send(None, None, false).is_err());
        assert!(plan_send(None, Some("zz".into()), false).is_err(), "bad hex");
    }

    #[test]
    fn op_shapes_match_wire_contract() {
        // Wire canonical: layout_ops.rs — tag field "op", snake_case names.
        let create = build_create_op("note", Some(1.0), None, None, None, Some(json!({"title": "t"})));
        assert_eq!(
            create,
            json!({"op": "create", "item_type": "note", "x": 1.0, "fields": {"title": "t"}})
        );
        let mut spawn = obj(&[("op", json!("spawn"))]);
        insert_opt_f64(&mut spawn, "x", None);
        assert_eq!(Value::Object(spawn), json!({"op": "spawn"}));
    }

    #[test]
    fn web_view_create_op_carries_url_field() {
        // ADR-0059 D4 — `layout create web_view --set url=…` rides the generic
        // create plumbing: the CLI forwards the type + `url` field verbatim and
        // the server (Batch A) owns scheme/own-origin/4KiB validation.
        let fields = fields_from_args(&["url=https://example.com".to_string()], None)
            .unwrap()
            .unwrap();
        let create = build_create_op("web_view", None, None, None, None, Some(fields));
        assert_eq!(
            create,
            json!({
                "op": "create", "item_type": "web_view",
                "fields": {"url": "https://example.com"}
            })
        );
        // A workspace-relative path is an opaque string to the CLI too.
        let fields = fields_from_args(&["url=docs/report.md".to_string()], None)
            .unwrap()
            .unwrap();
        let edit = json!({ "op": "edit", "id": ID_TERM, "fields": fields, "force": false });
        assert_eq!(edit["fields"]["url"], "docs/report.md");
    }

    #[test]
    fn web_view_item_type_name_maps() {
        let it: Item = serde_json::from_value(merged(
            common(ID_NOTE_A, "site", 0.0, 0.0, 480.0, 360.0),
            json!({"type": "web_view", "url": "https://example.com"}),
        ))
        .expect("web_view item parses");
        assert_eq!(item_type_name(&it), "web_view");
    }

    #[test]
    fn set_pairs_parse_json_with_string_fallback() {
        let fields = fields_from_args(
            &[
                "text=hello world".to_string(),
                "font_size=20".to_string(),
                "italic=true".to_string(),
                "stroke_dash=null".to_string(),
            ],
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            fields,
            json!({
                "text": "hello world",
                "font_size": 20,
                "italic": true,
                "stroke_dash": null,
            })
        );
        assert!(fields_from_args(&["broken".to_string()], None).is_err());
        assert!(fields_from_args(&[], Some("[1]")).is_err()); // non-object
        assert!(fields_from_args(&[], None).unwrap().is_none());
    }

    #[test]
    fn align_ops_translate_line_start_not_bbox() {
        let layout = fixture();
        // Align note A (x 100..400) and the line (bbox x 10..250) to the left
        // edge (bbox min x = 10). The line's *item.x* stays the op payload —
        // the server translates x2 by the same delta.
        let ops = build_align_ops(
            &layout,
            &["dup".to_string()], // ambiguous → error path exercised below
            AlignMode::Left,
        );
        assert!(ops.is_err(), "ambiguous label must fail resolution");

        let ops = build_align_ops(
            &layout,
            &[ID_NOTE_A.to_string(), ID_LINE.to_string()],
            AlignMode::Left,
        )
        .unwrap();
        // BBox min x = 10 (line). Note A moves dx = -90; line unchanged.
        assert_eq!(ops.len(), 1);
        assert_eq!(
            ops[0],
            json!({"op": "move", "id": ID_NOTE_A, "x": 10.0, "y": 0.0, "force": false})
        );
    }

    #[test]
    fn align_dedups_targets() {
        let layout = fixture();
        let err = build_align_ops(
            &layout,
            &[ID_NOTE_A.to_string(), ID_NOTE_A.to_string()],
            AlignMode::Left,
        )
        .unwrap_err();
        // Deduped to one box → "needs at least 2 targets".
        assert!(format!("{err:?}").contains("at least 2"));
    }

    #[test]
    fn connections_of_item_lists_touching_paths() {
        let layout = fixture();
        let rows = connections_for(&layout, ID_TERM);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["path_id"], ID_PATH);
        assert_eq!(row["endpoint"], "from");
        assert_eq!(row["anchor"], "E");
        assert_eq!(row["other"]["kind"], "free");
        assert_eq!(row["other"]["x"], 700.0);
        // Untouched item → empty.
        assert!(connections_for(&layout, ID_NOTE_A).is_empty());
    }

    #[test]
    fn connections_of_path_lists_its_endpoints() {
        let layout = fixture();
        let rows = connections_for(&layout, ID_PATH);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["endpoint"], "from");
        assert_eq!(rows[0]["connection"]["kind"], "connected");
        assert_eq!(rows[0]["connection"]["item_id"], ID_TERM);
        assert_eq!(rows[0]["connection"]["item_label"], "term");
        assert_eq!(rows[1]["connection"]["kind"], "free");
    }

    #[test]
    fn connection_rows_render_plain_text() {
        let layout = fixture();
        let rows = connections_for(&layout, ID_TERM);
        let text = connection_row_text(&rows[0]);
        assert!(text.contains(ID_PATH));
        assert!(text.contains("from@E"));
        assert!(text.contains("free (700, 200)"));
    }

    #[test]
    fn item_align_box_uses_line_endpoints() {
        let layout = fixture();
        let line = find_item(&layout, ID_LINE).unwrap();
        let b = item_align_box(line);
        assert_eq!((b.x, b.y), (10.0, 10.0));
        assert_eq!((b.w, b.h), (240.0, 80.0));
    }
}
