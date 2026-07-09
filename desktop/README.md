# Desktop

A native-rendered Native SDK app: the view lives in `src/app.native`
(declarative markup) and the logic in `src/main.zig` (`Model`, `Msg`,
`update`). There is no WebView; the pinned `native` CLI owns the application
build and npm provides the reproducible command wrappers.

## Commands

```sh
npm run dev    # build the matching Rust engine, then run with hot reload
npm test       # run the app's test suite
npm run build  # produce a ReleaseFast binary in zig-out/bin/
npm run check  # validate src/*.native markup and app.zon
```

## Hot reload

`src/app.native` is watched while `native dev` runs: edit it and the
window updates within ~2s without losing model state. Parse failures
keep the last good view. The development wrapper builds the Rust engine in
`../target/desktop-engine/`, isolated from regular Cargo test and lint output.

## Owning the build

Need custom build logic? `native eject` writes a build.zig and
build.zig.zon into the app — from then on the `native` verbs drive
your files through `zig build` and never regenerate them.
