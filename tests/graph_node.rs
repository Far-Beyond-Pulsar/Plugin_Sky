//! The sky passes are nodes of render graphs: registered by the
//! plugin's root (through its render feature), placed where the default
//! pipeline ran them, with the pins their passes declare, built from a graph
//! asset, and absent (with the reason) while the plugin is disabled.

// The plugin as a game links it: its root, with every registration.
use sky_plugin as _;

use std::sync::{Arc, Mutex};

use engine_backend::subsystems::render::helio_renderer::{pipelines, render_features};
use helio::{DebugDrawState, PassBuildContext, RendererConfig};
use helio_graph_asset::{validate, NodeInstance, NodeRegistry, PinKind};
use helio_presets::Preset;
use pulsar_scenedb::gpu::{EngineGpuContext, GpuMirrorHandle, SceneGpuConfig, SceneGpuStore};

const PLUGIN: &str = "pulsar.sky";

/// Every pass node of the plugin: kind, owning feature, a fragment of the
/// type name of the pass it builds, and whether the pass declares resources.
const NODES: &[(&str, &str, &str, bool)] = &[
    ("helio.pass.atmosphere", "helio.sky", "AtmospherePass", true),
    ("helio.pass.cloud_trace", "helio.sky", "CloudTracePass", true),
    ("helio.pass.atmosphere_composite", "helio.sky", "AtmosphereCompositePass", true),
];

/// A pipeline the plugin's nodes are placed in.
struct Case {
    name: &'static str,
    preset: fn(&NodeRegistry) -> Preset,
    /// The plugin's node kinds in the pipeline's order.
    kinds: &'static [&'static str],
    /// The plugin's passes the compiled pipeline holds, in order (a node that
    /// builds nothing under the default config is not listed).
    passes: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "deferred",
        preset: |_| Preset::Deferred,
        kinds: &["helio.pass.atmosphere", "helio.pass.cloud_trace", "helio.pass.atmosphere_composite"],
        passes: &["AtmospherePass", "CloudTracePass", "AtmosphereCompositePass"],
    },
];


/// The plugin gate is process-wide state and a wgpu device is one at a time
/// (Pulsar-Native#1647): every test takes this first.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn device() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(helio_core::wgpu_instance_descriptor());
        let adapter = instance.request_adapter(&Default::default()).await.ok()?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: helio::required_wgpu_features(adapter.features()),
                required_limits: helio::required_wgpu_limits(adapter.limits()),
                experimental_features: helio::required_experimental_features(adapter.features()),
                ..Default::default()
            })
            .await
            .ok()?;
        Some((Arc::new(device), Arc::new(queue)))
    })
}

fn buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::UNIFORM
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn configured() -> RendererConfig {
    // The renderer config with the plugins' settings, as a host makes it.
    render_features::configure(RendererConfig::new(
        320,
        180,
        wgpu::TextureFormat::Rgba8Unorm,
    ))
}

fn registry() -> NodeRegistry {
    helio_presets::graphs::registry_with(&render_features::plugin_features())
}

fn is_ours(kind: &str) -> bool {
    NODES.iter().any(|(ours, ..)| *ours == kind)
}

/// The case's pipeline as a graph asset over `registry`.
fn asset_of(case: &Case, registry: &NodeRegistry) -> helio_graph_asset::GraphAsset {
    helio_presets::graphs::graph_asset((case.preset)(registry), registry, &configured())
}

/// Every node instance of the asset, in its sections too.
fn instances_mut(
    asset: &mut helio_graph_asset::GraphAsset,
) -> impl Iterator<Item = &mut NodeInstance> {
    asset
        .nodes
        .iter_mut()
        .chain(asset.sections.iter_mut().flat_map(|s| s.nodes.iter_mut()))
}

/// Compiles `asset` and lists the plugin's passes in order (`None`: no GPU).
fn plugin_passes(asset: &helio_graph_asset::GraphAsset) -> Option<Vec<&'static str>> {
    let (device, queue) = device()?;
    let gpu = EngineGpuContext::new(Arc::clone(&device), Arc::clone(&queue));
    let store = SceneGpuStore::new(
        &gpu,
        SceneGpuConfig {
            classes: Vec::new(),
            tombstone_headroom: 0,
            max_cells_metadata: 0,
        },
    );
    let camera = buffer(&device, 512);
    let debug_camera = buffer(&device, 64);
    let cull_stats = buffer(&device, 64);
    let mut env = helio_graph_asset::build_env(&PassBuildContext {
        device: &device,
        queue: &queue,
        config: configured(),
        debug_state: Arc::new(Mutex::new(DebugDrawState::default())),
        camera_buffer: &camera,
        debug_camera_buffer: &debug_camera,
        cull_stats_buffer: &cull_stats,
        owns_device: false,
        scene_db: GpuMirrorHandle::new(Arc::new(store), Arc::clone(&queue)),
    });
    let graph = helio_graph_asset::compile(asset, &registry(), &mut env)
        .unwrap_or_else(|errors| panic!("the graph builds: {errors:?}"));
    Some(
        (0..graph.pass_count())
            .filter_map(|index| graph.pass_declaration(index))
            .filter_map(|pass| {
                NODES
                    .iter()
                    .map(|(_, _, fragment, _)| *fragment)
                    .find(|fragment| pass.type_name.contains(fragment))
            })
            .collect(),
    )
}

