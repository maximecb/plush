use sdl2::audio::{AudioCallback, AudioSpecDesired, AudioDevice};
use std::sync::{Arc, Weak, Mutex, Condvar};
use std::collections::HashMap;
use crate::vm::{Actor, Message, MsgAlloc, Object, Value, VM};
use crate::alloc::Alloc;
use crate::ast::{AUDIO_NEEDED_ID, AUDIO_DATA_ID};
use crate::window::with_sdl_context;
use crate::bytearray::ByteArray;

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
    msg_alloc: MsgAlloc,
}

impl OutputCB
{
    /// Request more samples from the parent actor
    fn request_samples(&self, num_samples: usize)
    {
        // Create the AudioNeeded object
        let obj = {
            let mut obj_val = match self.msg_alloc.new_object(AUDIO_NEEDED_ID, 3) {
                Ok(obj_val) => obj_val,
                Err(_) => return, // This means that the parent actor is no longer available
            };
            let obj = obj_val.unwrap_obj();
            obj.set(0, Value::from(num_samples));
            obj.set(1, Value::from(self.num_channels));
            obj.set(2, Value::from(0)); // device_id 0
            obj_val
        };

        // Get the VM and send the message
        let vm = self.vm.lock().unwrap();
        let _ = vm.send_nocopy(self.actor_id, obj);
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

    let sample_rate = sample_rate.unwrap_u32();
    let num_channels = num_channels.unwrap_u32();

    if sample_rate != 44100 {
        return Err("for now, only 44100Hz sample rate supported".into());
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
            msg_alloc: actor.msg_alloc(),
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
    let device_id = device_id.unwrap_usize();

    if device_id != 0 {
        return Err("for now, only one audio output device is supported".into());
    }

    let (lock, cvar) = &AUDIO_OUT_PAIR;
    let mut audio_state = lock.lock().unwrap();
    if audio_state.is_none() {
        return Err("audio output not open".into());
    }
    let state = audio_state.as_mut().unwrap();

    let samples_ba = match samples {
        Value::ByteArray(p) => unsafe { &mut *p },
        _ => return Err("expected a byte array of samples".into())
    };

    // The bytearray contains f32 samples
    // We need to iterate and read f32 values
    let num_samples = samples_ba.num_bytes() / std::mem::size_of::<f32>();
    for i in 0..num_samples {
        state.out_queue.push(samples_ba.load::<f32>(i));
    }

    // Notify the audio thread that samples are available
    cvar.notify_one();

    Ok(Value::Nil)
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
        let obj = {
            let mut obj_val = match msg_alloc.new_object(AUDIO_DATA_ID, 2) {
                Ok(obj_val) => obj_val,
                Err(err) => return, // This means that the parent actor is terminated
            };
            let obj = obj_val.unwrap_obj();
            obj.set(0, Value::from(device_id));
            obj.set(1, Value::from(num_samples));
            obj_val
        };

        // Get the VM and send the message
        let vm = self.vm.lock().unwrap();
        let _ = vm.send_nocopy(self.actor_id, obj);
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
            panic!("audio input device already open");
        }
    }

    let sample_rate = sample_rate.unwrap_u32();
    let num_channels = num_channels.unwrap_u32();

    if sample_rate != 44100 {
        panic!("for now, only 44100Hz sample rate supported");
    }

    if num_channels > 1 {
        panic!("for now, only one input channel supported");
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
    let device_id = device_id.unwrap_usize();
    let num_samples_to_read = num_samples.unwrap_usize();
    let dst_idx_f32 = dst_idx.unwrap_usize();

    if device_id != 0 {
        panic!("for now, only one audio input device is supported");
    }

    let (lock, cvar) = &AUDIO_IN_PAIR;
    let mut audio_state_lock = lock.lock().unwrap();
    if audio_state_lock.is_none() {
        panic!("audio input not open");
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

    let dst_ba_ptr = match dst_ba {
        Value::ByteArray(p) => p,
        _ => panic!("expected a byte array for dst_ba")
    };

    // Ensure dst_ba has enough space
    let dst_ba_len_f32 = unsafe { (*dst_ba_ptr).num_bytes() } / std::mem::size_of::<f32>();
    if dst_idx_f32 + num_samples_to_read > dst_ba_len_f32 {
        panic!("dst_ba does not have enough space for samples at given dst_idx");
    }

    // Copy samples from in_queue to dst_ba using get_slice_mut
    unsafe {
        let dst_slice = (*dst_ba_ptr).get_slice_mut::<f32>(dst_idx_f32, num_samples_to_read);
        dst_slice.copy_from_slice(&state.in_queue[0..num_samples_to_read]);
    }

    state.in_queue.drain(0..num_samples_to_read);

    Ok(Value::Nil)
}
