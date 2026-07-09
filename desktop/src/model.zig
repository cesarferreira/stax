const std = @import("std");
const native_sdk = @import("native_sdk");

const bridge = @import("engine_bridge.zig");
const protocol = @import("protocol.zig");

pub const snapshot_effect_key = bridge.snapshot_key;
pub const diff_effect_key = bridge.diff_key;
pub const action_effect_key = bridge.action_key;
pub const recents_read_effect_key = bridge.recents_read_key;
pub const recents_write_effect_key = bridge.recents_write_key;
pub const clipboard_effect_key = bridge.clipboard_key;

const max_path_bytes = 1024;
const max_request_id_bytes = 64;
const max_error_bytes = 2048;
const max_status_bytes = 512;
const max_recents = 10;

pub const Effects = native_sdk.Effects(Msg);
pub const Action = bridge.Action;

pub const Confirmation = enum {
    none,
    restack,
    submit_stack,
};

pub const BranchRow = struct {
    index: usize,
    name: []const u8,
    parent: []const u8,
    column: usize,
    is_current: bool,
    is_selected: bool,
    ahead: usize,
    behind: usize,
    status: []const u8,
};

pub const DiffRow = struct {
    index: usize,
    text: []const u8,
    kind: protocol.DiffLineKind,
    is_addition: bool,
    is_deletion: bool,
    is_file: bool,
    is_hunk: bool,
};

pub const RecentRow = struct {
    index: usize,
    path: []const u8,
    label: []const u8,
};

pub const Msg = union(enum) {
    pub const view_unbound = .{
        "repository_selected",
        "repository_picker_cancelled",
        "recents_loaded",
        "recents_saved",
        "snapshot_exit",
        "diff_exit",
        "action_line",
        "action_exit",
        "select_next",
        "select_previous",
        "app_activated",
        "focus_filter",
        "diagnostics_copied",
        "dismiss",
    };

    repository_selected: []const u8,
    repository_picker_cancelled,
    recents_loaded: native_sdk.EffectFileResult,
    recents_saved: native_sdk.EffectFileResult,
    snapshot_exit: native_sdk.EffectExit,
    diff_exit: native_sdk.EffectExit,
    action_line: native_sdk.EffectLine,
    action_exit: native_sdk.EffectExit,
    select_branch: usize,
    select_recent: usize,
    select_next,
    select_previous,
    toggle_recents,
    close_recents,
    request_checkout,
    request_restack,
    request_submit,
    request_open_pr,
    confirm_action,
    cancel_confirmation,
    refresh,
    app_activated,
    choose_repository,
    focus_filter,
    stack_resized: f32,
    inspector_resized: f32,
    copy_diagnostics,
    diagnostics_copied: native_sdk.EffectClipboardResult,
    dismiss,
    dismiss_error,
    filter_changed: native_sdk.canvas.TextInputEvent,
};

