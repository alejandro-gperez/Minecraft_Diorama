//! Raylib presentation boundary for the CPU-generated framebuffer.
//!
//! The renderer and scene remain independent of Raylib. This module owns only the window,
//! input, timing, texture transfer, and the dirty flag that avoids rerendering idle frames.

use std::{io, path::Path, time::Instant};

use raylib::prelude::{
    Color as RaylibColor, Image as RaylibImage, KeyboardKey, RaylibDraw, RaylibHandle,
    RaylibTexture2D, TextureFilter, Vector2,
};

use crate::{
    camera::OrbitalCamera,
    environment::Environment,
    lighting::Lighting,
    output::write_ppm,
    render::{Framebuffer, Renderer, parallel::default_worker_count},
    scene::Scene,
};

/// Whole-number magnification of the framebuffer in the window (640x360 -> 1280x720). It is an
/// integer so point filtering maps every framebuffer pixel to an exact block of window pixels.
pub const WINDOW_SCALE: i32 = 2;
const TARGET_FPS: u32 = 60;
const MAX_INPUT_DELTA_SECONDS: f32 = 0.1;

const ORBIT_SPEED_RADIANS_PER_SECOND: f32 = 1.4;
const KEYBOARD_ZOOM_UNITS_PER_SECOND: f32 = 5.0;
const MOUSE_WHEEL_ZOOM_UNITS: f32 = 0.75;

/// Window size that shows a `framebuffer_width` x `framebuffer_height` frame at `WINDOW_SCALE`.
pub fn window_size(framebuffer_width: i32, framebuffer_height: i32) -> (i32, i32) {
    (
        framebuffer_width
            .checked_mul(WINDOW_SCALE)
            .expect("window width must fit the Raylib presentation API"),
        framebuffer_height
            .checked_mul(WINDOW_SCALE)
            .expect("window height must fit the Raylib presentation API"),
    )
}

