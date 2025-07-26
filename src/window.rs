// Simple display/window device

extern crate sdl2;
use sdl2::pixels::Color;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use sdl2::surface::Surface;
use sdl2::render::Texture;
use sdl2::render::TextureAccess;
use sdl2::pixels::PixelFormatEnum;
use sdl2::video::WindowContext;
use std::sync::{Mutex, mpsc};
use std::time::Duration;
use crate::vm::{VM, Value, Actor};
use crate::bytearray::ByteArray;

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
    window_id: u32,

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
    flags: Value
) -> Value
{
    if actor.actor_id != 0 {
        panic!("window functions should only be called from the main actor");
    }

    let window = WINDOW.lock().unwrap();
    if window.is_some() {
        panic!("for now, only one window supported");
    }
    drop(window);

    let width: u32 = width.unwrap_u32();
    let height: u32 = height.unwrap_u32();
    let title_str = title.unwrap_rust_str();

    init_sdl_video();
    let mut sdl_state = SDL_STATE.lock().unwrap();
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
        window_id: 0,
        canvas,
        texture_creator,
        texture: None,
    };

    let mut global_window = WINDOW.lock().unwrap();
    *global_window = Some(window);

    // TODO: return unique window id
    Value::from(0)
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
)
{
    if actor.actor_id != 0 {
        panic!("window functions should only be called from the main actor");
    }

    let window_id = window_id.unwrap_u32();
    let frame = match frame {
        Value::ByteArray(p) => unsafe { &*p }
        _ => panic!()
    };

    assert!(window_id == 0);
    let mut window_lock = WINDOW.lock().unwrap();
    let mut window = window_lock.as_mut().unwrap();

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
}





/// Poll for UI events
pub fn poll_ui_msg(actor: &mut Actor) -> Option<Value>
{
    // This should only ever be called on the main thread
    assert!(actor.actor_id == 0);

    let mut sdl_state = SDL_STATE.lock().unwrap();

    if sdl_state.event_pump.is_none() {
        let sdl = sdl_state.sdl.as_ref().unwrap();
        sdl_state.event_pump = Some(sdl.event_pump().unwrap());
    }

    let mut event_pump = sdl_state.event_pump.as_mut().unwrap();

    let event = event_pump.poll_event();

    if event.is_none() {
        return None;
    }



    /*
    match event.unwrap() {
        Event::Quit { .. } => {
            println!("got quit event");

            // { event: 'window_closed', window_id: 0 }
            let alloc = &mut actor.alloc;
            let obj = Object::new(alloc);

            Object::def_const(
                obj,
                alloc.get_string("event"),
                alloc.get_string("window_closed"),
            );

            Object::def_const(
                obj,
                alloc.get_string("window_id"),
                Value::from(0),
            );

            Object::seal(obj);

            Some(Value::Object(obj))
        }

        _ => None
    }
    */


    todo!()

}









// TODO: we probably want to use string constants for the event types and buttons?
// Can we create a local class for UI events?


/*
/// Process SDL events
pub fn process_events(vm: &mut VM) -> ExitReason
{
    let mut event_pump = get_sdl_context().event_pump().unwrap();

    // Process all pending events
    // See: https://docs.rs/sdl2/0.30.0/sdl2/event/enum.Event.html
    // TODO: we probably want to process window/input related events in window.rs ?
    for event in event_pump.poll_iter() {
        match event {
            Event::Quit { .. } => {
                return ExitReason::Exit(Value::from(0));
            }

            Event::MouseMotion { window_id, x, y, .. } => {
                if let ExitReason::Exit(val) = window_call_mousemove(vm, window_id, x, y) {
                    return ExitReason::Exit(val);
                }
            }

            Event::MouseButtonDown { window_id, which, mouse_btn, x, y, .. } => {
                if let ExitReason::Exit(val) = window_call_mousedown(vm, window_id, mouse_btn, x, y) {
                    return ExitReason::Exit(val);
                }
            }

            Event::MouseButtonUp { window_id, which, mouse_btn, x, y, .. } => {
                if let ExitReason::Exit(val) = window_call_mouseup(vm, window_id, mouse_btn, x, y) {
                    return ExitReason::Exit(val);
                }
            }

            Event::KeyDown { window_id, keycode: Some(keycode), .. } => {
                if let ExitReason::Exit(val) = window_call_keydown(vm, window_id, keycode) {
                    return ExitReason::Exit(val);
                }
            }

            Event::KeyUp { window_id, keycode: Some(keycode), .. } => {
                if let ExitReason::Exit(val) = window_call_keyup(vm, window_id, keycode) {
                    return ExitReason::Exit(val);
                }
            }

            Event::TextInput { window_id, text, .. } => {
                // For each UTF-8 byte of input
                for ch in text.bytes() {
                    if let ExitReason::Exit(val) = window_call_textinput(vm, window_id, ch) {
                        return ExitReason::Exit(val);
                    }
                }
            }

            _ => {}
        }
    }

    return ExitReason::default();
}
*/




