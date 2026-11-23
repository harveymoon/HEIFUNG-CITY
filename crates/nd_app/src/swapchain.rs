//! [r15-polish] Surviving an outdated swapchain.
//!
//! The paused game sat idle for an hour and died in wgpu-hal 29.0.4
//! (`vulkan/swapchain/native.rs:359`, "Trying to destroy a
//! SwapchainAcquireSemaphore that is still in use by a SurfaceTexture").
//! When the surface reports `Outdated` (the display slept or changed
//! mode), Bevy 0.19's `prepare_windows` reconfigures it at once, but the
//! cameras' `ViewTarget`s from the frame before still hold a view of the
//! old swapchain image, which keeps its semaphores alive; Vulkan's
//! release of the old swapchain then panics. Bevy drops those views first
//! only when the window is resized (`cleanup_view_targets_for_resize`).
//!
//! Here the window cameras' `ViewTarget`s are dropped before each frame's
//! surfaces are made or acquired, whatever the reason; `prepare_view_targets`
//! builds them afresh every frame anyway, so nothing else changes.

use bevy::camera::NormalizedRenderTarget;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::view::window::{create_surfaces, prepare_windows};
use bevy::render::view::ViewTarget;
use bevy::render::{Render, RenderApp, RenderSystems};

pub struct SwapchainGuardPlugin;

impl Plugin for SwapchainGuardPlugin {
    fn build(&self, app: &mut App) {
        if let Some(r) = app.get_sub_app_mut(RenderApp) {
            r.add_systems(Render, drop_window_view_targets.in_set(RenderSystems::PrepareViews).before(create_surfaces).before(prepare_windows));
        }
    }
}

/// Remove last frame's `ViewTarget` from every camera drawing to a window.
fn drop_window_view_targets(mut commands: Commands, cameras: Query<(Entity, &ExtractedCamera), With<ViewTarget>>) {
    for (entity, camera) in &cameras {
        if matches!(camera.target, Some(NormalizedRenderTarget::Window(_))) {
            commands.entity(entity).remove::<ViewTarget>();
        }
    }
}
