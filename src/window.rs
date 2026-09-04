// Simple display/window device

extern crate sdl2;
use sdl2::pixels::Color;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use sdl2::render::Texture;
use sdl2::render::TextureAccess;
use sdl2::pixels::PixelFormatEnum;
use std::sync::Mutex;
use crate::vm::Actor;
use crate::value::*;
use crate::bytearray::ByteArray;
use crate::str::Str;
use crate::host::HostResult;
use crate::ast::UIEVENT_ID;
use crate::*;

// Global SDL state
struct SdlState {
    sdl: Option<sdl2::Sdl>,
    video: Option<sdl2::VideoSubsystem>,
    event_pump: Option<sdl2::EventPump>,
}
unsafe impl Send for SdlState {}
static SDL_STATE: Mutex<SdlState> = Mutex::new(SdlState {
    sdl: None,
    video: None,
    event_pump: None,
});

fn init_sdl()
{
    let mut sdl_state = SDL_STATE.lock().unwrap();

    if sdl_state.sdl.is_none() {
        sdl_state.sdl = Some(sdl2::init().unwrap());
    }
}

pub fn with_sdl_context<F, R>(f: F) -> R
where
    F: FnOnce(&sdl2::Sdl) -> R,
{
    init_sdl();
    let sdl_state = SDL_STATE.lock().unwrap();
    f(sdl_state.sdl.as_ref().unwrap())
}

fn init_sdl_video()
{
    init_sdl();

    let mut sdl_state = SDL_STATE.lock().unwrap();

    if sdl_state.video.is_none() {
        let sdl = sdl_state.sdl.as_ref().unwrap();
        sdl_state.video = Some(sdl.video().unwrap());
    }
}

struct Window<'a>
{
    width: u32,
    height: u32,

    // SDL canvas to draw into
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
    texture_creator: sdl2::render::TextureCreator<sdl2::video::WindowContext>,
    texture: Option<Texture<'a>>,
}

// Note: we're leaving this global to avoid the Window lifetime
// bubbling up everywhere.
// TODO: eventually we will likely want to allow multiple windows
unsafe impl Send for Window<'_> {}
static WINDOW: Mutex<Option<Window>> = Mutex::new(None);

pub fn window_create(
    actor: &mut Actor,
    width: Value,
    height: Value,
    title: Value,
    _flags: Value
) -> HostResult
{
    if actor.actor_id != 0 {
        error!("window functions should only be called from the main actor");
    }

    let window = WINDOW.lock().unwrap();
    if window.is_some() {
        error!("for now, only one window supported");
    }
    drop(window);

    let width: u32 = unwrap_u32!(width);
    let height: u32 = unwrap_u32!(height);
    let title_str = unwrap_str!(title);

    init_sdl_video();
    let sdl_state = SDL_STATE.lock().unwrap();
    let video_subsystem = sdl_state.video.as_ref().unwrap();

    let sdl_window = video_subsystem.window(&title_str, width, height)
        .hidden()
        .position_centered()
        .build()
        .unwrap();

    let mut canvas = sdl_window.into_canvas().build().unwrap();

    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    canvas.present();

    let texture_creator = canvas.texture_creator();

    let window = Window {
        width,
        height,
        canvas,
        texture_creator,
        texture: None,
    };

    let mut global_window = WINDOW.lock().unwrap();
    *global_window = Some(window);

    // TODO: return unique window id
    Ok(Value::from(0))
}

// Needed because of the SDL2 crate's insane lifetime
// constraints on textures
unsafe fn make_static<T>(t: &T) -> &'static T {
    core::mem::transmute(t)
}

pub fn window_draw_frame(
    actor: &mut Actor,
    window_id: Value,
    frame: Value,
) -> HostResult
{
    if actor.actor_id != 0 {
        error!("window functions should only be called from the main actor");
    }

    let window_id = unwrap_u32!(window_id);
    let frame = unwrap_ba!(frame);

    if window_id != 0 {
        error!("invalid window id {}", window_id);
    }

    let mut window_lock = WINDOW.lock().unwrap();
    let window = match window_lock.as_mut() {
        Some(window) => window,
        None => error!("the window must be created before drawing a frame")
    };

    // Get the address to copy pixel data from
    let data_len = (4 * window.width * window.height) as usize;
    let pixel_slice = unsafe { ByteArray::get_slice(frame, 0, data_len) };

    // If no frame has been drawn yet
    if window.texture.is_none() {
        // Desperate times require desperate measures
        let texture_creator = unsafe { make_static(&window.texture_creator) };

        // Create the texture to render into
        // Pixels use the BGRA byte order (0xAA_RR_GG_BB on a little-endian machine)
        window.texture = Some(texture_creator.create_texture(
            PixelFormatEnum::BGRA32,
            TextureAccess::Streaming,
            window.width,
            window.height
        ).unwrap());

        // We show and raise the window at the moment the first frame is drawn
        // This avoids showing a blank window too early
        window.canvas.window_mut().show();
        window.canvas.window_mut().raise();
    }

    // Update the texture
    let pitch = 4 * window.width as usize;
    window.texture.as_mut().unwrap().update(None, pixel_slice, pitch).unwrap();

    // Copy the texture into the canvas
    window.canvas.copy(
        &window.texture.as_ref().unwrap(),
        None,
        None
    ).unwrap();

    // Update the screen with any rendering performed since the previous call
    window.canvas.present();

    Ok(Value::NIL)
}