pub fn run(
    mut camera: OrbitalCamera,
    scene: Scene,
    lighting: Lighting,
    environment: Environment,
    mut framebuffer: Framebuffer,
    ppm_path: &Path,
    overlay_label: Option<&str>,
) -> io::Result<()> {
    let framebuffer_width = i32::try_from(framebuffer.width())
        .expect("framebuffer width must fit the Raylib presentation API");
    let framebuffer_height = i32::try_from(framebuffer.height())
        .expect("framebuffer height must fit the Raylib presentation API");

    let (window_width, window_height) = window_size(framebuffer_width, framebuffer_height);
    let (mut raylib, thread) = raylib::init()
        .size(window_width, window_height)
        .title("CPU Raytraced EggWars Diorama")
        .build();
    raylib.set_target_fps(TARGET_FPS);

    let image =
        RaylibImage::gen_image_color(framebuffer_width, framebuffer_height, RaylibColor::BLACK);
    let mut texture = raylib
        .load_texture_from_image(&thread, &image)
        .expect("Raylib must create the framebuffer presentation texture");
    texture.set_texture_filter(&thread, TextureFilter::TEXTURE_FILTER_POINT);

    let mut rgba_pixels = vec![0; framebuffer.pixels().len() * 4];
    let mut dirty = true;
    let mut initial_ppm_pending = true;
    let mut last_render_seconds = 0.0_f64;
    let mut cpu_render_count = 0_u64;

    println!("Controls: arrow keys orbit; mouse wheel or W/S zoom; Esc closes the window.");
    println!(
        "CPU raytracing on up to {} scoped worker threads (available parallelism).",
        default_worker_count()
    );

    while !raylib.window_should_close() {
        let frame_seconds = raylib.get_frame_time().clamp(0.0, MAX_INPUT_DELTA_SECONDS);
        let input = CameraInput::read(&raylib);
        dirty |= apply_camera_input(&mut camera, input, frame_seconds);

        if dirty {
            let started = Instant::now();
            Renderer::render(&camera, &scene, &lighting, &environment, &mut framebuffer)
                .expect("Phase 3 render configuration and scene data must remain valid");
            last_render_seconds = started.elapsed().as_secs_f64();
            cpu_render_count += 1;

            let converted = copy_framebuffer_to_rgba(&framebuffer, &mut rgba_pixels);
            debug_assert!(converted, "presentation buffer size must remain unchanged");
            texture
                .update_texture(&rgba_pixels)
                .expect("RGBA presentation data must match the texture dimensions");

            if initial_ppm_pending {
                write_ppm(ppm_path, &framebuffer)?;
                println!(
                    "Wrote {} ({}x{}); initial CPU render: {:.2} ms",
                    ppm_path.display(),
                    framebuffer.width(),
                    framebuffer.height(),
                    last_render_seconds * 1_000.0,
                );
                initial_ppm_pending = false;
            }

            dirty = false;
        }

        let fps = raylib.get_fps();
        let diagnostics = format!(
            "CPU render: {:.2} ms | Internal: {}x{} | Window FPS: {} | CPU renders: {}",
            last_render_seconds * 1_000.0,
            framebuffer.width(),
            framebuffer.height(),
            fps,
            cpu_render_count,
        );

        let mut drawing = raylib.begin_drawing(&thread);
        drawing.clear_background(RaylibColor::BLACK);
        drawing.draw_texture_ex(
            &texture,
            Vector2::new(0.0, 0.0),
            0.0,
            WINDOW_SCALE as f32,
            RaylibColor::WHITE,
        );
        let overlay_height = if overlay_label.is_some() { 82 } else { 58 };
        drawing.draw_rectangle(
            0,
            0,
            window_width,
            overlay_height,
            RaylibColor::new(0, 0, 0, 180),
        );
        drawing.draw_text(&diagnostics, 12, 8, 18, RaylibColor::RAYWHITE);
        drawing.draw_text(
            "Arrows: orbit | Mouse wheel or W/S: zoom | Esc: close",
            12,
            32,
            18,
            RaylibColor::LIGHTGRAY,
        );
        if let Some(label) = overlay_label {
            drawing.draw_text(label, 12, 56, 18, RaylibColor::GOLD);
        }
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CameraInput {
    yaw_axis: f32,
    pitch_axis: f32,
    zoom_axis: f32,
    wheel: f32,
}

impl CameraInput {
    fn read(raylib: &RaylibHandle) -> Self {
        Self {
            yaw_axis: axis(
                raylib.is_key_down(KeyboardKey::KEY_RIGHT),
                raylib.is_key_down(KeyboardKey::KEY_LEFT),
            ),
            pitch_axis: axis(
                raylib.is_key_down(KeyboardKey::KEY_UP),
                raylib.is_key_down(KeyboardKey::KEY_DOWN),
            ),
            zoom_axis: axis(
                raylib.is_key_down(KeyboardKey::KEY_S),
                raylib.is_key_down(KeyboardKey::KEY_W),
            ),
            wheel: raylib.get_mouse_wheel_move(),
        }
    }
}

fn axis(positive: bool, negative: bool) -> f32 {
    f32::from(positive as u8) - f32::from(negative as u8)
}

fn apply_camera_input(camera: &mut OrbitalCamera, input: CameraInput, frame_seconds: f32) -> bool {
    let original = *camera;

    if frame_seconds.is_finite() && frame_seconds > 0.0 {
        camera.orbit_yaw(input.yaw_axis * ORBIT_SPEED_RADIANS_PER_SECOND * frame_seconds);
        camera.orbit_pitch(input.pitch_axis * ORBIT_SPEED_RADIANS_PER_SECOND * frame_seconds);
        camera.adjust_radius(input.zoom_axis * KEYBOARD_ZOOM_UNITS_PER_SECOND * frame_seconds);
    }

    if input.wheel.is_finite() {
        camera.adjust_radius(-input.wheel * MOUSE_WHEEL_ZOOM_UNITS);
    }

    *camera != original
}

fn copy_framebuffer_to_rgba(framebuffer: &Framebuffer, rgba_pixels: &mut [u8]) -> bool {
    let Some(expected_length) = framebuffer.pixels().len().checked_mul(4) else {
        return false;
    };
    if rgba_pixels.len() != expected_length {
        return false;
    }

    for (color, rgba) in framebuffer
        .pixels()
        .iter()
        .zip(rgba_pixels.chunks_exact_mut(4))
    {
        let [red, green, blue] = color.to_rgb8();
        rgba.copy_from_slice(&[red, green, blue, 255]);
    }

    true
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::{
        CameraInput, WINDOW_SCALE, apply_camera_input, copy_framebuffer_to_rgba, window_size,
    };
    use crate::{
        camera::OrbitalCamera,
        math::Vec3,
        render::{Color, Framebuffer},
    };

    fn camera() -> OrbitalCamera {
        OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap()
    }

    #[test]
    fn converts_framebuffer_to_reusable_rgba_pixels() {
        let mut framebuffer = Framebuffer::try_new(2, 1).unwrap();
        framebuffer.set_pixel(0, 0, Color::new(1.0, 0.5, 0.0));
        framebuffer.set_pixel(1, 0, Color::new(0.0, 0.25, 1.0));
        let mut rgba = vec![0; 8];
        let original_pointer = rgba.as_ptr();

        assert!(copy_framebuffer_to_rgba(&framebuffer, &mut rgba));
        assert_eq!(rgba, [255, 128, 0, 255, 0, 64, 255, 255]);
        assert_eq!(rgba.as_ptr(), original_pointer);
    }

    #[test]
    fn rejects_incorrect_presentation_buffer_length() {
        let framebuffer = Framebuffer::try_new(2, 1).unwrap();
        let mut rgba = [0; 7];

        assert!(!copy_framebuffer_to_rgba(&framebuffer, &mut rgba));
    }

    #[test]
    fn window_shows_the_framebuffer_at_an_exact_integer_scale() {
        assert_eq!(WINDOW_SCALE, 2);
        assert_eq!(window_size(640, 360), (1280, 720));
        let (width, height) = window_size(37, 21);
        assert_eq!((width % 37, height % 21), (0, 0));
        assert_eq!((width / 37, height / 21), (WINDOW_SCALE, WINDOW_SCALE));
    }

    #[test]
    fn rgba_upload_buffer_matches_the_development_frame() {
        let framebuffer = Framebuffer::try_new(640, 360).unwrap();
        assert_eq!(framebuffer.pixels().len(), 230_400);
        let mut rgba = vec![0; 640 * 360 * 4];
        assert_eq!(rgba.len(), 921_600);
        assert!(copy_framebuffer_to_rgba(&framebuffer, &mut rgba));
        // The previous 320x180 buffer is rejected rather than silently half-filled.
        let mut old = vec![0; 320 * 180 * 4];
        assert!(!copy_framebuffer_to_rgba(&framebuffer, &mut old));
    }

    #[test]
    fn camera_changes_only_for_meaningful_input() {
        let mut camera = camera();
        let original = camera;

        assert!(!apply_camera_input(
            &mut camera,
            CameraInput::default(),
            1.0 / 60.0,
        ));
        assert_eq!(camera, original);

        assert!(apply_camera_input(
            &mut camera,
            CameraInput {
                yaw_axis: 1.0,
                pitch_axis: 1.0,
                zoom_axis: -1.0,
                wheel: 0.0,
            },
            1.0 / 60.0,
        ));
        assert_ne!(camera, original);
        assert!(camera.radius() < original.radius());
        assert!(camera.pitch() > original.pitch());
    }
}