pub const Model = struct {
    pub const view_unbound = .{
        "allocator",
        "snapshot_arena",
        "diff_arena",
        "selected_index",
        "selection_generation",
        "engine_storage",
        "engine_len",
        "store_storage",
        "store_len",
        "repository_storage",
        "repository_len",
        "request_sequence",
        "snapshot_request_storage",
        "snapshot_request_len",
        "snapshot_repository_storage",
        "snapshot_repository_len",
        "diff_request_storage",
        "diff_request_len",
        "action_request_storage",
        "action_request_len",
        "error_storage",
        "error_len",
        "status_storage",
        "status_len",
        "recent_storage",
        "recent_lens",
        "recent_count",
        "recent_serialized",
        "recent_serialized_len",
        "filter",
        "focused_pane",
        "confirmation",
        "active_action",
        "action_terminal_seen",
        "loading_snapshot",
        "needs_repository_picker",
        "recents_open",
        "repositoryPath",
        "selectedBranch",
        "hasRepository",
        "hasSnapshot",
        "isBusy",
    };

    allocator: std.mem.Allocator,
    snapshot_arena: std.heap.ArenaAllocator,
    diff_arena: std.heap.ArenaAllocator,

    snapshot: ?protocol.RepositorySnapshot = null,
    diff: ?protocol.DiffSnapshot = null,
    selected_index: usize = 0,
    selection_generation: u64 = 0,

    engine_storage: [max_path_bytes]u8 = undefined,
    engine_len: usize = 0,
    store_storage: [max_path_bytes]u8 = undefined,
    store_len: usize = 0,
    repository_storage: [max_path_bytes]u8 = undefined,
    repository_len: usize = 0,

    request_sequence: u64 = 0,
    snapshot_request_storage: [max_request_id_bytes]u8 = undefined,
    snapshot_request_len: usize = 0,
    snapshot_repository_storage: [max_path_bytes]u8 = undefined,
    snapshot_repository_len: usize = 0,
    diff_request_storage: [max_request_id_bytes]u8 = undefined,
    diff_request_len: usize = 0,
    action_request_storage: [max_request_id_bytes]u8 = undefined,
    action_request_len: usize = 0,

    error_storage: [max_error_bytes]u8 = undefined,
    error_len: usize = 0,
    status_storage: [max_status_bytes]u8 = undefined,
    status_len: usize = 0,

    recent_storage: [max_recents][max_path_bytes]u8 = undefined,
    recent_lens: [max_recents]usize = [_]usize{0} ** max_recents,
    recent_count: usize = 0,
    recent_serialized: [max_recents * (max_path_bytes + 1)]u8 = undefined,
    recent_serialized_len: usize = 0,

    filter: native_sdk.canvas.TextBuffer(96) = .{},
    pane_stack_ratio: f32 = 0.32,
    pane_inspector_ratio: f32 = 0.43,
    filter_focused: bool = false,
    focused_pane: u8 = 0,

    confirmation: Confirmation = .none,
    active_action: ?Action = null,
    action_terminal_seen: bool = false,
    loading_snapshot: bool = false,
    loading_diff: bool = false,
    needs_repository_picker: bool = false,
    recents_open: bool = false,

    pub fn init(allocator: std.mem.Allocator) Model {
        return .{
            .allocator = allocator,
            .snapshot_arena = std.heap.ArenaAllocator.init(allocator),
            .diff_arena = std.heap.ArenaAllocator.init(allocator),
        };
    }

    pub fn deinit(self: *Model) void {
        self.snapshot_arena.deinit();
        self.diff_arena.deinit();
    }

    pub fn setEnginePath(self: *Model, path: []const u8) void {
        copyFixed(&self.engine_storage, &self.engine_len, path);
    }

    pub fn setStorePath(self: *Model, path: []const u8) void {
        copyFixed(&self.store_storage, &self.store_len, path);
    }

    pub fn repositoryPath(self: *const Model) []const u8 {
        return self.repository_storage[0..self.repository_len];
    }

    pub fn selectedBranch(self: *const Model) ?[]const u8 {
        const snapshot = self.snapshot orelse return null;
        if (self.selected_index >= snapshot.branches.len) return null;
        return snapshot.branches[self.selected_index].name;
    }

    pub fn branchRows(self: *const Model, arena: std.mem.Allocator) []const BranchRow {
        const snapshot = self.snapshot orelse return &.{};
        const rows = arena.alloc(BranchRow, snapshot.branches.len) catch return &.{};
        const filter = self.filter.text();
        var row_count: usize = 0;
        for (snapshot.branches, 0..) |branch, index| {
            if (filter.len > 0 and std.mem.indexOf(u8, branch.name, filter) == null) continue;
            rows[row_count] = .{
                .index = index,
                .name = branch.name,
                .parent = branch.parent orelse "",
                .column = branch.column,
                .is_current = branch.is_current,
                .is_selected = index == self.selected_index,
                .ahead = branch.ahead,
                .behind = branch.behind,
                .status = @tagName(branch.recommended_action),
            };
            row_count += 1;
        }
        return rows[0..row_count];
    }

    pub fn filterText(self: *const Model) []const u8 {
        return self.filter.text();
    }

    pub fn diffRows(self: *const Model, arena: std.mem.Allocator) []const DiffRow {
        const diff = self.diff orelse return &.{};
        const rows = arena.alloc(DiffRow, diff.lines.len) catch return &.{};
        for (diff.lines, 0..) |line, index| rows[index] = .{
            .index = index,
            .text = line.text,
            .kind = line.kind,
            .is_addition = line.kind == .addition,
            .is_deletion = line.kind == .deletion,
            .is_file = line.kind == .file,
            .is_hunk = line.kind == .hunk,
        };
        return rows;
    }

    pub fn recentRows(self: *const Model, arena: std.mem.Allocator) []const RecentRow {
        const rows = arena.alloc(RecentRow, self.recent_count) catch return &.{};
        for (0..self.recent_count) |index| {
            const path = self.recent_storage[index][0..self.recent_lens[index]];
            rows[index] = .{
                .index = index,
                .path = path,
                .label = std.fs.path.basename(path),
            };
        }
        return rows;
    }

    pub fn repositoryName(self: *const Model) []const u8 {
        if (self.snapshot) |snapshot| return snapshot.repository_name;
        if (self.repository_len > 0) return self.repositoryPath();
        return "No repository selected";
    }

    pub fn selectedBranchName(self: *const Model) []const u8 {
        return self.selectedBranch() orelse "No branch selected";
    }

    pub fn selectedParent(self: *const Model) []const u8 {
        const branch = self.selectedBranchSnapshot() orelse return "—";
        return branch.parent orelse "Trunk";
    }

    pub fn selectedDistance(self: *const Model, arena: std.mem.Allocator) []const u8 {
        const branch = self.selectedBranchSnapshot() orelse return "No stack distance";
        return std.fmt.allocPrint(arena, "{d} ahead · {d} behind", .{ branch.ahead, branch.behind }) catch "";
    }

    pub fn selectedPullRequest(self: *const Model, arena: std.mem.Allocator) []const u8 {
        const branch = self.selectedBranchSnapshot() orelse return "No pull request";
        const pull_request = branch.pull_request orelse return "No pull request";
        const draft = if (pull_request.is_draft) " · draft" else "";
        return std.fmt.allocPrint(arena, "PR #{d} · {s}{s}", .{ pull_request.number, pull_request.state, draft }) catch "";
    }

    pub fn selectedCi(self: *const Model) []const u8 {
        const branch = self.selectedBranchSnapshot() orelse return "CI unavailable";
        return branch.ci_state orelse "CI not reported";
    }

    pub fn selectedRecommendation(self: *const Model) []const u8 {
        const branch = self.selectedBranchSnapshot() orelse return "none";
        return switch (branch.recommended_action) {
            .none => "Up to date",
            .checkout => "Checkout recommended",
            .restack => "Restack recommended",
            .submit_stack => "Submit stack recommended",
            .open_pr => "Open pull request",
        };
    }

    pub fn diffSummary(self: *const Model, arena: std.mem.Allocator) []const u8 {
        const diff = self.diff orelse return "No patch loaded";
        return std.fmt.allocPrint(arena, "{d} files · +{d} −{d}", .{ diff.files.len, diff.additions, diff.deletions }) catch "";
    }

    pub fn confirmationTitle(self: *const Model) []const u8 {
        return switch (self.confirmation) {
            .none => "Confirm action",
            .restack => "Restack selected branch?",
            .submit_stack => "Submit this stack?",
        };
    }

    pub fn confirmationMessage(self: *const Model) []const u8 {
        return switch (self.confirmation) {
            .none => "",
            .restack => "This rewrites the selected branch onto its parent.",
            .submit_stack => "This pushes the stack and updates its pull requests.",
        };
    }

    pub fn statusLine(self: *const Model, _: std.mem.Allocator) []const u8 {
        if (self.status_len > 0) return self.status_storage[0..self.status_len];
        if (self.loading_snapshot) return "Loading repository…";
        if (self.loading_diff) return "Loading patch…";
        return "Ready";
    }

    pub fn errorText(self: *const Model) []const u8 {
        return self.error_storage[0..self.error_len];
    }

    pub fn hasError(self: *const Model) bool {
        return self.error_len > 0;
    }

    pub fn hasRepository(self: *const Model) bool {
        return self.repository_len > 0;
    }

    pub fn hasSnapshot(self: *const Model) bool {
        return self.snapshot != null;
    }

    pub fn hasDiff(self: *const Model) bool {
        return self.diff != null;
    }

    pub fn hasBranchSelection(self: *const Model) bool {
        return self.selectedBranchSnapshot() != null;
    }

    pub fn hasConfirmation(self: *const Model) bool {
        return self.confirmation != .none;
    }

    pub fn diffTruncated(self: *const Model) bool {
        return if (self.diff) |diff| diff.truncated else false;
    }

    pub fn actionsDisabled(self: *const Model) bool {
        return self.isBusy() or !self.hasBranchSelection();
    }

    pub fn openPrDisabled(self: *const Model) bool {
        if (self.actionsDisabled()) return true;
        const branch = self.selectedBranchSnapshot() orelse return true;
        return branch.pull_request == null or branch.pull_request.?.url == null;
    }

    pub fn showRepositoryPrompt(self: *const Model) bool {
        return !self.hasRepository() and !self.needs_repository_picker;
    }

    pub fn showEmptyPatch(self: *const Model) bool {
        return !self.loading_diff and self.diff == null;
    }

    pub fn recentsOpen(self: *const Model) bool {
        return self.recents_open;
    }

    pub fn isBusy(self: *const Model) bool {
        return self.loading_snapshot or self.loading_diff or self.active_action != null;
    }

    fn enginePath(self: *const Model) []const u8 {
        return self.engine_storage[0..self.engine_len];
    }

    fn storePath(self: *const Model) []const u8 {
        return self.store_storage[0..self.store_len];
    }

    fn selectedBranchSnapshot(self: *const Model) ?protocol.BranchSnapshot {
        const snapshot = self.snapshot orelse return null;
        if (self.selected_index >= snapshot.branches.len) return null;
        return snapshot.branches[self.selected_index];
    }

    fn moveSelection(self: *Model, fx: *Effects, direction: enum { previous, next }) void {
        const snapshot = self.snapshot orelse return;
        if (snapshot.branches.len == 0) return;
        const next_index = switch (direction) {
            .previous => if (self.selected_index == 0) snapshot.branches.len - 1 else self.selected_index - 1,
            .next => (self.selected_index + 1) % snapshot.branches.len,
        };
        if (next_index == self.selected_index) return;
        self.selected_index = next_index;
        self.selection_generation +%= 1;
        self.requestDiff(fx);
    }

    fn snapshotRequestId(self: *const Model) []const u8 {
        return self.snapshot_request_storage[0..self.snapshot_request_len];
    }

    fn snapshotRepositoryPath(self: *const Model) []const u8 {
        return self.snapshot_repository_storage[0..self.snapshot_repository_len];
    }

    fn diffRequestId(self: *const Model) []const u8 {
        return self.diff_request_storage[0..self.diff_request_len];
    }

    fn actionRequestId(self: *const Model) []const u8 {
        return self.action_request_storage[0..self.action_request_len];
    }

    fn clearDiff(self: *Model) void {
        self.diff_arena.deinit();
        self.diff_arena = std.heap.ArenaAllocator.init(self.allocator);
        self.diff = null;
    }

    fn clearError(self: *Model) void {
        self.error_len = 0;
    }

    fn setError(self: *Model, text: []const u8) void {
        copyFixed(&self.error_storage, &self.error_len, text);
    }

    fn setStatus(self: *Model, text: []const u8) void {
        copyFixed(&self.status_storage, &self.status_len, text);
    }

    fn nextRequestId(self: *Model, prefix: []const u8, storage: []u8, len: *usize) []const u8 {
        self.request_sequence +%= 1;
        if (self.request_sequence == 0) self.request_sequence = 1;
        const value = std.fmt.bufPrint(storage, "{s}-{d}", .{ prefix, self.request_sequence }) catch {
            len.* = 0;
            return "";
        };
        len.* = value.len;
        return value;
    }

    fn requestSnapshotFor(self: *Model, fx: *Effects, repository_path: []const u8) void {
        if (self.engine_len == 0 or repository_path.len == 0) {
            self.setError("The bundled engine or repository path is unavailable.");
            return;
        }
        copyFixed(
            &self.snapshot_repository_storage,
            &self.snapshot_repository_len,
            repository_path,
        );
        self.loading_snapshot = true;
        self.setStatus("Refreshing repository…");
        const request_id = self.nextRequestId(
            "snapshot",
            &self.snapshot_request_storage,
            &self.snapshot_request_len,
        );
        bridge.requestSnapshot(fx, self.enginePath(), self.snapshotRepositoryPath(), request_id);
    }

    fn requestSnapshot(self: *Model, fx: *Effects) void {
        self.requestSnapshotFor(fx, self.repositoryPath());
    }

    fn beginRepositorySelection(self: *Model, fx: *Effects, path: []const u8) void {
        fx.cancel(diff_effect_key);
        self.loading_diff = false;
        self.needs_repository_picker = false;
        self.recents_open = false;
        self.clearError();
        self.requestSnapshotFor(fx, path);
    }

    fn requestDiff(self: *Model, fx: *Effects) void {
        const branch = self.selectedBranch() orelse return;
        self.loading_diff = true;
        self.clearDiff();
        const request_id = self.nextRequestId(
            "diff",
            &self.diff_request_storage,
            &self.diff_request_len,
        );
        bridge.requestDiff(fx, self.enginePath(), self.repositoryPath(), request_id, branch);
    }

    fn startAction(self: *Model, fx: *Effects, action: Action) void {
        if (self.active_action != null) {
            self.setStatus("Another stack action is already running.");
            return;
        }
        const branch = self.selectedBranch() orelse {
            self.setError("Choose a branch before running an action.");
            return;
        };
        self.confirmation = .none;
        self.active_action = action;
        self.action_terminal_seen = false;
        fx.cancel(diff_effect_key);
        self.loading_diff = false;
        const request_id = self.nextRequestId(
            "action",
            &self.action_request_storage,
            &self.action_request_len,
        );
        bridge.requestAction(fx, self.enginePath(), self.repositoryPath(), request_id, action, branch);
    }

    fn requestConfirmation(self: *Model, confirmation: Confirmation) void {
        if (self.active_action != null) {
            self.setStatus("Another stack action is already running.");
            return;
        }
        self.confirmation = confirmation;
    }

    fn addRecent(self: *Model, path: []const u8) void {
        var existing: ?usize = null;
        for (0..self.recent_count) |index| {
            if (std.mem.eql(u8, self.recent_storage[index][0..self.recent_lens[index]], path)) {
                existing = index;
                break;
            }
        }
        const stop = existing orelse @min(self.recent_count, max_recents - 1);
        var index = stop;
        while (index > 0) : (index -= 1) {
            self.recent_storage[index] = self.recent_storage[index - 1];
            self.recent_lens[index] = self.recent_lens[index - 1];
        }
        copyFixed(&self.recent_storage[0], &self.recent_lens[0], path);
        if (self.recent_count < max_recents) self.recent_count += 1;
    }

    fn removeRecentPath(self: *Model, path: []const u8) void {
        var index: usize = 0;
        while (index < self.recent_count) {
            const recent = self.recent_storage[index][0..self.recent_lens[index]];
            if (!std.mem.eql(u8, recent, path)) {
                index += 1;
                continue;
            }
            var next = index;
            while (next + 1 < self.recent_count) : (next += 1) {
                self.recent_storage[next] = self.recent_storage[next + 1];
                self.recent_lens[next] = self.recent_lens[next + 1];
            }
            self.recent_count -= 1;
            self.recent_lens[self.recent_count] = 0;
        }
    }

    fn persistRecents(self: *Model, fx: *Effects) void {
        self.recent_serialized_len = 0;
        for (0..self.recent_count) |index| {
            const path = self.recent_storage[index][0..self.recent_lens[index]];
            if (self.recent_serialized_len + path.len + 1 > self.recent_serialized.len) break;
            @memcpy(
                self.recent_serialized[self.recent_serialized_len .. self.recent_serialized_len + path.len],
                path,
            );
            self.recent_serialized_len += path.len;
            self.recent_serialized[self.recent_serialized_len] = '\n';
            self.recent_serialized_len += 1;
        }
        if (self.store_len > 0) {
            bridge.writeRecents(
                fx,
                self.storePath(),
                self.recent_serialized[0..self.recent_serialized_len],
            );
        }
    }
};

