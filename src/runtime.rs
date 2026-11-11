//! What the sky plugin registers for a game (and the editor): every runtime
//! contribution, grouped by the engine seam it plugs into.

use engine_backend::scene::level_rules::LevelSingletonRegistration;
use engine_backend::scene::SceneRowsRegistration;
use engine_backend::scene::{ClassSpec, ComponentsBuilder};
use engine_backend::subsystems::render::RenderFeatureRegistration;
use sky_component::{AtmosphereComponent, ATMOSPHERE_CLASS, ONE_ATMOSPHERE};

/// Everything the sky plugin registers for a game.
pub(crate) fn build(p: &mut pulsar_plugins::PluginBuilder) {
    // ── Component classes ───────────────────────────────────────────────────

    p.components()
        .class::<AtmosphereComponent>(ClassSpec::default());

    // ── Rendering ───────────────────────────────────────────────────────────

    // The atmosphere's source rows and the row join that places a planet's
    // atmosphere at its owner.
    p.add(SceneRowsRegistration {
        origin: p.origin(),
        name: "sky.rows",
        register_columns: sky_component::register_columns,
        derivation: Some(sky_component::derivation),
    });

    // The atmosphere's tables, the clouds and the sky composite; they read no
    // project settings through the renderer config.
    p.add(RenderFeatureRegistration {
        origin: p.origin(),
        features: sky_component::render_features,
        configure: |config| config,
    });

    // ── Scene ───────────────────────────────────────────────────────────────

    // A level has one sky (#1057): the atmosphere pass draws the first
    // enabled row, so a second would only fight the first.
    p.add(LevelSingletonRegistration {
        origin: p.origin(),
        class_name: ATMOSPHERE_CLASS,
        refusal: ONE_ATMOSPHERE,
        instances: sky_component::atmospheres,
        is: |value| value.is::<AtmosphereComponent>(),
        held: |world, instance| world.get::<AtmosphereComponent>(instance).is_some(),
        default_is: true,
        enabled_only: false,
        edits_checked: false,
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn registers_its_feature_and_its_rule() {
        let features =
            engine_backend::subsystems::render::helio_renderer::render_features::plugin_features();
        assert!(
            features.ids().contains(&"helio.sky"),
            "features: {:?}",
            features.ids()
        );
        assert!(pulsar_plugins::registered::<
            engine_backend::scene::level_rules::LevelSingletonRegistration,
        >()
        .any(|rule| rule.class_name == sky_component::ATMOSPHERE_CLASS));
    }

    /// Every registration of the plugin names this root, so it resolves to
    /// the plugin's id.
    #[test]
    fn every_registration_resolves_to_the_plugin() {
        let ours: Vec<_> = pulsar_plugins::contributions()
            .into_iter()
            .filter(|summary| summary.origin == pulsar_plugins::origin!())
            .collect();
        assert!(ours
            .iter()
            .any(|summary| summary.category == "component.class"));
        for summary in &ours {
            assert_eq!(
                pulsar_plugins::plugin_of(summary.origin).map(|plugin| plugin.id.as_str()),
                Some("pulsar.sky"),
                "{summary:?}"
            );
        }
    }
}
