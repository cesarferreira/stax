const std = @import("std");
const protocol = @import("protocol.zig");

const testing = std.testing;

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