pub fn boot(model: *Model, fx: *Effects) void {
    if (model.store_len == 0) {
        model.needs_repository_picker = true;
        return;
    }
    bridge.readRecents(fx, model.storePath());
}

pub fn update(model: *Model, msg: Msg, fx: *Effects) void {
    switch (msg) {
        .repository_selected => |path| {
            model.beginRepositorySelection(fx, path);
        },
        .repository_picker_cancelled => model.needs_repository_picker = false,
        .recents_loaded => |result| handleRecentsLoaded(model, result, fx),
        .recents_saved => |result| {
            if (result.outcome != .ok) model.setStatus("Recent repositories could not be saved.");
        },
        .snapshot_exit => |exit| handleSnapshotExit(model, exit, fx),
        .diff_exit => |exit| handleDiffExit(model, exit),
        .action_line => |line| handleActionLine(model, line),
        .action_exit => |exit| handleActionExit(model, exit, fx),
        .select_branch => |index| {
            const snapshot = model.snapshot orelse return;
            if (index >= snapshot.branches.len or index == model.selected_index) return;
            model.selected_index = index;
            model.selection_generation +%= 1;
            model.requestDiff(fx);
        },
        .select_recent => |index| {
            if (index >= model.recent_count) return;
            const path = model.recent_storage[index][0..model.recent_lens[index]];
            model.beginRepositorySelection(fx, path);
        },
        .select_next => model.moveSelection(fx, .next),
        .select_previous => model.moveSelection(fx, .previous),
        .toggle_recents => model.recents_open = !model.recents_open,
        .close_recents => model.recents_open = false,
        .request_checkout => model.startAction(fx, .checkout),
        .request_restack => model.requestConfirmation(.restack),
        .request_submit => model.requestConfirmation(.submit_stack),
        .request_open_pr => model.startAction(fx, .open_pr),
        .confirm_action => switch (model.confirmation) {
            .none => {},
            .restack => model.startAction(fx, .restack),
            .submit_stack => model.startAction(fx, .submit_stack),
        },
        .cancel_confirmation => model.confirmation = .none,
        .refresh => if (model.active_action == null and !model.loading_snapshot) model.requestSnapshot(fx),
        .app_activated => if (model.hasRepository() and model.active_action == null and !model.loading_snapshot) model.requestSnapshot(fx),
        .choose_repository => {
            model.recents_open = false;
            model.needs_repository_picker = true;
        },
        .focus_filter => model.filter_focused = true,
        .stack_resized => |fraction| model.pane_stack_ratio = fraction,
        .inspector_resized => |fraction| model.pane_inspector_ratio = fraction,
        .copy_diagnostics => fx.writeClipboard(.{
            .key = clipboard_effect_key,
            .text = model.errorText(),
            .on_result = Effects.clipboardMsg(.diagnostics_copied),
        }),
        .diagnostics_copied => |result| {
            if (result.outcome == .ok) {
                model.setStatus("Diagnostics copied.");
            } else {
                model.setStatus("Diagnostics could not be copied.");
            }
        },
        .dismiss => {
            model.confirmation = .none;
            model.clearError();
        },
        .dismiss_error => model.clearError(),
        .filter_changed => |event| {
            model.filter_focused = false;
            model.filter.apply(event);
        },
    }
}