/*
fn translate_keycode(sdl_keycode: Keycode) -> Option<u16>
{
    use crate::sys::constants::*;

    // https://docs.rs/sdl2/0.30.0/sdl2/keyboard/enum.Keycode.html
    match sdl_keycode {
        Keycode::A => Some(KEY_A),
        Keycode::B => Some(KEY_B),
        Keycode::C => Some(KEY_C),
        Keycode::D => Some(KEY_D),
        Keycode::E => Some(KEY_E),
        Keycode::F => Some(KEY_F),
        Keycode::G => Some(KEY_G),
        Keycode::H => Some(KEY_H),
        Keycode::I => Some(KEY_I),
        Keycode::J => Some(KEY_J),
        Keycode::K => Some(KEY_K),
        Keycode::L => Some(KEY_L),
        Keycode::M => Some(KEY_M),
        Keycode::N => Some(KEY_N),
        Keycode::O => Some(KEY_O),
        Keycode::P => Some(KEY_P),
        Keycode::Q => Some(KEY_Q),
        Keycode::R => Some(KEY_R),
        Keycode::S => Some(KEY_S),
        Keycode::T => Some(KEY_T),
        Keycode::U => Some(KEY_U),
        Keycode::V => Some(KEY_V),
        Keycode::W => Some(KEY_W),
        Keycode::X => Some(KEY_X),
        Keycode::Y => Some(KEY_Y),
        Keycode::Z => Some(KEY_Z),

        Keycode::Num0 => Some(KEY_NUM0),
        Keycode::Num1 => Some(KEY_NUM1),
        Keycode::Num2 => Some(KEY_NUM2),
        Keycode::Num3 => Some(KEY_NUM3),
        Keycode::Num4 => Some(KEY_NUM4),
        Keycode::Num5 => Some(KEY_NUM5),
        Keycode::Num6 => Some(KEY_NUM6),
        Keycode::Num7 => Some(KEY_NUM7),
        Keycode::Num8 => Some(KEY_NUM8),
        Keycode::Num9 => Some(KEY_NUM9),

        Keycode::Comma => Some(KEY_COMMA),
        Keycode::Period => Some(KEY_PERIOD),
        Keycode::Slash => Some(KEY_SLASH),
        Keycode::Colon => Some(KEY_COLON),
        Keycode::Semicolon => Some(KEY_SEMICOLON),
        Keycode::Equals => Some(KEY_EQUALS),
        Keycode::Question => Some(KEY_QUESTION),

        Keycode::Escape => Some(KEY_ESCAPE),
        Keycode::Backspace => Some(KEY_BACKSPACE),
        Keycode::Left => Some(KEY_LEFT),
        Keycode::Right => Some(KEY_RIGHT),
        Keycode::Up => Some(KEY_UP),
        Keycode::Down => Some(KEY_DOWN),
        Keycode::Space => Some(KEY_SPACE),
        Keycode::Return => Some(KEY_RETURN),
        Keycode::LShift => Some(KEY_SHIFT),
        Keycode::RShift => Some(KEY_SHIFT),
        Keycode::Tab => Some(KEY_TAB),

        _ => None
    }
}
*/
