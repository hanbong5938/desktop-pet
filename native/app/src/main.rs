use desktop_pet_core::Composition;
#[cfg(feature = "herdr")]
use std::sync::Arc;

fn composition() -> Composition {
    let composition = Composition::default();
    #[cfg(any(feature = "herdr", feature = "tern"))]
    let mut composition = composition;
    #[cfg(feature = "herdr")]
    {
        let integration = desktop_pet_herdr::HerdrIntegration::new();
        let extensions = integration.extensions();
        composition.integrations.push(integration.clone());
        composition
            .provider_labels
            .insert("herdr".into(), "Herdr".into());
        composition.extensions.control.extend(extensions.control);
        composition.extensions.cli.extend(extensions.cli);
        composition
            .extensions
            .lifecycle
            .extend(extensions.lifecycle);
        let configured = integration.clone();
        composition
            .profile_configurators
            .push(Arc::new(move |profile| {
                configured.configure_profile(profile)
            }));
        let input_policy = integration.clone();
        composition.cli_input_policies.insert(
            "herdr-observation".into(),
            Arc::new(move |arguments| {
                input_policy.command_reads_input("herdr-observation", arguments)
            }),
        );
        #[cfg(desktop_pet_legacy)]
        {
            let status = integration.clone();
            composition
                .status_fields
                .push(Arc::new(move || status.legacy_status_fields()));
        }
        composition.attach_frames.push(Arc::new(move |selection| {
            integration.registration_frame(selection)
        }));
    }
    #[cfg(feature = "tern")]
    {
        let (integration, extensions) = desktop_pet_tern::registration();
        composition.integrations.push(integration);
        composition.provider_labels.insert(
            desktop_pet_tern::PROVIDER_ID.into(),
            desktop_pet_tern::PROVIDER_LABEL.into(),
        );
        composition.extensions.control.extend(extensions.control);
        composition.extensions.cli.extend(extensions.cli);
        composition
            .extensions
            .lifecycle
            .extend(extensions.lifecycle);
    }
    composition
}

fn main() {
    if let Err(error) = desktop_pet_core::run(composition()) {
        eprintln!(
            "{}: {error}",
            desktop_pet_update_coordinator::identity::BINARY
        );
        std::process::exit(1);
    }
}
