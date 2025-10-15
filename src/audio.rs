use sdl2::audio::{AudioCallback, AudioSpecDesired, AudioDevice};
use std::sync::{Arc, Weak, Mutex};
use std::collections::HashMap;
use crate::vm::{Value, VM, Actor};
use crate::alloc::Alloc;
use crate::window::with_sdl_context;

// SDL audio output callback
struct OutputCB
{
    // Number of audio output channels
    num_channels: usize,

    // Expected buffer size in samples
    buf_size: usize,

    // Actor responsible for generating audio
    actor_id: u64,

    // VM reference, to send messages to the actor
    vm: Arc<Mutex<VM>>,

    // Message allocator for the actor
    msg_alloc: Weak<Mutex<Alloc>>,
}

impl AudioCallback for OutputCB
{
    // 32-bit floating-point samples
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32])
    {
        let output_len = out.len();
        assert!(output_len % self.num_channels == 0);
        let samples_per_chan = output_len / self.num_channels;
        assert!(samples_per_chan == self.buf_size);

        // Clear the buffer
        out.fill(0.0);










        todo!();
    }
}

//#[derive(Default)]
struct OutputState
{
    output_dev: AudioDevice<OutputCB>,

    // Samples queued for output
    out_queue: Vec<f32>,
}

unsafe impl Send for OutputState {}
static AUDIO_STATE: Mutex<Option<OutputState>> = Mutex::new(None);

pub fn audio_open_output(actor: &mut Actor, sample_rate: Value, num_channels: Value) -> Value
{
    {
        let audio_state = AUDIO_STATE.lock().unwrap();
        if audio_state.is_some() {
            panic!("audio output device already open");
        }
    }

    let sample_rate = sample_rate.unwrap_u32();
    let num_channels = num_channels.unwrap_u32();

    if sample_rate != 44100 {
        panic!("for now, only 44100Hz sample rate supported");
    }

    if num_channels > 1 {
        panic!("for now, only one output channel supported");
    }

    let desired_spec = AudioSpecDesired {
        freq: Some(sample_rate as i32),
        channels: Some(num_channels as u8),
        samples: Some(1024) // buffer size, 1024 samples
    };

    let audio_subsystem = with_sdl_context(|sdl| sdl.audio().unwrap());

    let device = audio_subsystem.open_playback(None, &desired_spec, |spec| {
        // The audio callback runs in a separate thread, so we need to
        // clone the actor's VM and allocator references
        OutputCB {
            num_channels: num_channels as usize,
            buf_size: spec.samples as usize,
            actor_id: actor.actor_id,
            vm: actor.vm.clone(),
            msg_alloc: Arc::downgrade(&actor.msg_alloc),
        }
    }).unwrap();

    device.resume();

    let mut audio_state = AUDIO_STATE.lock().unwrap();
    *audio_state = Some(OutputState {
        output_dev: device,
        out_queue: Vec::new(),
    });

    // For now just assume device id zero
    Value::from(0)
}

/// Write samples to an audio device
/// The samples must be a ByteArray containing float32 values
pub fn audio_write_samples(actor: &mut Actor, device_id: Value, samples: Value)
{
    let device_id = device_id.unwrap_usize();

    if device_id != 0 {
        panic!("for now, only one audio output device is supported");
    }








    todo!();
}











/*
// Audio input callback
struct InputCB
{
    // Number of audio input channels
    num_channels: usize,

    // Expected buffer size
    buf_size: usize,

    // VM thread in which to execute the audio callback
    thread: Thread,

    // Callback function pointer
    cb: u64,
}

impl AudioCallback for InputCB
{
    // Using signed 16-bit samples
    type Channel = i16;

    // Receives a buffer of input samples
    fn callback(&mut self, buf: &mut [i16])
    {
        assert!(buf.len() % self.num_channels == 0);
        let samples_per_chan = buf.len() / self.num_channels;
        assert!(samples_per_chan == self.buf_size);

        // Copy the samples to make them accessible to the audio thread
        INPUT_STATE.with_borrow_mut(|s| {
            s.input_tid = self.thread.id;
            s.samples.resize(buf.len(), 0);
            s.samples.copy_from_slice(buf);
        });

        // Run the audio callback
        let ptr = self.thread.call(self.cb, &[Value::from(self.num_channels), Value::from(samples_per_chan)]);
    }
}
*/

/*
#[derive(Default)]
struct InputState
{
    // Thread doing the audio input
    input_tid: u64,

    // Samples available to read
    samples: Vec<i16>,
}
*/

/*
pub fn audio_open_input(thread: &mut Thread, sample_rate: Value, num_channels: Value, format: Value, cb: Value) -> Value
{
    if thread.id != 0 {
        panic!("audio functions should only be called from the main thread");
    }

    AUDIO_STATE.with_borrow(|s| {
        if s.input_dev.is_some() {
            panic!("audio input device already open");
        }
    });

    let sample_rate = sample_rate.as_u32();
    let num_channels = num_channels.as_u16();
    let format = format.as_u16();
    let cb = cb.as_u64();

    if sample_rate != 44100 {
        panic!("for now, only 44100Hz sample rate suppored");
    }

    //if num_channels > 2 {
    if num_channels != 1 {
        panic!("for now, only one output channel supported");
    }

    if format != AUDIO_FORMAT_I16 {
        panic!("for now, only i16, 16-bit signed audio format supported");
    }

    let desired_spec = AudioSpecDesired {
        freq: Some(sample_rate as i32),
        channels: Some(num_channels as u8),
        samples: Some(1024) // buffer size, 1024 samples
    };

    // Create a new VM thread in which to run the audio callback
    let audio_thread = VM::new_thread(&thread.vm);

    let sdl = get_sdl_context();
    let audio_subsystem = sdl.audio().unwrap();

    let device = audio_subsystem.open_capture(None, &desired_spec, |spec| {
        InputCB {
            num_channels: num_channels.into(),
            buf_size: desired_spec.samples.unwrap() as usize,
            thread: audio_thread,
            cb: cb,
        }
    }).unwrap();

    // Start playback
    device.resume();

    // Keep the audio device alive
    AUDIO_STATE.with_borrow_mut(|s| {
        s.input_dev = Some(device);
    });

    // FIXME: return the device_id (u32)
    Value::from(1)
}

/// Read audio samples from an audio input thread
pub fn audio_read_samples(thread: &mut Thread, dst_ptr: Value, num_samples: Value)
{
    let dst_ptr = dst_ptr.as_usize();
    let num_samples = num_samples.as_usize();

    INPUT_STATE.with_borrow_mut(|s| {
        if s.input_tid != thread.id {
            panic!("can only read audio samples from audio input thread");
        }

        // For now, force reading all available samples
        if num_samples != s.samples.len() {
            panic!("must read all available samples");
        }

        let dst_buf: &mut [i16] = thread.get_heap_slice_mut(dst_ptr, num_samples);
        dst_buf.copy_from_slice(&s.samples);

        s.samples.clear();
    });
}
*/
