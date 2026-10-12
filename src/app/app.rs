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
    camera::{FreeCamera, OrbitalCamera},
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
/// Free (spectator) camera: world units per second and radians per mouse pixel.
const FREE_MOVE_UNITS_PER_SECOND: f32 = 14.0;
const MOUSE_LOOK_RADIANS_PER_PIXEL: f32 = 0.0025;

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
    camera: OrbitalCamera,
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
    let mut rig = CameraRig::new(camera);
    let mut dirty = true;
    let mut initial_ppm_pending = true;
    let mut last_render_seconds = 0.0_f64;
    let mut cpu_render_count = 0_u64;

    println!(
        "Controls: arrow keys orbit; mouse wheel or W/S zoom; C toggles the free camera \
         (W/A/S/D move, Space up, Shift down, mouse looks); Esc closes the window."
    );
    println!(
        "CPU raytracing on up to {} scoped worker threads (available parallelism).",
        default_worker_count()
    );

    while !raylib.window_should_close() {
        let frame_seconds = raylib.get_frame_time().clamp(0.0, MAX_INPUT_DELTA_SECONDS);
        if raylib.is_key_pressed(KeyboardKey::KEY_C) && rig.toggle() {
            // The mouse is captured only while the free camera is active. The first mouse delta
            // after a toggle is stale, so it is read and dropped.
            if rig.is_free() {
                raylib.disable_cursor();
            } else {
                raylib.enable_cursor();
            }
            let _ = raylib.get_mouse_delta();
            dirty = true;
        }
        let input = CameraInput::read(&raylib);
        let free_input = FreeInput::read(&raylib);
        dirty |= rig.update(input, free_input, frame_seconds);

        if dirty {
            let started = Instant::now();
            match &rig.mode {
                CameraMode::Orbital => Renderer::render(
                    &rig.orbital,
                    &scene,
                    &lighting,
                    &environment,
                    &mut framebuffer,
                ),
                CameraMode::Free(free) => {
                    Renderer::render(free, &scene, &lighting, &environment, &mut framebuffer)
                }
            }
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
            if rig.is_free() {
                "FREE: W/A/S/D move | Space up | Shift down | Mouse look | C: orbital | Esc: close"
            } else {
                "Arrows: orbit | Mouse wheel or W/S: zoom | C: free camera | Esc: close"
            },
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

/// Spectator-camera input for one frame: movement axes and the mouse motion in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct FreeInput {
    right_axis: f32,
    forward_axis: f32,
    up_axis: f32,
    mouse_dx: f32,
    mouse_dy: f32,
}

impl FreeInput {
    fn read(raylib: &RaylibHandle) -> Self {
        let mouse = raylib.get_mouse_delta();
        Self {
            right_axis: axis(
                raylib.is_key_down(KeyboardKey::KEY_D),
                raylib.is_key_down(KeyboardKey::KEY_A),
            ),
            forward_axis: axis(
                raylib.is_key_down(KeyboardKey::KEY_W),
                raylib.is_key_down(KeyboardKey::KEY_S),
            ),
            up_axis: axis(
                raylib.is_key_down(KeyboardKey::KEY_SPACE),
                raylib.is_key_down(KeyboardKey::KEY_LEFT_SHIFT)
                    || raylib.is_key_down(KeyboardKey::KEY_RIGHT_SHIFT),
            ),
            mouse_dx: mouse.x,
            mouse_dy: mouse.y,
        }
    }
}

/// Which camera renders. The orbital camera is always kept, untouched while the free camera is
/// active, so leaving free mode restores exactly the previous orbital view.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CameraMode {
    Orbital,
    Free(FreeCamera),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CameraRig {
    orbital: OrbitalCamera,
    mode: CameraMode,
}

impl CameraRig {
    fn new(orbital: OrbitalCamera) -> Self {
        Self {
            orbital,
            mode: CameraMode::Orbital,
        }
    }

    fn is_free(&self) -> bool {
        matches!(self.mode, CameraMode::Free(_))
    }

    /// Switches camera. Entering free mode starts from the orbital camera's exact position and
    /// view; leaving it restores the orbital camera as it was. Returns whether the mode changed,
    /// in which case the caller renders once.
    fn toggle(&mut self) -> bool {
        match self.mode {
            CameraMode::Orbital => match FreeCamera::from_orbital(&self.orbital) {
                Some(free) => {
                    self.mode = CameraMode::Free(free);
                    true
                }
                None => false,
            },
            CameraMode::Free(_) => {
                self.mode = CameraMode::Orbital;
                true
            }
        }
    }

    /// Applies one frame of input to the active camera only; true if it changed.
    fn update(&mut self, orbital: CameraInput, free: FreeInput, frame_seconds: f32) -> bool {
        match &mut self.mode {
            CameraMode::Orbital => apply_camera_input(&mut self.orbital, orbital, frame_seconds),
            CameraMode::Free(camera) => apply_free_input(camera, free, frame_seconds),
        }
    }
}

fn apply_free_input(camera: &mut FreeCamera, input: FreeInput, frame_seconds: f32) -> bool {
    let original = *camera;

    if frame_seconds.is_finite() && frame_seconds > 0.0 {
        let step = FREE_MOVE_UNITS_PER_SECOND * frame_seconds;
        camera.translate(
            input.right_axis * step,
            input.forward_axis * step,
            input.up_axis * step,
        );
    }
    // Moving the mouse right turns the view right, which lowers yaw; moving it down looks down.
    camera.look(
        -input.mouse_dx * MOUSE_LOOK_RADIANS_PER_PIXEL,
        -input.mouse_dy * MOUSE_LOOK_RADIANS_PER_PIXEL,
    );

    *camera != original
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
        CameraInput, CameraMode, CameraRig, FreeInput, WINDOW_SCALE, apply_camera_input,
        copy_framebuffer_to_rgba, window_size,
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

    fn rig() -> CameraRig {
        CameraRig::new(camera())
    }

    const FRAME: f32 = 1.0 / 60.0;

    #[test]
    fn toggling_to_free_keeps_the_view_and_back_restores_the_orbital_state() {
        let mut rig = rig();
        let orbital = rig.orbital;
        assert!(rig.toggle());
        let CameraMode::Free(free) = rig.mode else {
            panic!("expected free mode");
        };
        assert!((free.position() - orbital.position()).length() < 1.0e-4);
        assert!((free.forward() - orbital.forward()).length() < 1.0e-4);

        // Flying around leaves the stored orbital camera untouched.
        let flying = FreeInput {
            forward_axis: 1.0,
            mouse_dx: 30.0,
            ..FreeInput::default()
        };
        assert!(rig.update(CameraInput::default(), flying, FRAME));
        assert_eq!(rig.orbital, orbital);

        assert!(rig.toggle());
        assert_eq!(rig.mode, CameraMode::Orbital);
        assert_eq!(rig.orbital, orbital);
    }

    #[test]
    fn switching_is_one_dirty_render_and_idle_frames_are_clean() {
        let mut rig = rig();
        let idle = (CameraInput::default(), FreeInput::default());
        assert!(!rig.update(idle.0, idle.1, FRAME));
        assert!(rig.toggle(), "the switch itself asks for one render");
        assert!(!rig.update(idle.0, idle.1, FRAME), "idle free frame");
        assert!(rig.toggle());
        assert!(!rig.update(idle.0, idle.1, FRAME), "idle orbital frame");
    }

    #[test]
    fn each_mode_reads_only_its_own_input() {
        let mut rig = rig();
        let orbital_keys = CameraInput {
            yaw_axis: 1.0,
            ..CameraInput::default()
        };
        let free_keys = FreeInput {
            up_axis: 1.0,
            ..FreeInput::default()
        };
        assert!(!rig.update(CameraInput::default(), free_keys, FRAME));
        assert!(rig.update(orbital_keys, free_keys, FRAME));
        rig.toggle();
        let start = rig.mode;
        assert!(!rig.update(orbital_keys, FreeInput::default(), FRAME));
        assert_eq!(rig.mode, start);
        assert!(rig.update(orbital_keys, free_keys, FRAME));
    }

    #[test]
    fn free_mouse_and_invalid_input_are_handled() {
        let mut rig = rig();
        rig.toggle();
        let look = FreeInput {
            mouse_dx: 10.0,
            mouse_dy: -10.0,
            ..FreeInput::default()
        };
        assert!(rig.update(CameraInput::default(), look, FRAME));
        let before = rig.mode;
        let bad = FreeInput {
            right_axis: f32::NAN,
            mouse_dx: f32::NAN,
            ..FreeInput::default()
        };
        assert!(!rig.update(CameraInput::default(), bad, f32::NAN));
        assert_eq!(rig.mode, before);
    }
}
