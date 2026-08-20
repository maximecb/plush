use rustc_hash::FxHashMap as HashMap;
use sdl2::audio::{AudioCallback, AudioSpecDesired, AudioDevice};
use std::sync::{Arc, Weak, Mutex, Condvar};
use crate::vm::{VM, Actor, Message};
use crate::value::*;
use crate::object::Object;
use crate::alloc::Alloc;
use crate::ast::{AUDIO_NEEDED_ID, AUDIO_DATA_ID};
use crate::window::with_sdl_context;
use crate::bytearray::ByteArray;
use crate::*;

// --- Audio Output ---

// SDL audio output callback
struct OutputCB
{
    // Number of audio output channels
    num_channels: usize,

    // Expected buffer size in samples
    buf_size: usize,

    // Actor responsible for generating audio
    actor_id: u64,

    // VM reference, to send messages to the parent actor
    vm: Arc<Mutex<VM>>,

    // Message allocator for the parent actor
    msg_alloc: Weak<Mutex<Alloc>>,
}

impl OutputCB
{
    /// Request more samples from the parent actor
    fn request_samples(&self, num_samples: usize)
    {
        // We'll use the message allocator of the parent thread
        let alloc_rc = self.msg_alloc.upgrade();
        if alloc_rc.is_none() {
            return; // Parent actor is terminated
        }
        let alloc_rc = alloc_rc.unwrap();
        let mut msg_alloc = alloc_rc.lock().unwrap();

        // Create the AudioNeeded object
        let bytes_before = msg_alloc.bytes_used();
        let obj = {
            let obj_val = Object::new(AUDIO_NEEDED_ID, 3, &mut msg_alloc);
            let obj = obj_val.as_obj();
            obj.set(0, Value::fixnum(num_samples as i64));
            obj.set(1, Value::fixnum(self.num_channels as i64));
            obj.set(2, Value::fixnum(0)); // device_id 0
            obj_val
        };
        let size = msg_alloc.bytes_used() - bytes_before;

        // Get the VM and send the message
        let vm = self.vm.lock().unwrap();
        let _ = vm.send_nocopy(self.actor_id, obj, size);
    }
}

impl AudioCallback for OutputCB
{
    // 32-bit floating-point samples
    type Channel = f32;

    /// This gets called when more audio samples are needed
    fn callback(&mut self, out: &mut [f32])
    {
        let output_len = out.len();
        assert!(output_len % self.num_channels == 0);
        let samples_per_chan = output_len / self.num_channels;
        assert!(samples_per_chan == self.buf_size);

        let (lock, cvar) = &AUDIO_OUT_PAIR;
        let mut audio_state_lock = lock.lock().unwrap();

        // If the queue doesn't have enough samples, wait
        while audio_state_lock.as_ref().unwrap().out_queue.len() < output_len {
            // Send a message to request more samples
            self.request_samples(output_len);

            // Wait for samples to be provided by the parent actor
            audio_state_lock = cvar.wait(audio_state_lock).unwrap();
        }

        // Copy samples to the output
        let state = audio_state_lock.as_mut().unwrap();
        let queue = &mut state.out_queue;
        assert!(queue.len() >= output_len);
        out.copy_from_slice(&queue[..output_len]);
        queue.drain(0..output_len);
    }
}

struct OutputState
{
    output_dev: AudioDevice<OutputCB>,

    // Samples queued for output
    out_queue: Vec<f32>,
}

unsafe impl Send for OutputState {}
static AUDIO_OUT_PAIR: (Mutex<Option<OutputState>>, Condvar) = (Mutex::new(None), Condvar::new());

