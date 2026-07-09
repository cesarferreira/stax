const std = @import("std");
const native_sdk = @import("native_sdk");
const model_mod = @import("model.zig");
const protocol = @import("protocol.zig");

const testing = std.testing;
const Effects = native_sdk.Effects(model_mod.Msg);
const canvas = native_sdk.canvas;
const WorkspaceUi = canvas.Ui(model_mod.Msg);
const WorkspaceView = canvas.CompiledMarkupView(
    model_mod.Model,
    model_mod.Msg,
    @embedFile("app.native"),
);

const snapshot_json =
    \\{
    \\  "schema_version": 1,
    \\  "request_id": "req-snapshot",
    \\  "type": "result",
    \\  "ok": true,
    \\  "data": {
    \\    "generation": "0123456789abcdef",
    \\    "repository_path": "/tmp/example",
    \\    "repository_name": "example",
    \\    "trunk": "main",
    \\    "current_branch": "feature/ui",
    \\    "repository_state": "normal",
    \\    "dirty": false,
    \\    "branches": [
    \\      {
    \\        "name": "feature/ui",
    \\        "parent": "main",
    \\        "column": 0,
    \\        "is_current": true,
    \\        "is_trunk": false,
    \\        "ahead": 2,
    \\        "behind": 0,
    \\        "needs_restack": false,
    \\        "has_remote": true,
    \\        "unpushed": 1,
    \\        "unpulled": 0,
    \\        "pull_request": {
    \\          "number": 42,
    \\          "state": "open",
    \\          "is_draft": false,
    \\          "url": "https://github.com/example/repo/pull/42"
    \\        },
    \\        "ci_state": "success",
    \\        "recommended_action": "submit_stack"
    \\      },
    \\      {
    \\        "name": "main",
    \\        "parent": null,
    \\        "column": 0,
    \\        "is_current": false,
    \\        "is_trunk": true,
    \\        "ahead": 0,
    \\        "behind": 0,
    \\        "needs_restack": false,
    \\        "has_remote": true,
    \\        "unpushed": 0,
    \\        "unpulled": 0,
    \\        "pull_request": null,
    \\        "ci_state": null,
    \\        "recommended_action": "none"
    \\      }
    \\    ]
    \\  }
    \\}
;

test "parses a complete repository snapshot" {
    var arena_state = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena_state.deinit();

    const envelope = try protocol.parseTerminal(
        protocol.RepositorySnapshot,
        arena_state.allocator(),
        snapshot_json,
    );
    try testing.expect(envelope.ok);
    try testing.expectEqualStrings("0123456789abcdef", envelope.data.?.generation);
    try testing.expectEqual(protocol.RepositoryState.normal, envelope.data.?.repository_state);
    try testing.expectEqual(@as(usize, 2), envelope.data.?.branches.len);
    try testing.expectEqualStrings("feature/ui", envelope.data.?.branches[0].name);
    try testing.expectEqual(
        protocol.RecommendedAction.submit_stack,
        envelope.data.?.branches[0].recommended_action,
    );
    try testing.expectEqual(@as(u64, 42), envelope.data.?.branches[0].pull_request.?.number);
}

test "parses a truncated structured diff" {
    var arena_state = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena_state.deinit();
    const diff_json =
        \\{"schema_version":1,"request_id":"req-diff","type":"result","ok":true,"data":{"generation":"0123456789abcdef","branch":"feature/ui","parent":"main","additions":1,"deletions":0,"files":[{"path":"src/app.zig","additions":1,"deletions":0}],"lines":[{"kind":"addition","text":"+hello"}],"truncated":true}}
    ;

    const envelope = try protocol.parseTerminal(
        protocol.DiffSnapshot,
        arena_state.allocator(),
        diff_json,
    );
    try testing.expect(envelope.ok);
    try testing.expect(envelope.data.?.truncated);
    try testing.expectEqual(protocol.DiffLineKind.addition, envelope.data.?.lines[0].kind);
}

test "parses a terminal protocol error" {
    var arena_state = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena_state.deinit();
    const error_json =
        \\{"schema_version":1,"request_id":"req-error","type":"result","ok":false,"error":{"code":"invalid_repository","message":"Not a repository","details":"missing .git","recovery":"choose_repository"}}
    ;

    const envelope = try protocol.parseTerminal(
        protocol.RepositorySnapshot,
        arena_state.allocator(),
        error_json,
    );
    try testing.expect(!envelope.ok);
    try testing.expectEqualStrings("invalid_repository", envelope.@"error".?.code);
    try testing.expectEqualStrings("choose_repository", envelope.@"error".?.recovery);
}

