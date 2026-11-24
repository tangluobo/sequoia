//! Try one preferred backend without loading unused graphics drivers. Retain the
//! original configuration as a fallback, including its adapter/device settings.
use std::sync::Arc;

use egui_wgpu::{RendererOptions, WgpuConfiguration, WgpuSetup, wgpu, winit::Painter};
use winit::window::Window;

fn preferred_config(
    config: &WgpuConfiguration,
    preferred: Option<wgpu::Backends>,
) -> Option<WgpuConfiguration> {
    let preferred = preferred?;
    let WgpuSetup::CreateNew(setup) = &config.wgpu_setup else {
        return None;
    };
    if setup.native_adapter_selector.is_some()
        || !setup.instance_descriptor.backends.contains(preferred)
        || setup.instance_descriptor.backends == preferred
    {
        return None;
    }
    let mut config = config.clone();
    if let WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup {
        setup.instance_descriptor.backends = preferred;
    }
    Some(config)
}

pub(super) async fn create(
    ctx: &egui::Context,
    config: WgpuConfiguration,
    preferred: Option<wgpu::Backends>,
    transparent: bool,
    options: RendererOptions,
    window: Arc<Window>,
) -> Result<Painter, egui_wgpu::WgpuError> {
    if let Some(preferred_config) = preferred_config(&config, preferred) {
        let mut painter = Painter::new(ctx.clone(), preferred_config, transparent, options).await;
        match painter
            .set_window(egui::ViewportId::ROOT, Some(window.clone()))
            .await
        {
            Ok(()) => {
                // A software-only preferred API must not hide hardware adapters
                // offered by another API in the original configuration.
                let software = painter.render_state().is_some_and(|state| {
                    state.adapter.get_info().device_type == wgpu::DeviceType::Cpu
                });
                if !software {
                    return Ok(painter);
                }
                log::debug!(
                    "Preferred graphics backend is software-only; checking configured backends"
                );
            }
            Err(error) => {
                log::warn!(
                    "Preferred graphics backend failed; retrying configured backends: {error}"
                );
            }
        }
        // Drop the failed surface/instance before creating a replacement.
    }
    let mut painter = Painter::new(ctx.clone(), config, transparent, options).await;
    painter
        .set_window(egui::ViewportId::ROOT, Some(window))
        .await?;
    Ok(painter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_preserves_fallback_and_custom_selection() {
        let mut original = WgpuConfiguration::default();
        let WgpuSetup::CreateNew(setup) = &mut original.wgpu_setup else {
            unreachable!()
        };
        setup.instance_descriptor.backends = wgpu::Backends::VULKAN | wgpu::Backends::DX12;
        let narrowed = preferred_config(&original, Some(wgpu::Backends::VULKAN)).unwrap();
        let WgpuSetup::CreateNew(narrowed) = narrowed.wgpu_setup else {
            unreachable!()
        };
        assert_eq!(
            narrowed.instance_descriptor.backends,
            wgpu::Backends::VULKAN
        );
        let WgpuSetup::CreateNew(setup) = &mut original.wgpu_setup else {
            unreachable!()
        };
        assert_eq!(
            setup.instance_descriptor.backends,
            wgpu::Backends::VULKAN | wgpu::Backends::DX12
        );
        assert!(Arc::ptr_eq(
            &setup.device_descriptor,
            &narrowed.device_descriptor
        ));
        assert!(preferred_config(&original, Some(wgpu::Backends::GL)).is_none());
        let WgpuSetup::CreateNew(setup) = &mut original.wgpu_setup else {
            unreachable!()
        };
        setup.native_adapter_selector = Some(Arc::new(|_, _| Err("custom selector".into())));
        assert!(preferred_config(&original, Some(wgpu::Backends::VULKAN)).is_none());
    }
}