/// Lock the mouse to a window for FPS-style mouse look. While locked, the
/// cursor is hidden and confined to the window, and MOUSE_MOVE events keep
/// reporting motion once the pointer reaches the window edge.
pub fn window_lock_mouse(
    actor: &mut Actor,
    window_id: Value,
    enabled: Value,
) -> HostResult
{
    if actor.actor_id != 0 {
        error!("window functions should only be called from the main actor");
    }

    let window_id = unwrap_u32!(window_id);
    let enabled = unwrap_bool!(enabled);

    if window_id != 0 {
        error!("invalid window id {}", window_id);
    }

    let window = WINDOW.lock().unwrap();
    if window.is_none() {
        error!("the window must be created before locking the mouse");
    }
    drop(window);

    // SDL2 has a single global relative mouse mode, whereas SDL3 makes it a
    // property of the window. We take a window id so that Plush code doesn't
    // have to change when we move to SDL3.
    with_sdl_context(|sdl| {
        let mouse = sdl.mouse();
        mouse.set_relative_mouse_mode(enabled);

        // SDL reports failure through the setter's return value, which the
        // sdl2 crate discards, so we read the mode back instead
        Ok(Value::from(mouse.relative_mouse_mode() == enabled))
    })
}

/// Poll for UI events
pub fn poll_ui_msg(actor: &mut Actor) -> Option<Value>
{
    // This should only ever be called on the main thread
    assert!(actor.actor_id == 0);

    let mut sdl_state = SDL_STATE.lock().unwrap();

    // If no window has been created, stop
    if sdl_state.sdl.as_ref().is_none() {
        return None;
    }

    // Create the event pump if needed
    if sdl_state.event_pump.is_none() {
        let sdl = sdl_state.sdl.as_ref().unwrap();
        sdl_state.event_pump = Some(sdl.event_pump().unwrap());
    }

    let event_pump = sdl_state.event_pump.as_mut().unwrap();

    let event = match event_pump.poll_event() {
        Some(event) => event,
        None => { return None; }
    };

    match event.clone() {
        Event::Quit { .. } => {
            let msg = actor.alloc_obj(UIEVENT_ID);
            actor.set_field(msg, "window_id", Value::from(0));
            let event_type = actor.intern_str("CLOSE_WINDOW");
            actor.set_field(msg, "kind", event_type);
            Some(msg)
        }

        Event::KeyDown { window_id: _, keycode: Some(keycode), .. } |
        Event::KeyUp { window_id: _, keycode: Some(keycode), .. } => {
            let key_name = translate_keycode(keycode);
            if key_name.is_none() {
                return None;
            }

            let msg = actor.alloc_obj(UIEVENT_ID);
            actor.set_field(msg, "window_id", Value::from(0));

            let event_type = if let Event::KeyDown { .. } = event {
                actor.intern_str("KEY_DOWN")
            } else {
                actor.intern_str("KEY_UP")
            };
            actor.set_field(msg, "kind", event_type);

            let key_name = actor.intern_str(key_name.unwrap());
            actor.set_field(msg, "key", key_name);

            Some(msg)
        }

        Event::MouseButtonDown { window_id: _, which: _, mouse_btn, x, y, .. } |
        Event::MouseButtonUp { window_id: _, which: _, mouse_btn, x, y, .. } => {
            let button_name = translate_mouse_button(mouse_btn);
            if button_name.is_none() {
                return None;
            }

            let msg = actor.alloc_obj(UIEVENT_ID);
            actor.set_field(msg, "window_id", Value::from(0));

            let event_type = if let Event::MouseButtonDown { .. } = event {
                actor.intern_str("MOUSE_DOWN")
            } else {
                actor.intern_str("MOUSE_UP")
            };
            actor.set_field(msg, "kind", event_type);

            let button_name = actor.intern_str(button_name.unwrap());
            actor.set_field(msg, "button", button_name);

            actor.set_field(msg, "x", Value::from(x));
            actor.set_field(msg, "y", Value::from(y));

            Some(msg)
        }

        Event::MouseMotion { window_id: _, x, y, xrel, yrel, .. } => {
            let msg = actor.alloc_obj(UIEVENT_ID);
            actor.set_field(msg, "window_id", Value::from(0));
            let event_type = actor.intern_str("MOUSE_MOVE");
            actor.set_field(msg, "kind", event_type);
            actor.set_field(msg, "x", Value::from(x));
            actor.set_field(msg, "y", Value::from(y));

            // Reported as floats because SDL3 gives sub-pixel deltas. They
            // are the only meaningful motion while the mouse is locked, as
            // x and y then stop at the window edge.
            let dx = actor.float64(xrel as f64);
            let dy = actor.float64(yrel as f64);
            actor.set_field(msg, "dx", dx);
            actor.set_field(msg, "dy", dy);

            Some(msg)
        }

        Event::TextInput { window_id: _, text, .. } => {
            let msg = actor.alloc_obj(UIEVENT_ID);
            actor.set_field(msg, "window_id", Value::from(0));
            let kind = actor.intern_str("TEXT_INPUT");
            actor.set_field(msg, "kind", kind);
            let text = Str::new(&text, &mut actor.alloc);
            actor.set_field(msg, "text", text);

            Some(msg)
        }

        _ => None
    }
}