test "parses a progress event" {
    var arena_state = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena_state.deinit();
    const progress_json =
        \\{"schema_version":1,"request_id":"req-action","type":"progress","phase":"restacking","message":"Restacking feature/ui"}
    ;

    const event = try protocol.parseProgress(arena_state.allocator(), progress_json);
    try testing.expectEqualStrings("restacking", event.phase);
    try testing.expectEqualStrings("Restacking feature/ui", event.message);
}

test "rejects malformed JSON and unsupported schemas" {
    var arena_state = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena_state.deinit();

    if (protocol.parseTerminal(
        protocol.RepositorySnapshot,
        arena_state.allocator(),
        "{",
    )) |_| {
        return error.ExpectedMalformedJsonFailure;
    } else |_| {}
    try testing.expectError(error.UnsupportedSchema, protocol.expectSchema(2));
}

fn snapshotFixture(allocator: std.mem.Allocator, request_id: []const u8) ![]u8 {
    return std.fmt.allocPrint(
        allocator,
        "{{\"schema_version\":1,\"request_id\":\"{s}\",\"type\":\"result\",\"ok\":true,\"data\":{{\"generation\":\"generation-one\",\"repository_path\":\"/tmp/repo\",\"repository_name\":\"repo\",\"trunk\":\"main\",\"current_branch\":\"feature/ui\",\"repository_state\":\"normal\",\"dirty\":false,\"branches\":[{{\"name\":\"feature/ui\",\"parent\":\"main\",\"column\":0,\"is_current\":true,\"is_trunk\":false,\"ahead\":1,\"behind\":0,\"needs_restack\":false,\"has_remote\":false,\"unpushed\":0,\"unpulled\":0,\"pull_request\":null,\"ci_state\":null,\"recommended_action\":\"submit_stack\"}},{{\"name\":\"main\",\"parent\":null,\"column\":0,\"is_current\":false,\"is_trunk\":true,\"ahead\":0,\"behind\":0,\"needs_restack\":false,\"has_remote\":false,\"unpushed\":0,\"unpulled\":0,\"pull_request\":null,\"ci_state\":null,\"recommended_action\":\"none\"}}]}}}}",
        .{request_id},
    );
}

fn diffFixture(
    allocator: std.mem.Allocator,
    request_id: []const u8,
    generation: []const u8,
) ![]u8 {
    return std.fmt.allocPrint(
        allocator,
        "{{\"schema_version\":1,\"request_id\":\"{s}\",\"type\":\"result\",\"ok\":true,\"data\":{{\"generation\":\"{s}\",\"branch\":\"feature/ui\",\"parent\":\"main\",\"additions\":1,\"deletions\":0,\"files\":[],\"lines\":[],\"truncated\":false}}}}",
        .{ request_id, generation },
    );
}

fn initModel() model_mod.Model {
    var model = model_mod.Model.init(testing.allocator);
    model.setEnginePath("/Applications/Stax.app/Contents/Resources/bin/st");
    model.setStorePath("/tmp/stax-desktop/recent-repositories");
    return model;
}

fn drainEffects(model: *model_mod.Model, fx: *Effects) void {
    while (fx.takeMsg()) |msg| model_mod.update(model, msg, fx);
}

fn loadSnapshot(model: *model_mod.Model, fx: *Effects) !void {
    model_mod.update(model, .{ .repository_selected = "/tmp/repo" }, fx);
    const request = fx.pendingSpawnAt(0).?;
    const json = try snapshotFixture(testing.allocator, request.argv[8]);
    defer testing.allocator.free(json);
    try fx.feedOutput(model_mod.snapshot_effect_key, json);
    try fx.feedExit(model_mod.snapshot_effect_key, 0);
    drainEffects(model, fx);
}

test "boot requests persisted recent repositories" {
    var model = initModel();
    defer model.deinit();
    var fx = Effects.init(testing.allocator);
    defer fx.deinit();
    fx.executor = .fake;

    model_mod.boot(&model, &fx);

    try testing.expectEqual(@as(usize, 1), fx.pendingFileCount());
    const request = fx.pendingFileAt(0).?;
    try testing.expectEqual(model_mod.recents_read_effect_key, request.key);
    try testing.expectEqualStrings("/tmp/stax-desktop/recent-repositories", request.path);
}

test "repository selection builds the exact snapshot argv" {
    var model = initModel();
    defer model.deinit();
    var fx = Effects.init(testing.allocator);
    defer fx.deinit();
    fx.executor = .fake;

    model_mod.update(&model, .{ .repository_selected = "/tmp/repo" }, &fx);

    try testing.expectEqual(@as(usize, 1), fx.pendingSpawnCount());
    const request = fx.pendingSpawnAt(0).?;
    try testing.expectEqual(model_mod.snapshot_effect_key, request.key);
    try testing.expectEqualDeep(
        @as([]const []const u8, &.{
            "/Applications/Stax.app/Contents/Resources/bin/st",
            "desktop",
            "snapshot",
            "--repo",
            "/tmp/repo",
            "--schema-version",
            "1",
            "--request-id",
            request.argv[8],
        }),
        request.argv,
    );
}