fn handleRecentsLoaded(model: *Model, result: native_sdk.EffectFileResult, fx: *Effects) void {
    if (result.outcome == .not_found) {
        model.needs_repository_picker = true;
        return;
    }
    if (result.outcome != .ok) {
        model.needs_repository_picker = true;
        model.setError("Recent repositories could not be read; choose a repository.");
        return;
    }
    var lines = std.mem.splitScalar(u8, result.bytes, '\n');
    while (lines.next()) |line| {
        const path = std.mem.trim(u8, line, " \t\r");
        if (path.len == 0) continue;
        if (model.recent_count >= max_recents) break;
        var duplicate = false;
        for (0..model.recent_count) |index| {
            if (std.mem.eql(u8, model.recent_storage[index][0..model.recent_lens[index]], path)) {
                duplicate = true;
                break;
            }
        }
        if (!duplicate) {
            copyFixed(
                &model.recent_storage[model.recent_count],
                &model.recent_lens[model.recent_count],
                path,
            );
            model.recent_count += 1;
        }
    }
    if (model.recent_count == 0) {
        model.needs_repository_picker = true;
        return;
    }
    const first = model.recent_storage[0][0..model.recent_lens[0]];
    model.beginRepositorySelection(fx, first);
}

fn handleSnapshotExit(model: *Model, exit: native_sdk.EffectExit, fx: *Effects) void {
    if (exit.reason == .cancelled) return;
    if (!validateCollectExit(model, exit, "snapshot")) {
        model.loading_snapshot = false;
        return;
    }

    var next_arena = std.heap.ArenaAllocator.init(model.allocator);
    var keep_arena = false;
    defer if (!keep_arena) next_arena.deinit();
    const envelope = protocol.parseTerminal(
        protocol.RepositorySnapshot,
        next_arena.allocator(),
        exit.output,
    ) catch |err| {
        model.loading_snapshot = false;
        setParseError(model, err);
        return;
    };
    if (!std.mem.eql(u8, envelope.request_id, model.snapshotRequestId())) return;
    model.loading_snapshot = false;
    if (!envelope.ok) {
        setProtocolError(model, envelope.@"error".?);
        if (std.mem.eql(u8, envelope.@"error".?.code, "invalid_repository")) {
            model.removeRecentPath(model.snapshotRepositoryPath());
            model.persistRecents(fx);
            model.needs_repository_picker = true;
        }
        return;
    }

    const snapshot = envelope.data.?;
    model.snapshot_arena.deinit();
    model.snapshot_arena = next_arena;
    keep_arena = true;
    model.snapshot = snapshot;
    model.removeRecentPath(model.snapshotRepositoryPath());
    copyFixed(
        &model.repository_storage,
        &model.repository_len,
        snapshot.repository_path,
    );
    model.addRecent(snapshot.repository_path);
    model.persistRecents(fx);
    model.clearError();
    model.status_len = 0;
    model.selected_index = 0;
    for (snapshot.branches, 0..) |branch, index| {
        if (branch.is_current) {
            model.selected_index = index;
            break;
        }
    }
    model.selection_generation +%= 1;
    model.requestDiff(fx);
}

