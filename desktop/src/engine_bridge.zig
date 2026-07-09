pub const snapshot_key: u64 = 1;
pub const diff_key: u64 = 2;
pub const action_key: u64 = 3;
pub const recents_read_key: u64 = 4;
pub const recents_write_key: u64 = 5;
pub const clipboard_key: u64 = 6;

pub const Action = enum {
    checkout,
    restack,
    submit_stack,
    open_pr,
};

pub fn requestSnapshot(
    fx: anytype,
    engine_path: []const u8,
    repo_path: []const u8,
    request_id: []const u8,
) void {
    const Fx = @TypeOf(fx.*);
    const argv = [_][]const u8{
        engine_path,
        "desktop",
        "snapshot",
        "--repo",
        repo_path,
        "--schema-version",
        "1",
        "--request-id",
        request_id,
    };
    fx.spawn(.{
        .key = snapshot_key,
        .argv = &argv,
        .output = .collect,
        .on_exit = Fx.exitMsg(.snapshot_exit),
    });
}

pub fn requestDiff(
    fx: anytype,
    engine_path: []const u8,
    repo_path: []const u8,
    request_id: []const u8,
    branch: []const u8,
) void {
    const Fx = @TypeOf(fx.*);
    fx.cancel(diff_key);
    const argv = [_][]const u8{
        engine_path,
        "desktop",
        "diff",
        "--repo",
        repo_path,
        "--schema-version",
        "1",
        "--request-id",
        request_id,
        "--branch",
        branch,
    };
    fx.spawn(.{
        .key = diff_key,
        .argv = &argv,
        .output = .collect,
        .on_exit = Fx.exitMsg(.diff_exit),
    });
}

pub fn requestAction(
    fx: anytype,
    engine_path: []const u8,
    repo_path: []const u8,
    request_id: []const u8,
    action: Action,
    branch: []const u8,
) void {
    const Fx = @TypeOf(fx.*);
    const argv = [_][]const u8{
        engine_path,
        "desktop",
        "action",
        "--repo",
        repo_path,
        "--schema-version",
        "1",
        "--request-id",
        request_id,
        "--action",
        actionName(action),
        "--branch",
        branch,
    };
    fx.spawn(.{
        .key = action_key,
        .argv = &argv,
        .output = .lines,
        .max_line_bytes = 256 * 1024,
        .on_line = Fx.lineMsg(.action_line),
        .on_exit = Fx.exitMsg(.action_exit),
    });
}

pub fn readRecents(fx: anytype, store_path: []const u8) void {
    const Fx = @TypeOf(fx.*);
    fx.readFile(.{
        .key = recents_read_key,
        .path = store_path,
        .on_result = Fx.fileMsg(.recents_loaded),
    });
}

pub fn writeRecents(fx: anytype, store_path: []const u8, bytes: []const u8) void {
    const Fx = @TypeOf(fx.*);
    fx.writeFile(.{
        .key = recents_write_key,
        .path = store_path,
        .bytes = bytes,
        .on_result = Fx.fileMsg(.recents_saved),
    });
}

pub fn actionName(action: Action) []const u8 {
    return switch (action) {
        .checkout => "checkout",
        .restack => "restack",
        .submit_stack => "submit-stack",
        .open_pr => "open-pr",
    };
}
