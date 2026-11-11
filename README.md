# Plugin_Sky

The level's sky: the atmosphere component (air, sun disc, clouds and weather), its row and the renderer feature that draws it and publishes the sky's lighting inputs.

`pulsar.sky` ("Sky") is a plugin of
[Pulsar-Native](https://github.com/Far-Beyond-Pulsar/Pulsar-Native), the engine and editor that renders with
[Helio](https://github.com/Far-Beyond-Pulsar/Helio). Pulsar-Native vendors this repository as a git submodule
at `plugins/vendor/sky`.

## What it owns

Every object kind is a plugin. A plugin owns its SceneDB components (their
data and scriptable methods; the editor calls the same methods scripts do),
the contracts other kinds read (its `api` crate: graph keys, row ABIs, WGSL
snippets), the Helio feature that renders it (passes, scene derivations,
settings), its file formats and its editor UI. Helio knows no object kind,
and other kinds meet this one only through frame contracts, extension points,
draw phases and api crates.

| Crate | Path | What |
|---|---|---|
| `sky_plugin` | `./` (`src/`) | The plugin's root crate: the one place it registers everything it contributes (`src/runtime.rs`; `src/editor.rs` with the `editor` feature). Its integration tests are in `tests/`. |
| `sky_component` | `component/` | Sky (pulsar.sky): the atmosphere component (the level's sky and its clouds), its row, its row join and the plugin's Helio feature set (atmosphere tables, clouds and the sky composite) |
| `sky_editor` | `editor/` | Sky (pulsar.sky): the editor crate -- the level editor's sky entry and World Settings section |
| `sky-examples` | `examples/` | The sky plugin's standalone hosts on Helio: Earth's atmosphere over a lit scene |
| `helio-pass-sky` | `feature/` | Helio atmosphere (sky) passes |

- **Root crate** `sky_plugin`: the only crate that registers anything (the
  component class, its scene rows, the Helio feature, the one-sky level
  rule; with `editor`, the Create menu's sky and the World Settings
  section). A game links it without `editor`; the editor links it with.
  Other plugins never depend on it.
- **Requires**: nothing. A project enables or disables the plugin in its
  `Pulsar.toml` (`[plugins]`); disabled, the kind is absent and the frame
  draws without it.
- **Reads other kinds through**: `helio-light-api` (lights).
- **Its standalone hosts also compose**: `helio-pass-gi` (gi); `helio-light-api, helio-light-host` (lights); `helio-mats` (materials); `helio-mesh-api, helio-mesh-host, helio-pass-forward-lit` (meshes) (demos and GPU tests
  draw a whole scene; the plugin itself does not depend on these).

## Render graph nodes

Every pass the plugin's feature adds to the default pipeline is a node of `.rendergraph` assets, registered from its feature (`registrar.node`, so only while the plugin is enabled) with generated pins, an `Enabled` parameter and the default placement below. A graph naming one while `pulsar.sky` is disabled fails validation with the plugin named. Test: `tests/graph_node.rs`; see Pulsar-Native's `.agents/RENDER_GRAPH.md` ("Plugin nodes").

| Node kind | Title | Default placement |
|---|---|---|
| `helio.pass.atmosphere` | Sky: Atmosphere Tables | Prepare slot |
| `helio.pass.cloud_trace` | Sky: Cloud Trace | Lighting-inputs slot, order 10 |
| `helio.pass.atmosphere_composite` | Sky: Composite | Lighting slot |

## Building and testing

Like [Plugin_Physics](https://github.com/Far-Beyond-Pulsar/Plugin_Physics), this plugin's crates are members
of Pulsar-Native's workspace: their dependencies are `workspace = true`, and
Helio comes from Pulsar-Native's `crates/renderer/helio` submodule. It builds
only inside a Pulsar-Native checkout:

```bash
git clone --recursive https://github.com/Far-Beyond-Pulsar/Pulsar-Native
cd Pulsar-Native
cargo check -p sky_component -p sky_editor -p sky_plugin --features sky_plugin/editor -p sky-examples -p helio-pass-sky --tests
cargo test -p sky_component -p sky_plugin -p helio-pass-sky
```

Suites that render real frames need a GPU; CI leaves them out by name
(Pulsar-Native's `.config/nextest.toml`, `ci` profile). This repository's CI
checks the plugin inside a Pulsar-Native checkout: rustfmt, clippy and the
`ci` profile's tests.

A change that spans this plugin and Pulsar-Native (or Helio) merges
dependency-first: merge here, re-pin `plugins/vendor/sky` in Pulsar-Native to
this repository's `main`, then merge the Pulsar-Native PR (Pulsar-Native's
`.agents/CRATES.md`, "Submodule pins").

## Architecture

- [Every object kind is a plugin](https://github.com/Far-Beyond-Pulsar/Helio/blob/main/docs/architecture/plugin-migration.md): the
  rule, what Helio is, the plugin table and the "As built" notes of each
  phase.
- [The layered rewrite](https://github.com/Far-Beyond-Pulsar/Helio/blob/main/docs/architecture/layered-rewrite.md): layers, buffer
  contracts, extension points and why the renderer has this shape.
- [The `helio-architecture` skill](https://github.com/Far-Beyond-Pulsar/Helio/blob/main/.agents/skills/helio-architecture/SKILL.md): required reading for any change
  here (see `AGENTS.md`).
- Pulsar-Native's `.agents/PLUGIN_SYSTEM.md` and `.agents/CRATES.md`: plugin
  descriptors, the installed set and the submodule pins.

History before 2026-11-02 (when the plugin moved into Pulsar-Native) is in
Helio's repository; this repository's history starts with that move
(`git subtree split` of `plugins/vendor/sky`).

## License

MIT, as the rest of Pulsar (`LICENSE.md`).