fn handleDiffExit(model: *Model, exit: native_sdk.EffectExit) void {
    if (exit.reason == .cancelled) return;
    if (!validateCollectExit(model, exit, "diff")) {
        model.loading_diff = false;
        return;
    }

    var next_arena = std.heap.ArenaAllocator.init(model.allocator);
    var keep_arena = false;
    defer if (!keep_arena) next_arena.deinit();
    const envelope = protocol.parseTerminal(
        protocol.DiffSnapshot,
        next_arena.allocator(),
        exit.output,
    ) catch |err| {
        model.loading_diff = false;
        setParseError(model, err);
        return;
    };
    if (!std.mem.eql(u8, envelope.request_id, model.diffRequestId())) return;
    model.loading_diff = false;
    if (!envelope.ok) {
        setProtocolError(model, envelope.@"error".?);
        return;
    }
    const snapshot = model.snapshot orelse return;
    const diff = envelope.data.?;
    if (!std.mem.eql(u8, diff.generation, snapshot.generation)) return;
    model.diff_arena.deinit();
    model.diff_arena = next_arena;
    keep_arena = true;
    model.diff = diff;
    model.clearError();
}

fn handleActionLine(model: *Model, line: native_sdk.EffectLine) void {
    if (line.truncated or line.dropped_before > 0) {
        model.setError("The action response was truncated; refresh before retrying.");
        return;
    }
    var arena = std.heap.ArenaAllocator.init(model.allocator);
    defer arena.deinit();
    if (protocol.parseProgress(arena.allocator(), line.line)) |progress| {
        if (std.mem.eql(u8, progress.request_id, model.actionRequestId())) {
            model.setStatus(progress.message);
        }
        return;
    } else |_| {}

    const terminal = protocol.parseTerminal(
        protocol.ActionResult,
        arena.allocator(),
        line.line,
    ) catch {
        model.setError("The action produced malformed bridge output.");
        return;
    };
    if (!std.mem.eql(u8, terminal.request_id, model.actionRequestId())) return;
    model.action_terminal_seen = true;
    if (terminal.ok) {
        model.setStatus(terminal.data.?.summary);
        model.clearError();
    } else {
        setProtocolError(model, terminal.@"error".?);
    }
}

