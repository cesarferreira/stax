const std = @import("std");

pub const schema_version: u32 = 1;

pub const ProtocolError = struct {
    code: []const u8,
    message: []const u8,
    details: []const u8,
    recovery: []const u8,
};

pub const ProgressEvent = struct {
    schema_version: u32,
    request_id: []const u8,
    @"type": []const u8,
    phase: []const u8,
    message: []const u8,
};

pub const RepositoryState = enum {
    normal,
    rebase_in_progress,
    conflict_in_progress,
};

pub const RecommendedAction = enum {
    none,
    checkout,
    restack,
    submit_stack,
    open_pr,
};

pub const PullRequestSnapshot = struct {
    number: u64,
    state: []const u8,
    is_draft: bool,
    url: ?[]const u8,
};

pub const BranchSnapshot = struct {
    name: []const u8,
    parent: ?[]const u8,
    column: usize,
    is_current: bool,
    is_trunk: bool,
    ahead: usize,
    behind: usize,
    needs_restack: bool,
    has_remote: bool,
    unpushed: usize,
    unpulled: usize,
    pull_request: ?PullRequestSnapshot,
    ci_state: ?[]const u8,
    recommended_action: RecommendedAction,
};

pub const RepositorySnapshot = struct {
    generation: []const u8,
    repository_path: []const u8,
    repository_name: []const u8,
    trunk: []const u8,
    current_branch: []const u8,
    repository_state: RepositoryState,
    dirty: bool,
    branches: []const BranchSnapshot,
};

pub const DiffLineKind = enum {
    file,
    hunk,
    context,
    addition,
    deletion,
    metadata,
};

pub const DiffFileSnapshot = struct {
    path: []const u8,
    additions: usize,
    deletions: usize,
};

pub const DiffLineSnapshot = struct {
    kind: DiffLineKind,
    text: []const u8,
};

pub const DiffSnapshot = struct {
    generation: []const u8,
    branch: []const u8,
    parent: []const u8,
    additions: usize,
    deletions: usize,
    files: []const DiffFileSnapshot,
    lines: []const DiffLineSnapshot,
    truncated: bool,
};

pub const ActionResult = struct {
    action: []const u8,
    branch: ?[]const u8,
    summary: []const u8,
};

pub fn TerminalEnvelope(comptime T: type) type {
    return struct {
        schema_version: u32,
        request_id: []const u8,
        @"type": []const u8,
        ok: bool,
        data: ?T = null,
        @"error": ?ProtocolError = null,
    };
}

pub fn expectSchema(received: u32) !void {
    if (received != schema_version) return error.UnsupportedSchema;
}

pub fn parseTerminal(
    comptime T: type,
    allocator: std.mem.Allocator,
    bytes: []const u8,
) !TerminalEnvelope(T) {
    const value = try std.json.parseFromSliceLeaky(
        TerminalEnvelope(T),
        allocator,
        bytes,
        .{ .ignore_unknown_fields = false },
    );
    try expectSchema(value.schema_version);
    if (!std.mem.eql(u8, value.@"type", "result")) return error.UnexpectedEventType;
    if (value.ok == (value.data == null)) return error.InvalidTerminalEnvelope;
    if (value.ok == (value.@"error" != null)) return error.InvalidTerminalEnvelope;
    return value;
}

pub fn parseProgress(allocator: std.mem.Allocator, bytes: []const u8) !ProgressEvent {
    const value = try std.json.parseFromSliceLeaky(
        ProgressEvent,
        allocator,
        bytes,
        .{ .ignore_unknown_fields = false },
    );
    try expectSchema(value.schema_version);
    if (!std.mem.eql(u8, value.@"type", "progress")) return error.UnexpectedEventType;
    return value;
}