/// Open an audio output device
pub fn audio_open_output(actor: &mut Actor, sample_rate: Value, num_channels: Value) -> Result<Value, String>
{
    {
        let (lock, _) = &AUDIO_OUT_PAIR;
        let audio_state = lock.lock().unwrap();
        if audio_state.is_some() {
            return Err("audio output device already open".into());
        }
    }

    let sample_rate = unwrap_u32!(sample_rate);
    let num_channels = unwrap_u32!(num_channels);

    if sample_rate != 44100 && sample_rate != 8000 {
        return Err("for now, only 44100Hz or 8000Hz sample rates supported".into());
    }

    if num_channels > 1 {
        return Err("for now, only one output channel supported".into());
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

    let (lock, _) = &AUDIO_OUT_PAIR;
    let mut audio_state = lock.lock().unwrap();
    *audio_state = Some(OutputState {
        output_dev: device,
        out_queue: Vec::new(),
    });

    // For now just assume device id zero
    Ok(Value::from(0))
}

/// Write samples to an audio device
/// The samples must be a ByteArray containing float32 values
pub fn audio_write_samples(actor: &mut Actor, device_id: Value, samples: Value) -> Result<Value, String>
{
    let device_id = unwrap_usize!(device_id);

    if device_id != 0 {
        return Err("for now, only one audio output device is supported".into());
    }

    let samples_ba = match samples.to_ba() {
        Some(ba) => ba,
        None => return Err("expected a byte array of samples".into())
    };

    let (lock, cvar) = &AUDIO_OUT_PAIR;
    let mut audio_state = lock.lock().unwrap();
    if audio_state.is_none() {
        return Err("audio output not open".into());
    }
    let state = audio_state.as_mut().unwrap();

    // The bytearray contains f32 samples
    // We need to iterate and read f32 values
    let num_samples = samples_ba.num_bytes() / std::mem::size_of::<f32>();
    for i in 0..num_samples {
        state.out_queue.push(samples_ba.get::<f32>(i));
    }

    // Notify the audio thread that samples are available
    cvar.notify_one();

    Ok(Value::NIL)
}

// --- Audio Input ---

// SDL audio input callback
struct InputCB
{
    // Number of audio input channels
    num_channels: usize,

    // Expected buffer size in samples
    buf_size: usize,

    // Actor responsible for receiving audio
    actor_id: u64,

    // VM reference, to send messages to the parent actor
    vm: Arc<Mutex<VM>>,

    // Message allocator for the parent actor
    msg_alloc: Weak<Mutex<Alloc>>,
}

impl InputCB
{
    /// Send an AudioData message to the parent actor
    fn send_audio_data_message(&self, device_id: usize, num_samples: usize)
    {
        // We'll use the message allocator of the parent thread
        let alloc_rc = self.msg_alloc.upgrade();
        if alloc_rc.is_none() {
            return; // Parent actor is terminated
        }
        let alloc_rc = alloc_rc.unwrap();
        let mut msg_alloc = alloc_rc.lock().unwrap();

        // Create the AudioData object
        let bytes_before = msg_alloc.bytes_used();
        let obj = {
            let obj_val = Object::new(AUDIO_DATA_ID, 2, &mut msg_alloc);
            let obj = obj_val.as_obj();
            obj.set(0, Value::fixnum(device_id as i64));
            obj.set(1, Value::fixnum(num_samples as i64));
            obj_val
        };
        let size = msg_alloc.bytes_used() - bytes_before;

        // Get the VM and send the message
        let vm = self.vm.lock().unwrap();
        let _ = vm.send_nocopy(self.actor_id, obj, size);
    }
}

impl AudioCallback for InputCB
{
    // 32-bit floating-point samples
    type Channel = f32;

    /// This gets called when new audio samples are available
    fn callback(&mut self, input: &mut [f32])
    {
        let input_len = input.len();
        assert!(input_len % self.num_channels == 0);
        let samples_per_chan = input_len / self.num_channels;
        assert!(samples_per_chan == self.buf_size);

        let (lock, cvar) = &AUDIO_IN_PAIR;
        let mut audio_state_lock = lock.lock().unwrap();

        // Clip the samples in [-1, 1] for portability
        for mut s in input.iter_mut() {
            *s = s.max(-1.0).min(1.0);
        }

        let state = audio_state_lock.as_mut().unwrap();

        // Clear the samples in the queue
        // If the thread processing the input falls behind for some reason,
        // we can't let samples infinitely accumulate in the queue, otherwise
        // there is some risk that we will never catch up to the backlog
        state.in_queue.clear();

        // Write new samples to the input queue
        state.in_queue.extend_from_slice(input);

        // Send a message to the Plush actor that samples are available
        // For now, device_id is hardcoded to 1 for input
        self.send_audio_data_message(1, input_len);

        // Notify any waiting Plush actors that samples are available
        cvar.notify_one();
    }
}

struct InputState
{
    input_dev: AudioDevice<InputCB>,

    // Samples queued from input
    in_queue: Vec<f32>,
}

unsafe impl Send for InputState {}
static AUDIO_IN_PAIR: (Mutex<Option<InputState>>, Condvar) = (Mutex::new(None), Condvar::new());

/// Open an audio input device
pub fn audio_open_input(actor: &mut Actor, sample_rate: Value, num_channels: Value) -> Result<Value, String>
{
    {
        let (lock, _) = &AUDIO_IN_PAIR;
        let audio_state = lock.lock().unwrap();
        if audio_state.is_some() {
            return Err("audio input device already open".into());
        }
    }

    let sample_rate = unwrap_u32!(sample_rate);
    let num_channels = unwrap_u32!(num_channels);

    if sample_rate != 44100 {
        return Err("for now, only 44100Hz sample rate supported".into());
    }

    if num_channels > 1 {
        return Err("for now, only one input channel supported".into());
    }

    let desired_spec = AudioSpecDesired {
        freq: Some(sample_rate as i32),
        channels: Some(num_channels as u8),
        samples: Some(1024) // buffer size, 1024 samples
    };

    let audio_subsystem = with_sdl_context(|sdl| sdl.audio().unwrap());

    let device = audio_subsystem.open_capture(None, &desired_spec, |spec| {
        InputCB {
            num_channels: num_channels as usize,
            buf_size: spec.samples as usize,
            actor_id: actor.actor_id,
            vm: actor.vm.clone(),
            msg_alloc: Arc::downgrade(&actor.msg_alloc),
        }
    }).unwrap();

    device.resume();

    let (lock, _) = &AUDIO_IN_PAIR;
    let mut audio_state = lock.lock().unwrap();
    *audio_state = Some(InputState {
        input_dev: device,
        in_queue: Vec::new(),
    });

    // For now just assume device id zero
    Ok(Value::from(0))
}

/// Read samples from an audio input device into an existing ByteArray
pub fn audio_read_samples(actor: &mut Actor, device_id: Value, num_samples: Value, dst_ba: Value, dst_idx: Value) -> Result<Value, String>
{
    let device_id = unwrap_usize!(device_id);
    let num_samples_to_read = unwrap_usize!(num_samples);
    let dst_idx_f32 = unwrap_usize!(dst_idx);
    let dst_ba = unwrap_ba!(dst_ba);

    if device_id != 0 {
        return Err("for now, only one audio input device is supported".into());
    }

    // Checked before waiting, so that a destination that could never hold
    // the samples is reported instead of blocking on them
    let dst_ba_len_f32 = dst_ba.num_bytes() / std::mem::size_of::<f32>();
    if dst_idx_f32 + num_samples_to_read > dst_ba_len_f32 {
        return Err("destination byte array is too small to hold the samples".into());
    }

    let (lock, cvar) = &AUDIO_IN_PAIR;
    let mut audio_state_lock = lock.lock().unwrap();
    if audio_state_lock.is_none() {
        return Err("audio input not open".into());
    }

    // Wait until enough samples are available
    loop {
        let state = audio_state_lock.as_mut().unwrap();
        if state.in_queue.len() >= num_samples_to_read {
            break;
        }
        audio_state_lock = cvar.wait(audio_state_lock).unwrap();
    }

    let state = audio_state_lock.as_mut().unwrap();

    // Copy samples from in_queue to dst_ba using get_slice_mut
    unsafe {
        let dst_slice = dst_ba.get_slice_mut::<f32>(dst_idx_f32, num_samples_to_read);
        dst_slice.copy_from_slice(&state.in_queue[0..num_samples_to_read]);
    }

    state.in_queue.drain(0..num_samples_to_read);

    Ok(Value::NIL)
}