fn handleActionExit(model: *Model, exit: native_sdk.EffectExit, fx: *Effects) void {
    if (exit.reason != .exited) {
        model.setError("The stack action could not start or was interrupted.");
    } else if (exit.dropped_lines > 0) {
        model.setError("The stack action lost bridge events; refresh before retrying.");
    } else if (exit.code != 0 and !model.action_terminal_seen) {
        model.setError("The stack action failed without a terminal result.");
    }
    model.active_action = null;
    model.confirmation = .none;
    model.requestSnapshot(fx);
}

fn validateCollectExit(model: *Model, exit: native_sdk.EffectExit, label: []const u8) bool {
    if (exit.output_truncated) {
        model.setError("The bridge response was truncated; reinstall the app or refresh.");
        return false;
    }
    if (exit.reason != .exited) {
        if (exit.reason == .spawn_failed or exit.reason == .rejected) {
            model.setError("The bundled engine could not start; reinstall the app.");
        } else {
            model.setError("The bundled engine was interrupted; refresh and retry.");
        }
        return false;
    }
    if (exit.output.len == 0) {
        _ = label;
        model.setError("The bundled engine returned no terminal result.");
        return false;
    }
    return true;
}

fn setParseError(model: *Model, err: anyerror) void {
    if (err == error.UnsupportedSchema) {
        model.setError("The desktop engine schema is incompatible; reinstall the app.");
    } else {
        model.setError("The desktop engine returned malformed JSON.");
    }
}

fn setProtocolError(model: *Model, value: protocol.ProtocolError) void {
    var buffer: [max_error_bytes]u8 = undefined;
    const text = std.fmt.bufPrint(&buffer, "{s} {s}", .{ value.message, value.details }) catch value.message;
    model.setError(text);
}

fn copyFixed(storage: []u8, len: *usize, value: []const u8) void {
    const copy_len = @min(storage.len, value.len);
    @memcpy(storage[0..copy_len], value[0..copy_len]);
    len.* = copy_len;
}
