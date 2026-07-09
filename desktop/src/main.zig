const std = @import("std");
const builtin = @import("builtin");
const runner = @import("runner");
const native_sdk = @import("native_sdk");

const model_mod = @import("model.zig");

pub const panic = std.debug.FullPanic(native_sdk.debug.capturePanic);
pub const Model = model_mod.Model;
pub const Msg = model_mod.Msg;

const canvas = native_sdk.canvas;
const geometry = native_sdk.geometry;

const canvas_label = "stax-canvas";
const window_width: f32 = 1180;
const window_height: f32 = 760;

const app_permissions = [_][]const u8{
    native_sdk.security.permission_command,
    native_sdk.security.permission_view,
    native_sdk.security.permission_dialog,
    native_sdk.security.permission_filesystem,
    native_sdk.security.permission_clipboard,
};
const shell_views = [_]native_sdk.ShellView{.{
    .label = canvas_label,
    .kind = .gpu_surface,
    .fill = true,
    .role = "Stax workspace",
    .accessibility_label = "Stax",
    .gpu_backend = .metal,
    .gpu_pixel_format = .bgra8_unorm,
    .gpu_present_mode = .timer,
    .gpu_alpha_mode = .@"opaque",
    .gpu_color_space = .srgb,
    .gpu_vsync = true,
}};
const shell_windows = [_]native_sdk.ShellWindow{.{
    .label = "main",
    .title = "Stax",
    .width = window_width,
    .height = window_height,
    .min_width = 880,
    .min_height = 560,
    .restore_state = true,
    .restore_policy = .center_on_primary,
    .titlebar = .hidden_inset_tall,
    .views = &shell_views,
}};
const shell_scene: native_sdk.ShellConfig = .{ .windows = &shell_windows };

pub const app_markup = @embedFile("app.native");
const WorkspaceView = canvas.CompiledMarkupView(Model, Msg, app_markup);
const StaxApp = native_sdk.UiAppWithFeatures(Model, Msg, .{
    .runtime_markup = builtin.mode == .Debug,
});

fn onCommand(name: []const u8) ?Msg {
    if (std.mem.eql(u8, name, "stax.refresh")) return .refresh;
    if (std.mem.eql(u8, name, "stax.search")) return .focus_filter;
    if (std.mem.eql(u8, name, "stax.restack")) return .request_restack;
    if (std.mem.eql(u8, name, "stax.submit")) return .request_submit;
    if (std.mem.eql(u8, name, "stax.open-pr")) return .request_open_pr;
    if (std.mem.eql(u8, name, "stax.dismiss")) return .dismiss;
    return null;
}

pub fn onKey(keyboard: canvas.WidgetKeyboardEvent) ?Msg {
    if (keyboard.modifiers.hasNavigationModifier()) return null;
    if (std.ascii.eqlIgnoreCase(keyboard.key, "arrowdown") or std.ascii.eqlIgnoreCase(keyboard.key, "j")) return .select_next;
    if (std.ascii.eqlIgnoreCase(keyboard.key, "arrowup") or std.ascii.eqlIgnoreCase(keyboard.key, "k")) return .select_previous;
    if (std.ascii.eqlIgnoreCase(keyboard.key, "enter") or std.ascii.eqlIgnoreCase(keyboard.key, "return")) return .request_checkout;
    return null;
}

fn initialModel(allocator: std.mem.Allocator) Model {
    return Model.init(allocator);
}

const AppHost = struct {
    app_state: *StaxApp,
    delegated: native_sdk.App,
    picker_open: bool = false,

    fn app(self: *AppHost) native_sdk.App {
        return .{
            .context = self,
            .name = "stax",
            .scene_fn = sceneFn,
            .start_fn = startFn,
            .event_fn = eventFn,
            .stop_fn = stopFn,
            .replay_fn = replayFn,
        };
    }

    fn startFn(context: *anyopaque, runtime: *native_sdk.Runtime) anyerror!void {
        const self: *AppHost = @ptrCast(@alignCast(context));
        try self.delegated.start(runtime);
    }

    fn sceneFn(context: *anyopaque) anyerror!native_sdk.ShellConfig {
        const self: *AppHost = @ptrCast(@alignCast(context));
        return (try self.delegated.scene()).?;
    }

    fn eventFn(context: *anyopaque, runtime: *native_sdk.Runtime, event: native_sdk.Event) anyerror!void {
        const self: *AppHost = @ptrCast(@alignCast(context));
        try self.delegated.event(runtime, event);

        if (event == .lifecycle and event.lifecycle == .activate and self.app_state.installed) {
            try self.dispatch(runtime, .app_activated);
        }
        if (event == .lifecycle and event.lifecycle == .stop) return;
        try self.presentRepositoryPicker(runtime);
    }

    fn stopFn(context: *anyopaque, runtime: *native_sdk.Runtime) anyerror!void {
        const self: *AppHost = @ptrCast(@alignCast(context));
        try self.delegated.stop(runtime);
    }

    fn replayFn(context: *anyopaque, control: native_sdk.runtime.ReplayControl) anyerror!void {
        const self: *AppHost = @ptrCast(@alignCast(context));
        try self.delegated.replayControl(control);
    }

    fn dispatch(self: *AppHost, runtime: *native_sdk.Runtime, msg: Msg) !void {
        const window_id = if (self.app_state.canvas_window_id == 0) 1 else self.app_state.canvas_window_id;
        try self.app_state.dispatch(runtime, window_id, msg);
    }

    fn presentRepositoryPicker(self: *AppHost, runtime: *native_sdk.Runtime) !void {
        if (!self.app_state.model.needs_repository_picker or self.picker_open) return;
        self.picker_open = true;
        defer self.picker_open = false;

        var path_buffer: [native_sdk.platform.max_dialog_paths_bytes]u8 = undefined;
        const result = runtime.showOpenDialog(.{
            .title = "Choose a Git repository",
            .default_path = self.app_state.model.repositoryPath(),
            .allow_directories = true,
            .allow_multiple = false,
        }, &path_buffer) catch {
            try self.dispatch(runtime, .repository_picker_cancelled);
            return;
        };
        if (result.count == 0 or result.paths.len == 0) {
            try self.dispatch(runtime, .repository_picker_cancelled);
            return;
        }
        const newline = std.mem.indexOfScalar(u8, result.paths, '\n') orelse result.paths.len;
        try self.dispatch(runtime, .{ .repository_selected = result.paths[0..newline] });
    }
};