fn translate_keycode(sdl_keycode: Keycode) -> Option<&'static str>
{
    // https://docs.rs/sdl2/0.30.0/sdl2/keyboard/enum.Keycode.html
    match sdl_keycode {
        Keycode::A => Some("A"),
        Keycode::B => Some("B"),
        Keycode::C => Some("C"),
        Keycode::D => Some("D"),
        Keycode::E => Some("E"),
        Keycode::F => Some("F"),
        Keycode::G => Some("G"),
        Keycode::H => Some("H"),
        Keycode::I => Some("I"),
        Keycode::J => Some("J"),
        Keycode::K => Some("K"),
        Keycode::L => Some("L"),
        Keycode::M => Some("M"),
        Keycode::N => Some("N"),
        Keycode::O => Some("O"),
        Keycode::P => Some("P"),
        Keycode::Q => Some("Q"),
        Keycode::R => Some("R"),
        Keycode::S => Some("S"),
        Keycode::T => Some("T"),
        Keycode::U => Some("U"),
        Keycode::V => Some("V"),
        Keycode::W => Some("W"),
        Keycode::X => Some("X"),
        Keycode::Y => Some("Y"),
        Keycode::Z => Some("Z"),
        Keycode::Num0 => Some("0"),
        Keycode::Num1 => Some("1"),
        Keycode::Num2 => Some("2"),
        Keycode::Num3 => Some("3"),
        Keycode::Num4 => Some("4"),
        Keycode::Num5 => Some("5"),
        Keycode::Num6 => Some("6"),
        Keycode::Num7 => Some("7"),
        Keycode::Num8 => Some("8"),
        Keycode::Num9 => Some("9"),
        Keycode::Comma => Some(","),
        Keycode::Period => Some("."),
        Keycode::Slash => Some("/"),
        Keycode::Colon => Some(":"),
        Keycode::Semicolon => Some(";"),
        Keycode::Equals => Some("="),
        Keycode::Question => Some("?"),
        Keycode::Escape => Some("ESCAPE"),
        Keycode::Backspace => Some("BACKSPACE"),
        Keycode::Left => Some("LEFT"),
        Keycode::Right => Some("RIGHT"),
        Keycode::Up => Some("UP"),
        Keycode::Down => Some("DOWN"),
        Keycode::Space => Some("SPACE"),
        Keycode::Return => Some("RETURN"),
        Keycode::LShift => Some("SHIFT"),
        Keycode::RShift => Some("SHIFT"),
        Keycode::Tab => Some("TAB"),
        _ => None,
    }
}

fn translate_mouse_button(button: MouseButton) -> Option<&'static str>
{
    match button {
        MouseButton::Left => Some("LEFT"),
        MouseButton::Middle => Some("MIDDLE"),
        MouseButton::Right => Some("RIGHT"),
        MouseButton::X1 => Some("X1"),
        MouseButton::X2 => Some("X2"),
        _ => None
    }
}