test "snapshot success selects current branch and requests its diff" {
    var model = initModel();
    defer model.deinit();
    var fx = Effects.init(testing.allocator);
    defer fx.deinit();
    fx.executor = .fake;

    try loadSnapshot(&model, &fx);

    try testing.expectEqualStrings("feature/ui", model.selectedBranch().?);
    try testing.expectEqual(@as(usize, 1), fx.pendingSpawnCount());
    const request = fx.pendingSpawnAt(0).?;
    try testing.expectEqual(model_mod.diff_effect_key, request.key);
    try testing.expectEqualStrings("diff", request.argv[2]);
    try testing.expectEqualStrings("feature/ui", request.argv[10]);
}

test "parsed snapshots own strings after the effect scratch is reused" {
    var model = initModel();
    defer model.deinit();
    var fx = Effects.init(testing.allocator);
    defer fx.deinit();
    fx.executor = .fake;
    try loadSnapshot(&model, &fx);
    const request = fx.pendingSpawnAt(0).?;
    const json = try diffFixture(testing.allocator, request.argv[8], "generation-one");
    defer testing.allocator.free(json);

    try fx.feedOutput(model_mod.diff_effect_key, json);
    try fx.feedExit(model_mod.diff_effect_key, 0);
    drainEffects(&model, &fx);

    try testing.expect(model.diff != null);
    try testing.expectEqualStrings("feature/ui", model.selectedBranch().?);
    try testing.expectEqualStrings("repo", model.snapshot.?.repository_name);
}

test "selection generation ignores late diff responses" {
    var model = initModel();
    defer model.deinit();
    var fx = Effects.init(testing.allocator);
    defer fx.deinit();
    fx.executor = .fake;
    try loadSnapshot(&model, &fx);
    const old_request = fx.pendingSpawnAt(0).?;
    var old_request_id: [64]u8 = undefined;
    @memcpy(old_request_id[0..old_request.argv[8].len], old_request.argv[8]);
    const old_id = old_request_id[0..old_request.argv[8].len];
    const previous_generation = model.selection_generation;

    model_mod.update(&model, .{ .select_branch = 1 }, &fx);
    var found_new_diff = false;
    var request_index: usize = 0;
    while (request_index < fx.pendingSpawnCount()) : (request_index += 1) {
        const request = fx.pendingSpawnAt(request_index).?;
        if (request.key == model_mod.diff_effect_key and
            std.mem.eql(u8, request.argv[10], "main"))
        {
            found_new_diff = true;
        }
    }
    try testing.expect(found_new_diff);
    const late_json = try diffFixture(testing.allocator, old_id, "generation-one");
    defer testing.allocator.free(late_json);
    model_mod.update(&model, .{ .diff_exit = .{
        .key = model_mod.diff_effect_key,
        .code = 0,
        .reason = .exited,
        .output = late_json,
    } }, &fx);

    try testing.expect(model.selection_generation > previous_generation);
    try testing.expect(model.diff == null);
}

test "destructive actions confirm and only one mutation starts" {
    var model = initModel();
    defer model.deinit();
    var fx = Effects.init(testing.allocator);
    defer fx.deinit();
    fx.executor = .fake;
    try loadSnapshot(&model, &fx);

    model_mod.update(&model, .request_restack, &fx);
    try testing.expectEqual(model_mod.Confirmation.restack, model.confirmation);
    try testing.expect(fx.pendingSpawnAt(1) == null);

    model_mod.update(&model, .confirm_action, &fx);
    try testing.expect(model.active_action != null);
    const count = fx.pendingSpawnCount();
    model_mod.update(&model, .request_submit, &fx);
    try testing.expectEqual(count, fx.pendingSpawnCount());
}

test "action completion clears mutation and refreshes snapshot" {
    var model = initModel();
    defer model.deinit();
    var fx = Effects.init(testing.allocator);
    defer fx.deinit();
    fx.executor = .fake;
    try loadSnapshot(&model, &fx);
    model_mod.update(&model, .request_checkout, &fx);
    try testing.expect(model.active_action != null);

    model_mod.update(&model, .{ .action_exit = .{
        .key = model_mod.action_effect_key,
        .code = 0,
        .reason = .exited,
    } }, &fx);

    try testing.expect(model.active_action == null);
    try testing.expect(model.loading_snapshot);
    var found_snapshot = false;
    var index: usize = 0;
    while (index < fx.pendingSpawnCount()) : (index += 1) {
        if (fx.pendingSpawnAt(index).?.key == model_mod.snapshot_effect_key) found_snapshot = true;
    }
    try testing.expect(found_snapshot);
}

