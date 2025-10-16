use sdl2::audio::{AudioCallback, AudioSpecDesired, AudioDevice};
use std::sync::{Arc, Weak, Mutex, Condvar};
use std::collections::HashMap;
use crate::vm::{Value, VM, Actor, Object, Message};
use crate::alloc::Alloc;
use crate::ast::AUDIO_NEEDED_ID;
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
        let alloc_rc = self.msg_alloc.upgrade().unwrap();
        let mut msg_alloc = alloc_rc.lock().unwrap();

        // Create the AudioNeeded object
        let obj = {
            let mut obj = Object::new(AUDIO_NEEDED_ID, 3);
            obj.slots[0] = Value::from(num_samples);
            obj.slots[1] = Value::from(self.num_channels);
            obj.slots[2] = Value::from(0); // device_id 0
            Value::Object(msg_alloc.alloc(obj))
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

        let (lock, cvar) = &AUDIO_PAIR;
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
static AUDIO_PAIR: (Mutex<Option<OutputState>>, Condvar) = (Mutex::new(None), Condvar::new());

/// Open an audio output device
pub fn audio_open_output(actor: &mut Actor, sample_rate: Value, num_channels: Value) -> Value
{
    {
        let (lock, _) = &AUDIO_PAIR;
        let audio_state = lock.lock().unwrap();
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

    let (lock, _) = &AUDIO_PAIR;
    let mut audio_state = lock.lock().unwrap();
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

    let (lock, cvar) = &AUDIO_PAIR;
    let mut audio_state = lock.lock().unwrap();
    if audio_state.is_none() {
        panic!("audio output not open");
    }
    let state = audio_state.as_mut().unwrap();

    let samples_ba = match samples {
        Value::ByteArray(p) => unsafe { &*p },
        _ => panic!("expected a byte array of samples")
    };

    // The bytearray contains f32 samples
    let num_samples = samples_ba.num_bytes() / std::mem::size_of::<f32>();
    let sample_slice = unsafe { samples_ba.get_slice::<f32>(0, num_samples) };

    state.out_queue.extend_from_slice(sample_slice);

    // Notify the audio thread that samples are available
    cvar.notify_one();
}