#[test]
fn the_plugin_registers_its_nodes_where_the_default_pipeline_ran_them() {
    let _serial = serial();
    pulsar_plugins::deactivate();
    let registry = registry();
    for (kind, feature, _, _) in NODES {
        let node = registry
            .get(kind)
            .unwrap_or_else(|| panic!("`{kind}` is registered"));
        assert_eq!(registry.feature_of(kind), Some(*feature), "{kind}");
        assert!(node.generated, "{kind}: pins come from what the pass declares");
        assert!(node.params.iter().any(|p| p.key == "enabled"), "{kind}");
        assert!(node.placement.is_some(), "{kind}: placed in the frame");
    }
    for case in CASES {
        let preset = (case.preset)(&registry);
        let kinds: Vec<String> = preset
            .nodes(&registry, &configured())
            .into_iter()
            .map(|node| node.kind)
            .filter(|kind| is_ours(kind))
            .collect();
        assert_eq!(kinds, case.kinds, "{}: the plugin's nodes in order", case.name);
    }
}

#[test]
fn the_graph_builds_the_passes_and_enabled_bypasses_them() {
    let _serial = serial();
    pulsar_plugins::deactivate();
    let registry = registry();
    for case in CASES {
        let mut asset = asset_of(case, &registry);
        let Some(passes) = plugin_passes(&asset) else {
            eprintln!("GPU_VALIDATION_SKIPPED_NO_ADAPTER: {PLUGIN} graph nodes");
            return;
        };
        assert_eq!(passes, case.passes, "{}: the plugin's passes in order", case.name);

        // The Enabled parameter bypasses the node: no pass.
        for node in instances_mut(&mut asset).filter(|n| is_ours(&n.kind)) {
            node.params.insert("enabled".into(), false.into());
        }
        assert_eq!(
            plugin_passes(&asset),
            Some(Vec::new()),
            "{}: every node bypassed",
            case.name
        );
    }
}

#[test]
fn the_probe_gives_each_node_the_pins_its_pass_declares() {
    let _serial = serial();
    pulsar_plugins::deactivate();
    let registry = pipelines::graph_node_registry();
    if pipelines::graph_pin_probe().is_none() {
        eprintln!("GPU_VALIDATION_SKIPPED_NO_ADAPTER: {PLUGIN} node pins");
        return;
    }
    for (kind, _, _, declares) in NODES {
        let node = registry
            .get(kind)
            .unwrap_or_else(|| panic!("`{kind}` is registered"));
        let pins = node.pins(&Default::default());
        assert!(pins.inputs.iter().any(|p| p.ty.kind == PinKind::Flow), "{kind}");
        assert!(pins.outputs.iter().any(|p| p.ty.kind == PinKind::Flow), "{kind}");
        let resources = pins
            .inputs
            .iter()
            .chain(&pins.outputs)
            .filter(|p| p.ty.kind != PinKind::Flow)
            .count();
        if *declares {
            assert!(resources > 0, "{kind} declares resources: {pins:?}");
        }
        eprintln!("NODE_PINS {kind} {pins:?}");
    }
}

#[test]
fn a_disabled_plugin_has_no_nodes_and_a_graph_naming_them_says_so() {
    let _serial = serial();
    pulsar_plugins::deactivate();
    let with_plugin = registry();
    let mut asset = asset_of(&CASES[0], &with_plugin);
    asset.nodes.retain(|n| !is_ours(&n.kind));
    for (kind, ..) in NODES {
        asset.nodes.push(NodeInstance {
            id: format!("ours:{kind}"),
            kind: (*kind).into(),
            params: Default::default(),
            position: [0.0; 2],
        });
    }

    pulsar_plugins::activate(&pulsar_plugins::resolve(
        pulsar_plugins::installed(),
        &pulsar_plugins::ProjectPlugins::default().with(&[PLUGIN], false),
    ));
    let registry = pipelines::graph_node_registry();
    pulsar_plugins::deactivate();

    let validation = validate(&asset, &registry);
    let messages: Vec<_> = validation.errors().map(|d| d.message.clone()).collect();
    for (kind, ..) in NODES {
        assert!(registry.get(kind).is_none(), "`{kind}` is unregistered");
        assert_eq!(
            registry.absence(kind),
            Some(format!("plugin {PLUGIN} disabled").as_str()),
            "{kind}"
        );
        let expected = format!("missing node `{kind}` (plugin {PLUGIN} disabled)");
        assert!(messages.iter().any(|m| *m == expected), "{messages:?}");
    }
    // Enabled again, the registry follows.
    for (kind, ..) in NODES {
        assert!(pipelines::graph_node_registry().get(kind).is_some());
    }
}