fn resolveEnginePath(init: std.process.Init, allocator: std.mem.Allocator) ![]const u8 {
    if (builtin.mode == .Debug) {
        if (init.environ_map.get("STAX_DESKTOP_ENGINE")) |path| return path;
    }

    var executable_dir_buffer: [std.fs.max_path_bytes]u8 = undefined;
    const executable_dir_len = try std.process.executableDirPath(init.io, &executable_dir_buffer);
    return bundledEnginePath(allocator, executable_dir_buffer[0..executable_dir_len]);
}

fn bundledEnginePath(allocator: std.mem.Allocator, executable_dir: []const u8) ![]const u8 {
    if (!std.mem.eql(u8, std.fs.path.basename(executable_dir), "MacOS")) {
        return error.BundledEngineUnavailable;
    }
    const contents_dir = std.fs.path.dirname(executable_dir) orelse return error.BundledEngineUnavailable;
    if (!std.mem.eql(u8, std.fs.path.basename(contents_dir), "Contents")) {
        return error.BundledEngineUnavailable;
    }
    return std.fs.path.join(allocator, &.{ contents_dir, "Resources", "bin", "st" });
}

fn resolveStorePath(init: std.process.Init, allocator: std.mem.Allocator) ![]const u8 {
    const home = init.environ_map.get("HOME") orelse return error.MissingHome;
    const data_dir = try std.fs.path.join(allocator, &.{ home, "Library", "Application Support", "Stax" });
    try std.Io.Dir.cwd().createDirPath(init.io, data_dir);
    return std.fs.path.join(allocator, &.{ data_dir, "recent-repositories" });
}

pub fn main(init: std.process.Init) !void {
    const allocator = std.heap.page_allocator;
    const app_state = try allocator.create(StaxApp);
    defer allocator.destroy(app_state);
    app_state.initInPlace(allocator, .{
        .name = "stax",
        .scene = shell_scene,
        .canvas_label = canvas_label,
        .theme = .geist,
        .update_fx = model_mod.update,
        .init_fx = model_mod.boot,
        .view = WorkspaceView.build,
        .markup = if (builtin.mode == .Debug) .{
            .source = app_markup,
            .watch_path = "src/app.native",
            .io = init.io,
        } else null,
        .on_command = onCommand,
        .on_key = onKey,
    });
    defer app_state.deinit();
    app_state.model = initialModel(allocator);
    defer app_state.model.deinit();
    app_state.model.setEnginePath(try resolveEnginePath(init, init.arena.allocator()));
    app_state.model.setStorePath(try resolveStorePath(init, init.arena.allocator()));

    var host = AppHost{
        .app_state = app_state,
        .delegated = app_state.app(),
    };
    try runner.runWithOptions(host.app(), .{
        .app_name = "stax",
        .window_title = "Stax",
        .bundle_id = "dev.cesarferreira.stax.desktop",
        .icon_path = "assets/icon.png",
        .default_frame = geometry.RectF.init(0, 0, window_width, window_height),
        .restore_state = true,
        .js_window_api = false,
        .security = .{
            .permissions = &app_permissions,
            .navigation = .{ .allowed_origins = &.{ "zero://inline", "zero://app" } },
        },
    }, init);
}

test {
    _ = @import("tests.zig");
}

test "bundle engine path is derived from the executable directory" {
    const path = try bundledEnginePath(
        std.testing.allocator,
        "/Applications/Stax.app/Contents/MacOS",
    );
    defer std.testing.allocator.free(path);
    try std.testing.expectEqualStrings(
        "/Applications/Stax.app/Contents/Resources/bin/st",
        path,
    );
    try std.testing.expectError(
        error.BundledEngineUnavailable,
        bundledEnginePath(std.testing.allocator, "/tmp/zig-out/bin"),
    );
}