test "bridge exits surface truncation malformed schema and spawn failures" {
    var model = initModel();
    defer model.deinit();
    var fx = Effects.init(testing.allocator);
    defer fx.deinit();
    fx.executor = .fake;
    model_mod.update(&model, .{ .repository_selected = "/tmp/repo" }, &fx);
    const request_id = fx.pendingSpawnAt(0).?.argv[8];

    model_mod.update(&model, .{ .snapshot_exit = .{
        .key = model_mod.snapshot_effect_key,
        .code = 0,
        .reason = .exited,
        .output = "{}",
        .output_truncated = true,
    } }, &fx);
    try testing.expect(std.mem.indexOf(u8, model.errorText(), "truncated") != null);

    model_mod.update(&model, .{ .snapshot_exit = .{
        .key = model_mod.snapshot_effect_key,
        .code = 0,
        .reason = .exited,
        .output = "{",
    } }, &fx);
    try testing.expect(std.mem.indexOf(u8, model.errorText(), "malformed") != null);

    const schema_json = try std.fmt.allocPrint(
        testing.allocator,
        "{{\"schema_version\":2,\"request_id\":\"{s}\",\"type\":\"result\",\"ok\":false,\"error\":{{\"code\":\"unsupported\",\"message\":\"bad\",\"details\":\"bad\",\"recovery\":\"reinstall_app\"}}}}",
        .{request_id},
    );
    defer testing.allocator.free(schema_json);
    model_mod.update(&model, .{ .snapshot_exit = .{
        .key = model_mod.snapshot_effect_key,
        .code = 1,
        .reason = .exited,
        .output = schema_json,
    } }, &fx);
    try testing.expect(std.mem.indexOf(u8, model.errorText(), "schema") != null);

    model_mod.update(&model, .{ .snapshot_exit = .{
        .key = model_mod.snapshot_effect_key,
        .reason = .spawn_failed,
    } }, &fx);
    try testing.expect(std.mem.indexOf(u8, model.errorText(), "start") != null);
}

fn findByText(widget: canvas.Widget, value: []const u8) ?canvas.Widget {
    if (std.mem.eql(u8, widget.text, value)) return widget;
    for (widget.children) |child| {
        if (findByText(child, value)) |found| return found;
    }
    return null;
}

fn findRowSelecting(tree: WorkspaceUi.Tree, widget: canvas.Widget, index: usize) ?canvas.Widget {
    if (widget.kind == .list_item) {
        if (tree.msgForPointer(widget.id, .up)) |msg| {
            if (msg == .select_branch and msg.select_branch == index) return widget;
        }
    }
    for (widget.children) |child| {
        if (findRowSelecting(tree, child, index)) |found| return found;
    }
    return null;
}

test "workspace renders the stack branch and patch panes" {
    var arena_state = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena_state.deinit();
    var model = initModel();
    defer model.deinit();
    var ui = WorkspaceUi.init(arena_state.allocator());

    const tree = try ui.finalize(WorkspaceView.build(&ui, &model));

    try testing.expect(findByText(tree.root, "Stack") != null);
    try testing.expect(findByText(tree.root, "Branch") != null);
    try testing.expect(findByText(tree.root, "Patch") != null);
}

test "workspace branch rows dispatch typed selection messages" {
    var arena_state = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena_state.deinit();
    var model = initModel();
    defer model.deinit();
    var fx = Effects.init(testing.allocator);
    defer fx.deinit();
    fx.executor = .fake;
    try loadSnapshot(&model, &fx);
    var ui = WorkspaceUi.init(arena_state.allocator());
    const tree = try ui.finalize(WorkspaceView.build(&ui, &model));

    const main_row = findRowSelecting(tree, tree.root, 1).?;
    const msg = tree.msgForPointer(main_row.id, .up).?;

    try testing.expectEqual(@as(usize, 1), msg.select_branch);
}

test "split resize messages round trip through the model" {
    var model = initModel();
    defer model.deinit();
    var fx = Effects.init(testing.allocator);
    defer fx.deinit();
    fx.executor = .fake;

    model_mod.update(&model, .{ .stack_resized = 0.38 }, &fx);
    model_mod.update(&model, .{ .inspector_resized = 0.47 }, &fx);

    try testing.expectApproxEqAbs(@as(f32, 0.38), model.pane_stack_ratio, 0.001);
    try testing.expectApproxEqAbs(@as(f32, 0.47), model.pane_inspector_ratio, 0.001);
}
