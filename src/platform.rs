use gpui::Window;

/// Whether the app may use capture permission at all — mic access gates the
/// boss-voice listener, and speech-recognition access gates "go ahead"
/// consent. Both fail open: denied means boss speech behaves as if the gate
/// did not exist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureAccess {
    Granted,
    Denied,
    Undetermined,
}

/// What the consent recognizer observed — the app turns these into events on
/// its event pump, off the recognition thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsentSignal {
    /// The on-device recognizer heard the consent phrase.
    Heard,
    /// The recognition task ended on its own — an error or the natural end
    /// of an utterance. The app restarts it while consent is still wanted.
    Ended,
}

/// True when running inside an app bundle. TCC-backed capture and
/// recognition APIs are unsafe to touch from a bare executable — tests and
/// `cargo run` must take the denied path so playback still works.
#[cfg(target_os = "macos")]
fn in_app_bundle() -> bool {
    objc2_foundation::NSBundle::mainBundle()
        .bundleIdentifier()
        .is_some()
}

/// The app's microphone permission as macOS reports it. No prompt is ever
/// shown by asking — `Undetermined` means `request_microphone_access` would
/// have to ask.
#[cfg(target_os = "macos")]
pub fn microphone_access() -> CaptureAccess {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    if !in_app_bundle() {
        return CaptureAccess::Denied;
    }
    let Some(media_type) = (unsafe { AVMediaTypeAudio }) else {
        return CaptureAccess::Denied;
    };
    match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) } {
        status if status == AVAuthorizationStatus::Authorized => CaptureAccess::Granted,
        status if status == AVAuthorizationStatus::NotDetermined => CaptureAccess::Undetermined,
        _ => CaptureAccess::Denied,
    }
}

/// Ask macOS for mic access; `done` fires once on an arbitrary queue with
/// the answer. Call only when `microphone_access()` is `Undetermined`.
#[cfg(target_os = "macos")]
pub fn request_microphone_access(done: Box<dyn Fn(bool) + Send + Sync + 'static>) {
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};

    if !in_app_bundle() {
        done(false);
        return;
    }
    let Some(media_type) = (unsafe { AVMediaTypeAudio }) else {
        done(false);
        return;
    };
    let done = std::sync::Mutex::new(Some(done));
    let handler = block2::RcBlock::new(move |granted: Bool| {
        if let Some(done) = done.lock().unwrap().take() {
            done(granted.as_bool());
        }
    });
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &handler) };
}

/// The app's speech-recognition permission — a separate TCC item from the
/// mic, required before the consent recognizer may run.
#[cfg(target_os = "macos")]
pub fn speech_recognition_access() -> CaptureAccess {
    use objc2_speech::{SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus};

    if !in_app_bundle() {
        return CaptureAccess::Denied;
    }
    match unsafe { SFSpeechRecognizer::authorizationStatus() } {
        status if status == SFSpeechRecognizerAuthorizationStatus::Authorized => {
            CaptureAccess::Granted
        }
        status if status == SFSpeechRecognizerAuthorizationStatus::NotDetermined => {
            CaptureAccess::Undetermined
        }
        _ => CaptureAccess::Denied,
    }
}

/// Ask macOS for speech-recognition access; `done` fires once with the
/// answer. Call only when `speech_recognition_access()` is `Undetermined` —
/// without `NSSpeechRecognitionUsageDescription` in the bundle's plist this
/// crashes, which is also why bare executables must not reach it.
#[cfg(target_os = "macos")]
pub fn request_speech_recognition_access(done: Box<dyn Fn(bool) + Send + Sync + 'static>) {
    use objc2_speech::{SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus};

    if !in_app_bundle() {
        done(false);
        return;
    }
    let done = std::sync::Mutex::new(Some(done));
    let handler = block2::RcBlock::new(move |status: SFSpeechRecognizerAuthorizationStatus| {
        if let Some(done) = done.lock().unwrap().take() {
            done(status == SFSpeechRecognizerAuthorizationStatus::Authorized);
        }
    });
    unsafe { SFSpeechRecognizer::requestAuthorization(&handler) };
}

/// Whether a lowercase, punctuation-normalized transcript contains the
/// spoken consent — "go ahead" with any words around it.
#[cfg(target_os = "macos")]
fn hears_consent(text: &str) -> bool {
    let normalized: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .contains("go ahead")
}

#[cfg(target_os = "macos")]
mod voice_gate {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_audio_toolbox::{
        AudioUnitSetProperty, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global,
    };
    use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};
    use objc2_core_audio::{
        AudioObjectAddPropertyListenerBlock, AudioObjectGetPropertyData,
        AudioObjectGetPropertyDataSize, AudioObjectID, AudioObjectPropertyAddress,
        kAudioDevicePropertyDeviceUID, kAudioDevicePropertyStreamConfiguration,
        kAudioHardwarePropertyDefaultInputDevice, kAudioHardwarePropertyDevices,
        kAudioObjectPropertyElementMain, kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyScopeInput, kAudioObjectSystemObject,
    };
    use objc2_foundation::{NSArray, NSError, NSString};
    use objc2_speech::{
        SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask,
        SFSpeechRecognitionTaskHint, SFSpeechRecognizer,
    };

    use super::ConsentSignal;

    /// Loud input refreshes this many ~100 ms tap blocks of "someone is
    /// speaking" — an ~800 ms release tail so the gate does not flap on
    /// pauses between words.
    const AMBIENT_RELEASE_WINDOWS: u32 = 8;
    /// RMS of a quarter-sampled block that counts as speech — above room
    /// noise, below normal voice at arm's length.
    const AMBIENT_RMS_THRESHOLD: f32 = 0.012;

    static AMBIENT_WINDOWS: AtomicU32 = AtomicU32::new(0);
    /// One consent per listen session — the handler can report the same
    /// "go ahead" across partial and final results back to back.
    static CONSENT_HEARD: AtomicBool = AtomicBool::new(false);

    /// The request the live tap feeds. `Retained` is `!Send`, so this
    /// crosses threads under a mutex; the tap only `try_lock`s — it drops a
    /// buffer rather than block an audio I/O thread on recognition setup.
    struct Feed(Option<Retained<SFSpeechAudioBufferRecognitionRequest>>);
    unsafe impl Send for Feed {}
    static CONSENT_FEED: Mutex<Feed> = Mutex::new(Feed(None));

    /// Temporary PCM captured only while the user has explicitly started
    /// dictation. The audio I/O callback uses `try_lock` and drops samples if
    /// the consumer is briefly busy.
    static DICTATION_CAPTURE: Mutex<Option<(u32, Vec<f32>)>> = Mutex::new(None);

    type ConsentHook = Box<dyn Fn(ConsentSignal) + Send + Sync>;
    static CONSENT_HOOK: Mutex<Option<ConsentHook>> = Mutex::new(None);

    /// The dictation scratchpad's consumer for captured audio. Unlike the
    /// consent feed these samples leave the process — they stream to the
    /// transcription gateway — so the sink is registered only while a
    /// scratchpad owns capture; mute and cancel tear it down rather than
    /// gating the tap.
    type AudioSink = Box<dyn Fn(&[f32], f64) + Send + Sync>;
    static AUDIO_SINK: Mutex<Option<AudioSink>> = Mutex::new(None);

    /// The user's pinned input device UID — `None` follows the system
    /// default input, which macOS retargets whenever a Bluetooth device
    /// connects or drops.
    static PREFERRED_INPUT_UID: Mutex<Option<String>> = Mutex::new(None);
    /// Whether the chosen input (unpinned: any input device) is present.
    static INPUT_AVAILABLE: AtomicBool = AtomicBool::new(true);
    /// Set while any consumer still wants the mic — a returning device
    /// rebuilds the engine it took away.
    static ENGINE_WANTED: AtomicBool = AtomicBool::new(false);
    /// The AudioDeviceID the running engine is bound to: the pinned device
    /// when one is chosen, the observed default input otherwise.
    static BOUND_DEVICE: Mutex<Option<AudioObjectID>> = Mutex::new(None);
    /// Device-list changes report through here — the app forwards them onto
    /// the event pump so `devices_changed` can rebuild on the thread that
    /// owns the engine.
    static DEVICE_CHANGE_HOOK: Mutex<Option<Box<dyn Fn() + Send + Sync>>> = Mutex::new(None);
    static DEVICE_LISTENER_INSTALLED: AtomicBool = AtomicBool::new(false);

    /// The recognizer objects — created and retired on the main thread only.
    /// The recognizer is only held, never called: the task may not retain it.
    struct ConsentSession {
        request: Retained<SFSpeechAudioBufferRecognitionRequest>,
        task: Retained<SFSpeechRecognitionTask>,
        _recognizer: Retained<SFSpeechRecognizer>,
    }

    struct VoiceListener {
        engine: Retained<AVAudioEngine>,
        consent: Option<ConsentSession>,
    }

    thread_local! {
        static VOICE_LISTENER: RefCell<Option<VoiceListener>> = const { RefCell::new(None) };
    }

    /// Fold one tap block into the ambient-energy window. Planar float PCM
    /// is the input node's normal capture format; one channel and every
    /// fourth sample is plenty for an energy read.
    fn observe_ambient_level(buffer: &AVAudioPCMBuffer) {
        let frames = unsafe { buffer.frameLength() } as usize;
        let channels = unsafe { buffer.floatChannelData() };
        if frames == 0 || channels.is_null() {
            return;
        }
        let samples = unsafe { *channels }.as_ptr();
        let energy = (0..frames)
            .step_by(4)
            .map(|index| {
                let sample = unsafe { *samples.add(index) };
                sample * sample
            })
            .sum::<f32>()
            / (frames.div_ceil(4) as f32);
        if energy.sqrt() >= AMBIENT_RMS_THRESHOLD {
            AMBIENT_WINDOWS.store(AMBIENT_RELEASE_WINDOWS, Ordering::Relaxed);
        } else {
            let _ = AMBIENT_WINDOWS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |windows| {
                Some(windows.saturating_sub(1))
            });
        }
    }

    /// Downmix one tap block to mono f32 plus its sample rate — `None` when
    /// the buffer carries no float samples.
    fn mono_samples(buffer: &AVAudioPCMBuffer) -> Option<(Vec<f32>, f64)> {
        let frames = unsafe { buffer.frameLength() } as usize;
        let channels = unsafe { buffer.floatChannelData() };
        if frames == 0 || channels.is_null() {
            return None;
        }
        let format = unsafe { buffer.format() };
        let channel_count = unsafe { format.channelCount() } as usize;
        let rate = unsafe { format.sampleRate() };
        if channel_count == 0 {
            return None;
        }
        // Planar buffers give each channel its own chunk with stride 1;
        // interleaved buffers share one chunk — channel pointers sit at
        // their own offset and frames step `stride` samples apart.
        let stride = unsafe { buffer.stride() } as usize;
        let mut mono = Vec::with_capacity(frames);
        for frame in 0..frames {
            let mut sum = 0.0f32;
            for channel in 0..channel_count {
                let data = unsafe { *channels.add(channel) }.as_ptr();
                sum += unsafe { *data.add(frame * stride) };
            }
            mono.push(sum / channel_count as f32);
        }
        Some((mono, rate))
    }

    /// The recognizer's answer to one callback: consent heard, task ended,
    /// or an unremarkable partial result.
    fn consent_signal(
        result: *mut SFSpeechRecognitionResult,
        error: *mut NSError,
    ) -> Option<ConsentSignal> {
        unsafe {
            if !error.is_null() {
                eprintln!("boss voice consent recognition failed: {:?}", *error);
                return Some(ConsentSignal::Ended);
            }
            if result.is_null() {
                return None;
            }
            let result = &*result;
            if super::hears_consent(&result.bestTranscription().formattedString().to_string()) {
                return Some(ConsentSignal::Heard);
            }
            result.isFinal().then_some(ConsentSignal::Ended)
        }
    }

    /// Read a CoreAudio property's raw bytes, or `None` on any failure.
    fn audio_property_data(object: AudioObjectID, selector: u32, scope: u32) -> Option<Vec<u8>> {
        let mut address = AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut size = 0u32;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                object,
                NonNull::from(&mut address),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
            )
        };
        if status != 0 || size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        let out = NonNull::new(data.as_mut_ptr() as *mut c_void)?;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&mut address),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                out,
            )
        };
        (status == 0).then_some(data)
    }

    /// Read a CoreAudio property reported as a `u32`.
    fn audio_property_u32(object: AudioObjectID, selector: u32, scope: u32) -> Option<u32> {
        let data = audio_property_data(object, selector, scope)?;
        Some(u32::from_ne_bytes(data.first_chunk::<4>()?.to_owned()))
    }

    /// Read a CoreAudio property reported as a CFString. The string arrives
    /// retained — CFString toll-free bridges to NSString, so owning it as
    /// one releases correctly.
    fn audio_property_string(object: AudioObjectID, selector: u32) -> Option<String> {
        let data = audio_property_data(object, selector, kAudioObjectPropertyScopeGlobal)?;
        let raw = usize::from_ne_bytes(data.first_chunk::<8>()?.to_owned());
        let string = unsafe { Retained::<NSString>::from_raw(raw as *mut NSString) }?;
        Some(string.to_string())
    }

    /// Every audio device the HAL reports, as `(id, uid, name)` — filtered
    /// to devices with at least one live input channel.
    pub fn audio_input_devices() -> Vec<(AudioObjectID, String, String)> {
        let Some(ids) = audio_property_data(
            kAudioObjectSystemObject as AudioObjectID,
            kAudioHardwarePropertyDevices,
            kAudioObjectPropertyScopeGlobal,
        ) else {
            return Vec::new();
        };
        ids.chunks_exact(4)
            .map(|chunk| u32::from_ne_bytes(chunk.try_into().unwrap()))
            .filter(|device| {
                // The stream-configuration buffer list: a u32 buffer count
                // followed by 16-byte AudioBuffer entries — an input device
                // has at least one buffer with channels.
                let Some(config) = audio_property_data(
                    *device,
                    kAudioDevicePropertyStreamConfiguration,
                    kAudioObjectPropertyScopeInput,
                ) else {
                    return false;
                };
                let buffers =
                    u32::from_ne_bytes(config[0..4].try_into().unwrap_or([0; 4])) as usize;
                (0..buffers).any(|index| {
                    config
                        .get(8 + index * 16..)
                        .and_then(|rest| rest.first_chunk::<4>())
                        .is_some_and(|channels| u32::from_ne_bytes(*channels) > 0)
                })
            })
            .map(|device| {
                let uid = audio_property_string(device, kAudioDevicePropertyDeviceUID)
                    .unwrap_or_default();
                let name =
                    audio_property_string(device, kAudioObjectPropertyName).unwrap_or_default();
                (device, uid, name)
            })
            .collect()
    }

    /// The AudioDeviceID behind a device UID, when the device is attached.
    fn resolve_device_uid(uid: &str) -> Option<AudioObjectID> {
        audio_input_devices()
            .into_iter()
            .find(|(_, device_uid, _)| device_uid == uid)
            .map(|(id, _, _)| id)
    }

    /// The device the engine should bind to right now: the pinned device,
    /// or the current default input when nothing is pinned. `None` means
    /// the wanted device is gone (or, unpinned, that no default exists).
    fn current_target_device() -> Option<AudioObjectID> {
        match PREFERRED_INPUT_UID.lock().unwrap().as_deref() {
            Some(uid) => resolve_device_uid(uid),
            None => audio_property_u32(
                kAudioObjectSystemObject as AudioObjectID,
                kAudioHardwarePropertyDefaultInputDevice,
                kAudioObjectPropertyScopeGlobal,
            )
            .filter(|id| *id != 0),
        }
    }

    /// Whether the wanted input can capture at all — the pinned device is
    /// attached, or some input device exists for the default to pick.
    fn input_present() -> bool {
        if PREFERRED_INPUT_UID.lock().unwrap().is_some() {
            current_target_device().is_some()
        } else {
            !audio_input_devices().is_empty()
        }
    }

    /// Arm the tap on a fresh engine: pin the chosen input device onto the
    /// input node's audio unit (a pinned mic never follows the default), then
    /// install the buffer tap and start. Returns false when the wanted
    /// device can't be bound or the engine refuses to start.
    fn configure_and_start(engine: &AVAudioEngine) -> bool {
        let input = unsafe { engine.inputNode() };
        let target = current_target_device();
        if PREFERRED_INPUT_UID.lock().unwrap().is_some() {
            let Some(device) = target else {
                return false;
            };
            let unit = unsafe { input.audioUnit() };
            if unit.is_null() {
                return false;
            }
            let status = unsafe {
                AudioUnitSetProperty(
                    unit,
                    kAudioOutputUnitProperty_CurrentDevice,
                    kAudioUnitScope_Global,
                    0,
                    &device as *const AudioObjectID as *const c_void,
                    std::mem::size_of::<AudioObjectID>() as u32,
                )
            };
            if status != 0 {
                return false;
            }
        }
        let tap = RcBlock::new(
            |buffer: NonNull<AVAudioPCMBuffer>, _time: NonNull<AVAudioTime>| {
                let buffer = unsafe { buffer.as_ref() };
                capture_dictation(buffer);
                if let Ok(feed) = CONSENT_FEED.try_lock()
                    && let Some(request) = feed.0.as_ref()
                {
                    unsafe { request.appendAudioPCMBuffer(buffer) };
                }
                observe_ambient_level(buffer);
                if let Ok(sink) = AUDIO_SINK.try_lock()
                    && let Some(sink) = sink.as_ref()
                    && let Some((mono, rate)) = mono_samples(buffer)
                {
                    sink(&mono, rate);
                }
            },
        );
        let tap_pointer = &*tap as *const _ as *mut _;
        unsafe { input.installTapOnBus_bufferSize_format_block(0, 4_800, None, tap_pointer) };
        if unsafe { engine.startAndReturnError() }.is_err() {
            unsafe { input.removeTapOnBus(0) };
            return false;
        }
        *BOUND_DEVICE.lock().unwrap() = target;
        true
    }

    /// Tear down the engine only — consent state is the caller's call.
    fn teardown_engine(listener: &VoiceListener) {
        let input = unsafe { listener.engine.inputNode() };
        unsafe {
            input.removeTapOnBus(0);
            listener.engine.stop();
        }
    }

    /// Register the CoreAudio device-list listener once — its block fires
    /// on a HAL-owned thread and only forwards into the app's hook, which
    /// lands the real work on the event pump.
    fn install_device_listener() {
        if DEVICE_LISTENER_INSTALLED.swap(true, Ordering::Relaxed) {
            return;
        }
        let listener = RcBlock::new(
            |_count: u32, _addresses: NonNull<AudioObjectPropertyAddress>| {
                if let Ok(hook) = DEVICE_CHANGE_HOOK.lock()
                    && let Some(hook) = hook.as_ref()
                {
                    hook();
                }
            },
        );
        let block = &*listener as *const _ as *mut _;
        // The device list covers plugs and unplugs; the default-input
        // property covers macOS retargeting the default without one
        // (System Settings picks, Bluetooth routing).
        for selector in [
            kAudioHardwarePropertyDevices,
            kAudioHardwarePropertyDefaultInputDevice,
        ] {
            let mut address = AudioObjectPropertyAddress {
                mSelector: selector,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };
            unsafe {
                AudioObjectAddPropertyListenerBlock(
                    kAudioObjectSystemObject as AudioObjectID,
                    NonNull::from(&mut address),
                    None,
                    block,
                );
            }
        }
        // The HAL copies the block into its own dispatch list, so the local
        // handle can drop — but keeping it makes the lifetime obvious.
        std::mem::forget(listener);
    }

    /// Start the capture engine and its tap. Audio is only inspected for
    /// short-term energy and forwarded to any live consent request — samples
    /// are never retained or sent anywhere. Returns false when the engine
    /// cannot start (including when a pinned mic is absent); callers keep
    /// fail-open playback in that case, and `devices_changed` rebinds when
    /// the device returns.
    pub fn start() -> bool {
        install_device_listener();
        ENGINE_WANTED.store(true, Ordering::Relaxed);
        VOICE_LISTENER.with_borrow_mut(|slot| {
            if slot.is_some() {
                return true;
            }
            if !input_present() {
                INPUT_AVAILABLE.store(false, Ordering::Relaxed);
                return false;
            }
            let engine = unsafe { AVAudioEngine::new() };
            if !configure_and_start(&engine) {
                return false;
            }
            INPUT_AVAILABLE.store(true, Ordering::Relaxed);
            *slot = Some(VoiceListener {
                engine,
                consent: None,
            });
            true
        })
    }

    /// Choose the input device the engine binds to. `None` follows the
    /// system default. Takes effect immediately — a live engine rebuilds
    /// onto the new selection.
    pub fn set_preferred_device(uid: Option<String>) {
        install_device_listener();
        *PREFERRED_INPUT_UID.lock().unwrap() = uid.filter(|uid| !uid.is_empty());
        let _ = devices_changed();
    }

    /// Register the hook called whenever the system's device set changes.
    /// Fires off the main thread — it must only forward, never rebuild.
    pub fn set_device_change_hook(hook: Option<Box<dyn Fn() + Send + Sync + 'static>>) {
        install_device_listener();
        *DEVICE_CHANGE_HOOK.lock().unwrap() = hook;
    }

    /// Whether the wanted input device can capture right now.
    pub fn input_available() -> bool {
        INPUT_AVAILABLE.load(Ordering::Relaxed)
    }

    /// Re-evaluate the device set: stop the engine when its input vanished,
    /// (re)build it when the wanted device is back or the default moved.
    /// Call on the thread that owns `VOICE_LISTENER`; returns the input's
    /// current availability.
    pub fn devices_changed() -> bool {
        let available = input_present();
        INPUT_AVAILABLE.store(available, Ordering::Relaxed);
        let target = current_target_device();
        VOICE_LISTENER.with_borrow_mut(|slot| {
            let mut consent = None;
            if let Some(mut listener) = slot.take() {
                let bound = *BOUND_DEVICE.lock().unwrap();
                let engine_dead = !unsafe { listener.engine.isRunning() };
                if available && !engine_dead && bound == target {
                    // Nothing the engine cares about moved — put it back
                    // rather than interrupting live capture.
                    *slot = Some(listener);
                    return;
                }
                // The consent session and its feed outlive the engine —
                // the rebuilt tap keeps appending to the same request.
                consent = listener.consent.take();
                teardown_engine(&listener);
                if !available {
                    return;
                }
            }
            if !available || !ENGINE_WANTED.load(Ordering::Relaxed) {
                return;
            }
            let engine = unsafe { AVAudioEngine::new() };
            if configure_and_start(&engine) {
                *slot = Some(VoiceListener { engine, consent });
            }
        });
        available
    }

    fn capture_dictation(buffer: &AVAudioPCMBuffer) {
        let frames = unsafe { buffer.frameLength() } as usize;
        let channel_data = unsafe { buffer.floatChannelData() };
        if frames == 0 || channel_data.is_null() {
            return;
        }
        let format = unsafe { buffer.format() };
        let rate = unsafe { format.sampleRate() }.round().max(1.0) as u32;
        let channels = unsafe { format.channelCount() }.max(1) as usize;
        let mut capture = match DICTATION_CAPTURE.try_lock() {
            Ok(capture) => capture,
            Err(_) => return,
        };
        let Some((sample_rate, samples)) = capture.as_mut() else {
            return;
        };
        *sample_rate = rate;
        let available = ((*sample_rate as usize).saturating_mul(30)).saturating_sub(samples.len());
        let count = frames.min(available);
        if count == 0 {
            return;
        }
        for frame in 0..count {
            let mono = (0..channels)
                .map(|channel| unsafe { *(*channel_data.add(channel)).as_ptr().add(frame) })
                .sum::<f32>()
                / channels as f32;
            samples.push(mono.clamp(-1.0, 1.0));
        }
    }

    pub fn begin_dictation() -> bool {
        if !start() {
            return false;
        }
        let Ok(mut capture) = DICTATION_CAPTURE.lock() else {
            return false;
        };
        *capture = Some((48_000, Vec::with_capacity(48_000 * 8)));
        true
    }

    pub fn finish_dictation() -> Option<Vec<i16>> {
        let (rate, samples) = DICTATION_CAPTURE.lock().ok()?.take()?;
        if samples.is_empty() {
            return None;
        }
        let count = samples.len().saturating_mul(16_000) / rate as usize;
        if count == 0 {
            return None;
        }
        Some(
            (0..count)
                .map(|index| {
                    let source = index as f64 * rate as f64 / 16_000.0;
                    let left = (source.floor() as usize).min(samples.len() - 1);
                    let right = (left + 1).min(samples.len() - 1);
                    let fraction = (source - left as f64) as f32;
                    let sample = samples[left] * (1.0 - fraction) + samples[right] * fraction;
                    (sample * i16::MAX as f32).round() as i16
                })
                .collect(),
        )
    }

    /// Stop the engine and any consent session, and reset the VAD window.
    pub fn stop() {
        ENGINE_WANTED.store(false, Ordering::Relaxed);
        end_consent();
        VOICE_LISTENER.with_borrow_mut(|slot| {
            if let Some(listener) = slot.take() {
                teardown_engine(&listener);
            }
        });
        *BOUND_DEVICE.lock().unwrap() = None;
        AMBIENT_WINDOWS.store(0, Ordering::Relaxed);
    }

    /// Whether the detector has recently seen sustained mic energy. A
    /// missing or silent detector reads as inactive so playback stays
    /// fail-open.
    pub fn ambient_active() -> bool {
        AMBIENT_WINDOWS.load(Ordering::Relaxed) > 0
    }

    /// Whether a consent session is currently live — the sole reason to
    /// keep the engine running when nothing is playing.
    pub fn consent_active() -> bool {
        VOICE_LISTENER.with_borrow(|slot| {
            slot.as_ref()
                .is_some_and(|listener| listener.consent.is_some())
        })
    }

    /// Begin listening for the spoken consent phrase on-device. `hook` fires
    /// on the recognizer's own queue, so it must be cheap and thread-safe —
    /// the app forwards it into its event pump. Requires the engine and the
    /// speech-recognition grant already in place; returns false otherwise.
    pub fn begin_consent(hook: Box<dyn Fn(ConsentSignal) + Send + Sync + 'static>) -> bool {
        VOICE_LISTENER.with_borrow_mut(|slot| {
            let Some(listener) = slot.as_mut() else {
                return false;
            };
            if listener.consent.is_some() {
                *CONSENT_HOOK.lock().unwrap() = Some(hook);
                return true;
            }
            use objc2::AnyThread;
            let Some(recognizer) =
                (unsafe { SFSpeechRecognizer::init(SFSpeechRecognizer::alloc()) })
            else {
                return false;
            };
            if !(unsafe { recognizer.isAvailable() }
                && unsafe { recognizer.supportsOnDeviceRecognition() })
            {
                return false;
            }
            let request = unsafe { SFSpeechAudioBufferRecognitionRequest::new() };
            unsafe {
                request.setRequiresOnDeviceRecognition(true);
                request.setShouldReportPartialResults(true);
                request.setTaskHint(SFSpeechRecognitionTaskHint::Dictation);
                // Bias the language model toward the only phrase we accept.
                let phrases = [
                    NSString::from_str("go ahead"),
                    NSString::from_str("go-ahead"),
                ];
                let phrase_refs: Vec<&NSString> = phrases.iter().map(|p| &**p).collect();
                request.setContextualStrings(&NSArray::from_slice(&phrase_refs));
            }
            let handler = RcBlock::new(
                |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
                    if CONSENT_HEARD.load(Ordering::Relaxed) {
                        return;
                    }
                    // A cleared hook means the session is being torn down —
                    // the events `endAudio`/`cancel` fire are not real ends.
                    let Ok(hook) = CONSENT_HOOK.lock() else {
                        return;
                    };
                    let Some(hook) = hook.as_ref() else {
                        return;
                    };
                    let Some(signal) = consent_signal(result, error) else {
                        return;
                    };
                    if signal == ConsentSignal::Heard {
                        CONSENT_HEARD.store(true, Ordering::Relaxed);
                    }
                    hook(signal);
                },
            );
            let task =
                unsafe { recognizer.recognitionTaskWithRequest_resultHandler(&request, &handler) };
            CONSENT_HEARD.store(false, Ordering::Relaxed);
            *CONSENT_HOOK.lock().unwrap() = Some(hook);
            CONSENT_FEED.lock().unwrap().0 = Some(request.clone());
            listener.consent = Some(ConsentSession {
                request,
                task,
                _recognizer: recognizer,
            });
            true
        })
    }

    /// Hand captured mic audio to the dictation scratchpad: `sink` gets mono
    /// f32 samples plus the buffer's rate on the audio thread, so it must be
    /// cheap and never block — the app copies into a bounded channel and
    /// drops blocks when it's full. `None` detaches the sink.
    pub fn set_audio_sink(sink: Option<AudioSink>) {
        *AUDIO_SINK.lock().unwrap() = sink;
    }

    /// Whether a dictation sink is registered — the engine should stay up
    /// for it even while nothing else needs the mic.
    pub fn audio_sink_active() -> bool {
        AUDIO_SINK.lock().unwrap().is_some()
    }

    /// Whether a composer dictation capture is holding PCM — another
    /// consumer the engine should stay up for.
    pub fn dictation_active() -> bool {
        DICTATION_CAPTURE.lock().unwrap().is_some()
    }

    /// End the consent session but leave the engine running — playback still
    /// wants the ambient check. Feed and hook clear before `endAudio`/`cancel`
    /// so the handler they synchronously fire sees nothing to report.
    pub fn end_consent() {
        let session = VOICE_LISTENER
            .with_borrow_mut(|slot| slot.as_mut().and_then(|listener| listener.consent.take()));
        CONSENT_FEED.lock().unwrap().0 = None;
        *CONSENT_HOOK.lock().unwrap() = None;
        CONSENT_HEARD.store(false, Ordering::Relaxed);
        if let Some(session) = session {
            unsafe {
                session.request.endAudio();
                session.task.cancel();
            }
        }
    }
}

/// Start the local voice listener (ambient detection plus whatever consent
/// session is active). Safe to call repeatedly — the engine starts once.
#[cfg(target_os = "macos")]
pub fn start_voice_listener() -> bool {
    if !in_app_bundle() {
        return false;
    }
    voice_gate::start()
}

/// Stop the voice listener entirely — engine, tap, and consent session.
#[cfg(target_os = "macos")]
pub fn stop_voice_listener() {
    voice_gate::stop()
}

/// Whether the on-device detector has recently seen sustained mic energy.
/// A missing detector is treated as inactive so playback remains available.
#[cfg(target_os = "macos")]
pub fn ambient_speech_active() -> bool {
    voice_gate::ambient_active()
}

/// Whether "go ahead" recognition is live — the engine should stay up for
/// it even while nothing plays.
#[cfg(target_os = "macos")]
pub fn consent_recognition_active() -> bool {
    voice_gate::consent_active()
}

/// An audio input device the voice listener can be pinned to.
#[cfg(target_os = "macos")]
pub struct VoiceInputDevice {
    /// The CoreAudio device UID — stable across reconnects and reboots.
    pub uid: String,
    /// The device's display name.
    pub name: String,
}

/// The system's current audio input devices — the mic picker's options.
#[cfg(target_os = "macos")]
pub fn voice_input_devices() -> Vec<VoiceInputDevice> {
    voice_gate::audio_input_devices()
        .into_iter()
        .map(|(_, uid, name)| VoiceInputDevice { uid, name })
        .collect()
}

/// Pin the voice listener to one input device UID — `None` (or empty)
/// follows the system default. The listener binds to the specific device,
/// never silently to another mic; if it disappears the engine stops and
/// rebinds when it returns.
#[cfg(target_os = "macos")]
pub fn set_voice_input_device(uid: Option<String>) {
    voice_gate::set_preferred_device(uid)
}

/// Register the hook that fires (off the main thread) whenever the system's
/// audio device set changes. `None` detaches it.
#[cfg(target_os = "macos")]
pub fn set_voice_input_change_hook(hook: Option<Box<dyn Fn() + Send + Sync + 'static>>) {
    voice_gate::set_device_change_hook(hook)
}

/// Re-evaluate the device set after a reported change — stops or rebuilds
/// the engine as needed. Must run on the thread that owns the listener
/// (the main thread); returns whether the wanted input can capture.
#[cfg(target_os = "macos")]
pub fn voice_input_devices_changed() -> bool {
    voice_gate::devices_changed()
}

/// Whether the wanted input device can capture right now.
#[cfg(target_os = "macos")]
pub fn voice_input_available() -> bool {
    voice_gate::input_available()
}

/// Begin on-device recognition of the spoken consent phrase. `hook` is
/// invoked off the UI thread for each `ConsentSignal`.
#[cfg(target_os = "macos")]
pub fn begin_consent_recognition(hook: Box<dyn Fn(ConsentSignal) + Send + Sync + 'static>) -> bool {
    voice_gate::begin_consent(hook)
}

/// End consent recognition without stopping ambient detection.
#[cfg(target_os = "macos")]
pub fn end_consent_recognition() {
    voice_gate::end_consent()
}

/// Register the dictation scratchpad's consumer for live mic audio. Unlike
/// the on-device consent feed, samples handed to this sink leave the
/// process — they stream to the transcription gateway — so it only exists
/// while a scratchpad session is live. `None` detaches it.
#[cfg(target_os = "macos")]
pub fn set_voice_audio_sink(sink: Option<Box<dyn Fn(&[f32], f64) + Send + Sync + 'static>>) {
    voice_gate::set_audio_sink(sink)
}

/// Whether the dictation sink is attached — keeps the mic engine alive.
#[cfg(target_os = "macos")]
pub fn voice_audio_sink_active() -> bool {
    voice_gate::audio_sink_active()
}

/// Whether a composer dictation capture is mid-record — keeps the mic
/// engine alive until it finishes.
#[cfg(target_os = "macos")]
pub fn dictation_capture_active() -> bool {
    voice_gate::dictation_active()
}

#[cfg(not(target_os = "macos"))]
pub fn set_voice_audio_sink(_: Option<Box<dyn Fn(&[f32], f64) + Send + Sync + 'static>>) {}

#[cfg(not(target_os = "macos"))]
pub fn voice_audio_sink_active() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn dictation_capture_active() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub struct VoiceInputDevice {
    pub uid: String,
    pub name: String,
}

#[cfg(not(target_os = "macos"))]
pub fn voice_input_devices() -> Vec<VoiceInputDevice> {
    Vec::new()
}

#[cfg(not(target_os = "macos"))]
pub fn set_voice_input_device(_: Option<String>) {}

#[cfg(not(target_os = "macos"))]
pub fn set_voice_input_change_hook(_: Option<Box<dyn Fn() + Send + Sync + 'static>>) {}

#[cfg(not(target_os = "macos"))]
pub fn voice_input_devices_changed() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn voice_input_available() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn begin_dictation_capture() -> bool {
    voice_gate::begin_dictation()
}

#[cfg(target_os = "macos")]
pub fn finish_dictation_capture() -> Option<Vec<i16>> {
    voice_gate::finish_dictation()
}

#[cfg(not(target_os = "macos"))]
pub fn microphone_access() -> CaptureAccess {
    CaptureAccess::Denied
}

#[cfg(not(target_os = "macos"))]
pub fn request_microphone_access(done: Box<dyn Fn(bool) + Send + Sync + 'static>) {
    done(false);
}

#[cfg(not(target_os = "macos"))]
pub fn speech_recognition_access() -> CaptureAccess {
    CaptureAccess::Denied
}

#[cfg(not(target_os = "macos"))]
pub fn request_speech_recognition_access(done: Box<dyn Fn(bool) + Send + Sync + 'static>) {
    done(false);
}

#[cfg(not(target_os = "macos"))]
pub fn start_voice_listener() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn begin_dictation_capture() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn finish_dictation_capture() -> Option<Vec<i16>> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn stop_voice_listener() {}

#[cfg(not(target_os = "macos"))]
pub fn ambient_speech_active() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn consent_recognition_active() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn begin_consent_recognition(_: Box<dyn Fn(ConsentSignal) + Send + Sync + 'static>) -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn end_consent_recognition() {}

#[cfg(target_os = "macos")]
pub fn show_about_panel() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(main_thread).orderFrontStandardAboutPanel(None);
}

#[cfg(not(target_os = "macos"))]
pub fn show_about_panel() {}

/// Register embedded font data with CoreText at process scope. GPUI's
/// `add_fonts` only feeds its private font-kit source, which CoreText cascade
/// matching cannot see — and it refuses symbols-only faces outright (fonts
/// with no 'm' glyph). Fonts referenced through `FontFallbacks` therefore
/// must be registered here instead.
#[cfg(target_os = "macos")]
pub fn register_fonts_with_coretext(fonts: &[&'static [u8]]) -> anyhow::Result<()> {
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGDataProviderCreateWithData(
            info: *mut c_void,
            data: *const u8,
            size: usize,
            release_callback: *const c_void,
        ) -> *mut c_void;
        fn CGFontCreateWithDataProvider(provider: *mut c_void) -> *mut c_void;
        fn CGDataProviderRelease(provider: *mut c_void);
        fn CGFontRelease(font: *mut c_void);
    }
    #[link(name = "CoreText", kind = "framework")]
    unsafe extern "C" {
        fn CTFontManagerRegisterGraphicsFont(font: *mut c_void, error: *mut *mut c_void) -> bool;
    }

    for (index, data) in fonts.iter().enumerate() {
        unsafe {
            let provider = CGDataProviderCreateWithData(
                std::ptr::null_mut(),
                data.as_ptr(),
                data.len(),
                std::ptr::null(),
            );
            anyhow::ensure!(!provider.is_null(), "font {index}: not a readable buffer");
            let font = CGFontCreateWithDataProvider(provider);
            CGDataProviderRelease(provider);
            anyhow::ensure!(!font.is_null(), "font {index}: not a valid font");
            let registered = CTFontManagerRegisterGraphicsFont(font, std::ptr::null_mut());
            CGFontRelease(font);
            anyhow::ensure!(registered, "font {index}: CoreText registration failed");
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn register_fonts_with_coretext(_: &[&'static [u8]]) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn init_reduce_motion(cx: &mut gpui::App) {
    use objc2_app_kit::NSWorkspace;

    cx.set_reduce_motion(NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion());
}

#[cfg(target_os = "linux")]
pub fn init_reduce_motion(cx: &mut gpui::App) {
    if let Ok(value) = std::env::var("GODDARD_REDUCE_MOTION")
        && let Some(enabled) = parse_boolean_setting(&value)
    {
        cx.set_reduce_motion(enabled);
        return;
    }

    // GNOME exposes its animation preference through GSettings. Resolve it
    // once off the UI thread; frames only read GPUI's in-memory flag.
    cx.spawn(async move |cx| {
        let enabled = cx
            .background_executor()
            .spawn(async move { linux_reduce_motion_enabled() })
            .await;
        cx.update(|cx| cx.set_reduce_motion(enabled));
    })
    .detach();
}

#[cfg(target_os = "linux")]
fn linux_reduce_motion_enabled() -> bool {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| parse_boolean_setting(&value))
        .is_some_and(|animations_enabled| !animations_enabled)
}

/// Ease of Access → "Show animations in Windows" clears
/// `SPI_GETCLIENTAREAANIMATION`. GPUI has no Windows implementation of its
/// own, and the call only reads a cached user setting, so startup can ask
/// directly.
#[cfg(target_os = "windows")]
pub fn init_reduce_motion(cx: &mut gpui::App) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };

    let mut animations_enabled: i32 = 1;
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            std::ptr::from_mut(&mut animations_enabled).cast(),
            0,
        )
    };
    if read != 0 {
        cx.set_reduce_motion(animations_enabled == 0);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn init_reduce_motion(_: &mut gpui::App) {}

/// With "Reduce transparency" on, macOS drops all vibrancy — the Sidebar
/// material degrades to a flat tint that fakes a blur, so callers should
/// treat sidebar transparency as off and paint the solid fill instead.
#[cfg(target_os = "macos")]
pub fn reduce_transparency() -> bool {
    use objc2_app_kit::NSWorkspace;

    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
}

#[cfg(not(target_os = "macos"))]
pub fn reduce_transparency() -> bool {
    false
}

/// The OS "increase contrast" accessibility preference. Read when a theme is
/// built — palette construction is rare, so no caching is needed.
#[cfg(target_os = "macos")]
pub fn increase_contrast() -> bool {
    use objc2_app_kit::NSWorkspace;

    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast()
}

/// GNOME's High Contrast is a theme, not a flag — honor an explicit override
/// and leave other desktops to their own settings until one is wired up.
#[cfg(target_os = "linux")]
pub fn increase_contrast() -> bool {
    std::env::var("GODDARD_INCREASE_CONTRAST")
        .ok()
        .and_then(|value| parse_boolean_setting(&value))
        .unwrap_or(false)
}

/// SystemParametersInfo's high-contrast query reflects Ease of Access →
/// Contrast themes. The call only reads a cached user setting, so it is safe
/// to ask directly at theme-build time.
#[cfg(target_os = "windows")]
pub fn increase_contrast() -> bool {
    use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW};

    let mut hc = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        dwFlags: 0,
        lpszDefaultScheme: std::ptr::null_mut(),
    };
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            0,
            std::ptr::from_mut(&mut hc).cast(),
            0,
        )
    };
    read != 0 && hc.dwFlags & HCF_HIGHCONTRASTON != 0
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn increase_contrast() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn parse_boolean_setting(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Deliver an audible macOS notification. GPUI owns the notification-center
/// delegate (and therefore click responses); Goddard only supplies content here
/// because GPUI's generic payload does not currently expose a sound field.
#[cfg(target_os = "macos")]
pub fn show_task_notification(tag: &str, title: &str, body: &str, _: &gpui::App) {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNNotificationSound, UNUserNotificationCenter,
    };

    // UserNotifications raises an Objective-C exception for an executable
    // outside an application bundle, including unit tests and `cargo run`.
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }

    let tag = tag.to_owned();
    let title = title.to_owned();
    let body = body.to_owned();
    let authorization = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if !granted.as_bool() {
            return;
        }

        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str(&body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));

        // A nil trigger delivers immediately. The stable task tag replaces an
        // older completion banner for the same task and comes back on click.
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&tag),
            &content,
            None,
        );
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, None);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &authorization,
        );
}

#[cfg(not(target_os = "macos"))]
pub fn show_task_notification(tag: &str, title: &str, body: &str, cx: &gpui::App) {
    cx.show_system_notification(gpui::SystemNotification {
        tag: tag.to_owned().into(),
        title: title.to_owned().into(),
        body: body.to_owned().into(),
        actions: Vec::new(),
    });
}

#[cfg(target_os = "macos")]
thread_local! {
    /// `AVAudioPlayer` stops when deallocated, so the playing instance is
    /// retained until the next play replaces it. Playback outlives this only
    /// by the sound's own sub-second length.
    static PLAYING_COMPLETION_SOUND:
        std::cell::RefCell<Option<objc2::rc::Retained<objc2_avf_audio::AVAudioPlayer>>> =
        const { std::cell::RefCell::new(None) };
    /// Briefings run ~45 seconds, so they get their own slot — a new one
    /// replacing the slot stops the old clip rather than mixing over it.
    static PLAYING_BRIEFING:
        std::cell::RefCell<Option<objc2::rc::Retained<objc2_avf_audio::AVAudioPlayer>>> =
        const { std::cell::RefCell::new(None) };
}

/// The bundled sounds' embedded MP3 payloads.
#[cfg(target_os = "macos")]
fn completion_sound_data(sound: waku_client::persistence::CompletionSound) -> &'static [u8] {
    use waku_client::persistence::CompletionSound;

    match sound {
        CompletionSound::Bleep => include_bytes!("../assets/sounds/bleep.mp3").as_slice(),
        CompletionSound::Gentle => include_bytes!("../assets/sounds/gentle.mp3").as_slice(),
        CompletionSound::Bubble => include_bytes!("../assets/sounds/bubble.mp3").as_slice(),
        CompletionSound::Chime => include_bytes!("../assets/sounds/chime.mp3").as_slice(),
        CompletionSound::Retro => include_bytes!("../assets/sounds/retro.mp3").as_slice(),
        CompletionSound::Crystal => include_bytes!("../assets/sounds/crystal.mp3").as_slice(),
    }
}

/// Per-sound loudness compensation, multiplied with the user's volume so the
/// bundled set lands at a comparable level. Retro and Crystal run hot.
#[cfg(target_os = "macos")]
fn completion_sound_gain(sound: waku_client::persistence::CompletionSound) -> f32 {
    match sound {
        waku_client::persistence::CompletionSound::Retro
        | waku_client::persistence::CompletionSound::Crystal => 0.5,
        _ => 1.0,
    }
}

/// Play one of the bundled turn-completion sounds at `volume` relative to its
/// recorded level — 1.0 plays it as bundled and the slider allows up to 2.0.
/// `AVAudioPlayer` decodes the embedded MP3 itself and its volume is a linear
/// gain that can boost past 1.0, where `NSSound` clamps; `play` returns
/// immediately. There is no smaller portable API, so other platforms stay
/// silent for now.
#[cfg(target_os = "macos")]
pub fn play_completion_sound(sound: waku_client::persistence::CompletionSound, volume: f32) {
    use objc2::AnyThread;
    use objc2_avf_audio::AVAudioPlayer;
    use objc2_foundation::NSData;

    let volume = (volume * completion_sound_gain(sound))
        .clamp(0.0, waku_client::persistence::MAX_COMPLETION_SOUND_VOLUME);
    let data = NSData::with_bytes(completion_sound_data(sound));
    let Ok(player) = (unsafe { AVAudioPlayer::initWithData_error(AVAudioPlayer::alloc(), &data) })
    else {
        return;
    };
    unsafe {
        player.setVolume(volume);
        if player.play() {
            PLAYING_COMPLETION_SOUND.with_borrow_mut(|slot| *slot = Some(player));
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn play_completion_sound(_: waku_client::persistence::CompletionSound, _: f32) {}

/// Play a voice-briefing clip — encoded audio bytes in any container
/// `AVAudioPlayer` decodes (the gateway returns WAV or MP3 depending on the
/// model). Reuses the completion-sound volume so the experiment honors the
/// existing slider.
/// Returns the clip's duration when playback starts. Other platforms have no
/// playback implementation yet and return `None`.
#[cfg(target_os = "macos")]
pub fn play_briefing_audio(bytes: &[u8], volume: f32) -> Option<std::time::Duration> {
    use objc2::AnyThread;
    use objc2_avf_audio::AVAudioPlayer;
    use objc2_foundation::NSData;

    let volume = volume.clamp(0.0, waku_client::persistence::MAX_COMPLETION_SOUND_VOLUME);
    let data = NSData::with_bytes(bytes);
    let Ok(player) = (unsafe { AVAudioPlayer::initWithData_error(AVAudioPlayer::alloc(), &data) })
    else {
        return None;
    };
    let duration = std::time::Duration::try_from_secs_f64(unsafe { player.duration() }).ok()?;
    if duration.is_zero() {
        return None;
    }
    unsafe {
        player.setVolume(volume);
        if player.play() {
            PLAYING_BRIEFING.with_borrow_mut(|slot| *slot = Some(player));
            Some(duration)
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn play_briefing_audio(_: &[u8], _: f32) -> Option<std::time::Duration> {
    None
}

/// Apply a changed briefing volume to the currently loaded player.
#[cfg(target_os = "macos")]
pub fn set_briefing_audio_volume(volume: f32) {
    PLAYING_BRIEFING.with_borrow(|slot| {
        if let Some(player) = slot.as_ref() {
            unsafe {
                player.setVolume(waku_client::persistence::sanitized_completion_sound_volume(
                    volume,
                ))
            };
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn set_briefing_audio_volume(_: f32) {}

#[cfg(target_os = "macos")]
pub fn pause_briefing_audio() -> Option<std::time::Duration> {
    PLAYING_BRIEFING.with_borrow(|slot| {
        let player = slot.as_ref()?;
        unsafe { player.pause() };
        briefing_audio_status().map(|(_, remaining)| remaining)
    })
}

#[cfg(not(target_os = "macos"))]
pub fn pause_briefing_audio() -> Option<std::time::Duration> {
    None
}

/// Restart the current clip, including a paused clip, from its beginning.
#[cfg(target_os = "macos")]
pub fn restart_briefing_audio() -> Option<std::time::Duration> {
    PLAYING_BRIEFING.with_borrow(|slot| {
        let player = slot.as_ref()?;
        unsafe { player.setCurrentTime(0.0) };
        if !unsafe { player.play() } {
            return None;
        }
        briefing_audio_status().map(|(_, remaining)| remaining)
    })
}

#[cfg(not(target_os = "macos"))]
pub fn restart_briefing_audio() -> Option<std::time::Duration> {
    None
}

#[cfg(target_os = "macos")]
pub fn resume_briefing_audio() -> Option<std::time::Duration> {
    PLAYING_BRIEFING.with_borrow(|slot| {
        let player = slot.as_ref()?;
        if !unsafe { player.play() } {
            return None;
        }
        briefing_audio_status().map(|(_, remaining)| remaining)
    })
}

#[cfg(not(target_os = "macos"))]
pub fn resume_briefing_audio() -> Option<std::time::Duration> {
    None
}

#[cfg(target_os = "macos")]
pub fn briefing_audio_status() -> Option<(bool, std::time::Duration)> {
    PLAYING_BRIEFING.with_borrow(|slot| {
        let player = slot.as_ref()?;
        let (playing, current_time, duration) =
            unsafe { (player.isPlaying(), player.currentTime(), player.duration()) };
        let remaining =
            std::time::Duration::try_from_secs_f64((duration - current_time).max(0.0)).ok()?;
        Some((playing, remaining))
    })
}

#[cfg(not(target_os = "macos"))]
pub fn briefing_audio_status() -> Option<(bool, std::time::Duration)> {
    None
}

#[cfg(target_os = "macos")]
pub fn stop_briefing_audio() {
    PLAYING_BRIEFING.with_borrow_mut(|slot| *slot = None);
}

#[cfg(not(target_os = "macos"))]
pub fn stop_briefing_audio() {}

#[cfg(target_os = "macos")]
fn app_icon_for_application_path(
    application_path: &objc2_foundation::NSString,
) -> Option<std::sync::Arc<gpui::Image>> {
    use objc2::AnyThread;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSWorkspace};
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize};

    let image = NSWorkspace::sharedWorkspace().iconForFile(application_path);
    image.setSize(NSSize::new(32.0, 32.0));
    // Extract one small representation. `TIFFRepresentation` would serialize
    // the icon's entire rep stack — ~72 MB and hundreds of milliseconds per
    // app for a 1024px icon — and then hand GPUI a 1024px PNG to decode on
    // first paint. Proposing a 32pt rect selects the nearest small rep.
    let mut proposed = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(32.0, 32.0));
    let cg_image =
        unsafe { image.CGImageForProposedRect_context_hints(&mut proposed, None, None) }?;
    let bitmap_rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &cg_image);
    let properties = NSDictionary::new();
    let png_data = unsafe {
        bitmap_rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
    }?;
    let bytes = unsafe { png_data.as_bytes_unchecked() };
    (!bytes.is_empty()).then(|| {
        std::sync::Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            bytes.to_vec(),
        ))
    })
}

#[cfg(target_os = "macos")]
pub fn load_app_icon_for_bundle_id(bundle_id: &str) -> Option<std::sync::Arc<gpui::Image>> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    let bundle_id = NSString::from_str(bundle_id);
    let application_url =
        NSWorkspace::sharedWorkspace().URLForApplicationWithBundleIdentifier(&bundle_id)?;
    let application_path = application_url.path()?;
    app_icon_for_application_path(&application_path)
}

#[cfg(not(target_os = "macos"))]
pub fn load_app_icon_for_bundle_id(_: &str) -> Option<std::sync::Arc<gpui::Image>> {
    None
}

/// A folder-capable application the header's "open project in" control can
/// target, resolved against what is installed on this machine.
#[derive(Clone)]
pub struct ExternalApp {
    /// Stable identifier persisted as the user's preferred target.
    pub id: &'static str,
    pub label: &'static str,
    /// The bundle id that resolved here, for launching.
    pub bundle_id: &'static str,
    pub icon: std::sync::Arc<gpui::Image>,
}

impl ExternalApp {
    /// Reads as an editor for a single file — the catalog also lists the
    /// file manager and terminals, which only meaningfully open folders.
    pub fn is_editor(&self) -> bool {
        matches!(
            self.id,
            "vscode" | "cursor" | "zed" | "devin" | "xcode" | "android-studio"
        )
    }

    /// The `<scheme>://file/<path>:<line>` deep link this app registers, when
    /// it has one. Apps without one get a plain document open instead.
    fn file_line_scheme(&self) -> Option<&'static str> {
        match self.id {
            "vscode" => Some("vscode"),
            "cursor" => Some("cursor"),
            "zed" => Some("zed"),
            "devin" => Some("windsurf"),
            _ => None,
        }
    }
}

/// The `<scheme>://file/<path>:<line>` URL an editor deep link expects. The
/// `url` crate percent-encodes the path; `:` survives the path encode set,
/// which is what the editors split the line on.
fn editor_file_line_url(scheme: &str, path: &std::path::Path, line: u32) -> Option<String> {
    let mut url = url::Url::parse(&format!("{scheme}://file/")).ok()?;
    url.set_path(&format!("{}:{line}", path.to_string_lossy()));
    Some(url.into())
}

/// Open the file `path` in `app`, landing on `line` when the app takes a
/// line deep link and opening the document plainly when it does not. Every
/// route hands off to the OS asynchronously, so this is safe from any click
/// path.
pub fn open_file_in_app(
    path: &std::path::Path,
    line: Option<u32>,
    app: &ExternalApp,
    cx: &gpui::App,
) {
    if let (Some(line), Some(scheme)) = (line, app.file_line_scheme()) {
        if let Some(url) = editor_file_line_url(scheme, path, line) {
            cx.open_url(&url);
            return;
        }
    }
    open_path_in_app(path, app.bundle_id);
}

/// Known folder-capable apps in menu order — editors, the file manager,
/// terminals, IDEs. An entry lists every bundle id it ships under; the first
/// installed one wins.
#[cfg(target_os = "macos")]
const TERMY_BUNDLE_ID: &str = "com.lassevestergaard.termy";

#[cfg(target_os = "macos")]
const OPEN_IN_CATALOG: &[(&str, &str, &[&str])] = &[
    ("vscode", "VS Code", &["com.microsoft.VSCode"]),
    ("cursor", "Cursor", &["com.todesktop.230313mzl4w4u92"]),
    ("zed", "Zed", &["dev.zed.Zed", "dev.zed.Zed-Preview"]),
    ("devin", "Devin", &["com.exafunction.windsurf"]),
    ("finder", "Finder", &["com.apple.finder"]),
    ("terminal", "Terminal", &["com.apple.Terminal"]),
    ("termy", "Termy", &[TERMY_BUNDLE_ID]),
    ("iterm2", "iTerm2", &["com.googlecode.iterm2"]),
    ("kitty", "Kitty", &["net.kovidgoyal.kitty"]),
    ("ghostty", "Ghostty", &["com.mitchellh.ghostty"]),
    ("warp", "Warp", &["dev.warp.Warp-Stable", "dev.warp.Warp"]),
    ("xcode", "Xcode", &["com.apple.dt.Xcode"]),
    (
        "android-studio",
        "Android Studio",
        &["com.google.android.studio"],
    ),
];

/// Resolve which catalog apps are installed, with their icons.
#[cfg(target_os = "macos")]
pub fn detect_open_in_apps() -> Vec<ExternalApp> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    let workspace = NSWorkspace::sharedWorkspace();
    OPEN_IN_CATALOG
        .iter()
        .filter_map(|&(id, label, bundle_ids)| {
            bundle_ids.iter().find_map(|&bundle_id| {
                let application_url = workspace
                    .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
                let application_path = application_url.path()?;
                Some(ExternalApp {
                    id,
                    label,
                    bundle_id,
                    icon: app_icon_for_application_path(&application_path)?,
                })
            })
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub fn detect_open_in_apps() -> Vec<ExternalApp> {
    Vec::new()
}

/// Open `path` in the application `bundle_id`, activating it. Launch Services
/// delivers the open asynchronously, so this never blocks.
#[cfg(target_os = "macos")]
pub fn open_path_in_app(path: &std::path::Path, bundle_id: &str) {
    use objc2_app_kit::{NSWorkspace, NSWorkspaceOpenConfiguration};
    use objc2_foundation::{NSArray, NSString, NSURL};

    let workspace = NSWorkspace::sharedWorkspace();
    let Some(application_url) =
        workspace.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))
    else {
        return;
    };
    let url = if bundle_id == TERMY_BUNDLE_ID {
        // Termy rejects folder file URLs; its public new-tab route accepts the
        // working directory as an encoded query parameter instead.
        let Some(url) = NSURL::URLWithString(&NSString::from_str(&termy_open_url(path))) else {
            return;
        };
        url
    } else {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    };
    workspace.openURLs_withApplicationAtURL_configuration_completionHandler(
        &NSArray::from_retained_slice(&[url]),
        &application_url,
        &NSWorkspaceOpenConfiguration::configuration(),
        None,
    );
}

#[cfg(target_os = "macos")]
fn termy_open_url(path: &std::path::Path) -> String {
    let mut url = url::Url::parse("termy://new").expect("static Termy URL should be valid");
    url.query_pairs_mut()
        .append_pair("dir", &path.to_string_lossy());
    url.into()
}

#[cfg(not(target_os = "macos"))]
pub fn open_path_in_app(_: &std::path::Path, _: &str) {}

/// The default browser's bundle id plus the flag a fresh process of it reads
/// as "open a private window". Launch Services is asked for the https
/// handler; Safari and unknown handlers have no CLI private path, so callers
/// hide the item rather than open a plain window labeled private.
#[cfg(target_os = "macos")]
fn default_browser_private_mode() -> Option<(String, &'static str)> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSBundle, NSString, NSURL};

    let https = NSURL::URLWithString(&NSString::from_str("https://open.invalid"))?;
    let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&https)?;
    let bundle_id = NSBundle::bundleWithURL(&app)?
        .bundleIdentifier()?
        .to_string();
    let flag = match bundle_id.as_str() {
        "com.google.Chrome"
        | "com.google.Chrome.canary"
        | "org.chromium.Chromium"
        | "com.brave.Browser"
        | "com.vivaldi.Vivaldi"
        | "company.thebrowser.Browser" => "--incognito",
        "com.microsoft.edgemac" | "com.microsoft.edgemac.Dev" | "com.microsoft.edgemac.Canary" => {
            "--inprivate"
        }
        "org.mozilla.firefox" | "org.mozilla.firefoxdeveloperedition" | "org.mozilla.nightly" => {
            "--private-window"
        }
        _ => return None,
    };
    Some((bundle_id, flag))
}

/// Whether an "open in private window" menu item can do what it says — the
/// default browser is a build whose incognito flag we know.
#[cfg(target_os = "macos")]
pub fn can_open_url_in_private_window() -> bool {
    default_browser_private_mode().is_some()
}

#[cfg(not(target_os = "macos"))]
pub fn can_open_url_in_private_window() -> bool {
    false
}

/// Open `url` in a private window of the default browser. `open -n` spawns a
/// fresh process whose argv carries the incognito flag plus the URL — the
/// already-running browser performs the actual open, so this never blocks.
#[cfg(target_os = "macos")]
pub fn open_url_in_private_window(url: &str) {
    if let Some((bundle_id, flag)) = default_browser_private_mode() {
        let _ = std::process::Command::new("open")
            .arg("-n")
            .arg("-b")
            .arg(bundle_id)
            .arg("--args")
            .arg(flag)
            .arg(url)
            .spawn();
    }
}

#[cfg(not(target_os = "macos"))]
pub fn open_url_in_private_window(_: &str) {}

/// Select `path` in the platform file manager. GPUI dispatches Linux portal
/// and subprocess work away from the UI thread.
pub fn reveal_in_file_manager(path: &std::path::Path, cx: &gpui::App) {
    cx.reveal_path(path);
}

/// Open `path` with its default application — a document in its editor.
pub fn open_with_default_app(path: &std::path::Path, cx: &gpui::App) {
    cx.open_with_system(path);
}

/// Decode the embedded desktop icon once. X11 consumes the RGBA pixels from
/// `WindowOptions`; Wayland associates the window through `app_id` and its
/// installed desktop entry.
#[cfg(target_os = "linux")]
pub fn linux_app_icon() -> Option<std::sync::Arc<image::RgbaImage>> {
    static ICON: std::sync::LazyLock<Option<std::sync::Arc<image::RgbaImage>>> =
        std::sync::LazyLock::new(|| {
            image::load_from_memory(include_bytes!("../resources/linux/app-icon.png"))
                .ok()
                .map(|image| std::sync::Arc::new(image.into_rgba8()))
        });
    ICON.clone()
}

/// A compact shortcut label for the platform's primary GUI modifier.
pub const fn primary_shortcut<'a>(macos: &'a str, other: &'a str) -> &'a str {
    if cfg!(target_os = "macos") {
        macos
    } else {
        other
    }
}

#[cfg(target_os = "macos")]
pub fn hide_window(window: &mut Window) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let Some(_main_thread) = MainThreadMarker::new() else {
        return;
    };

    // GPUI owns this view and its NSWindow. AppKit access stays on the main
    // thread, and orderOut hides without triggering GPUI's close callback.
    unsafe {
        let view = handle.ns_view.cast::<NSView>().as_ref();
        if let Some(native_window) = view.window() {
            native_window.orderOut(None);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn hide_window(window: &mut Window) {
    window.remove_window();
}

#[cfg(target_os = "macos")]
thread_local! {
    static SIDEBAR_GLASS_VIEW: std::cell::RefCell<
        Option<objc2::rc::Retained<objc2_app_kit::NSGlassEffectView>>,
    > = const { std::cell::RefCell::new(None) };
}

/// NSGlassEffectView ships with macOS 26; the class lookup is the availability
/// check, so older systems keep the vibrancy + tint path untouched.
#[cfg(target_os = "macos")]
fn glass_effect_supported() -> bool {
    objc2::runtime::AnyClass::get(c"NSGlassEffectView").is_some()
}

#[cfg(target_os = "macos")]
const SIDEBAR_WIDTH: f64 = 252.0;

pub fn start_window_move(window: &Window) {
    window.start_window_move();
}

/// Perform the platform's titlebar double-click action. GPUI delegates this
/// to the user's system preference on macOS, while Linux client decorations
/// must toggle maximize explicitly.
pub fn titlebar_double_click(window: &Window) {
    #[cfg(target_os = "macos")]
    window.titlebar_double_click();

    // Windows performs the user's configured caption double-click action in
    // `DefWindowProc`, which sees the click because the drag region reports
    // itself as caption to the hit test.
    #[cfg(target_os = "windows")]
    let _ = window;

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    if window.window_controls().maximize && window.is_resizable() {
        window.zoom_window();
    }
}

/// Match Cursor's macOS glass window stack without asking GPUI's transparent
/// Metal target to blend two translucent quads. The semantic tint is painted
/// by GPUI as the sidebar's translucent fill — the slider drives its opacity —
/// while native material shows through the remainder: Sidebar vibrancy, or on
/// macOS 26 an `NSGlassEffectView` filling the strip with the vibrancy view
/// deactivated beneath it — glass lenses whatever is composited beneath it,
/// and leaving vibrancy on would have it refract already-blurred material,
/// which reads as plain blur. With `transparent` off the effect view stops
/// rendering and the glass view hides; GPUI's sidebar fill is opaque by then
/// and covers the strip itself.
#[cfg(target_os = "macos")]
pub fn configure_sidebar_material(
    window: &Window,
    sidebar: gpui::Hsla,
    dark: bool,
    transparent: bool,
) {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSColor, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let transparent = transparent && !reduce_transparency();
    let glass_active = transparent && glass_effect_supported();
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };

    // GPUI owns the view hierarchy and creates the effect view before the
    // root entity is installed. We only adjust public AppKit properties.
    unsafe {
        let view = handle.ns_view.cast::<NSView>().as_ref();
        let Some(native_window) = view.window() else {
            return;
        };
        let sidebar_rgb: gpui::Rgba = sidebar.into();
        let (r, g, b) = (
            f64::from(sidebar_rgb.r),
            f64::from(sidebar_rgb.g),
            f64::from(sidebar_rgb.b),
        );
        let background = if transparent {
            if dark {
                NSColor::colorWithSRGBRed_green_blue_alpha(0.0, 0.0, 0.0, 0.25)
            } else {
                NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 1.0, 1.0, 0.0)
            }
        } else {
            // The window stays non-opaque, so a clear pixel would otherwise
            // show the desktop; an opaque backdrop keeps any uncovered gap
            // the same color GPUI paints the sidebar.
            NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
        };
        native_window.setBackgroundColor(Some(&background));

        let Some(content_view) = native_window.contentView() else {
            return;
        };

        let mut configured_effect = false;
        for subview in content_view.subviews().iter() {
            let Some(effect_view) = subview.downcast_ref::<NSVisualEffectView>() else {
                continue;
            };
            effect_view.setMaterial(NSVisualEffectMaterial::Sidebar);
            effect_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            // Glass samples the window backdrop beneath it; with vibrancy
            // active it would refract already-blurred material, so when the
            // glass surface owns the strip the vibrancy view turns off and
            // the glass lenses the desktop directly.
            effect_view.setState(if transparent && !glass_active {
                NSVisualEffectState::Active
            } else {
                NSVisualEffectState::Inactive
            });
            configured_effect = true;
        }
        if !configured_effect {
            return;
        }

        // macOS 26 fills the strip with a real liquid-glass surface.
        // Allocation itself is gated — the class is absent on older systems
        // and `class!` would panic.
        if glass_effect_supported() {
            // A light constant tint keeps the bare lens on-theme; the
            // slider's density lives in GPUI's translucent sidebar fill
            // above the Metal layer.
            let glass_tint = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 0.15);
            SIDEBAR_GLASS_VIEW.with_borrow_mut(|slot| {
                let needs_new_view = slot.as_ref().is_none_or(|glass_view| {
                    glass_view
                        .window()
                        .as_deref()
                        .is_none_or(|window| !std::ptr::eq(window, native_window.as_ref()))
                });
                if needs_new_view {
                    let mut frame = content_view.bounds();
                    frame.size.width = SIDEBAR_WIDTH;
                    let glass_view = objc2_app_kit::NSGlassEffectView::initWithFrame(
                        objc2_app_kit::NSGlassEffectView::alloc(main_thread),
                        frame,
                    );
                    glass_view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);
                    // The strip reaches the window edge; a capsule radius would
                    // round the wrong corners.
                    glass_view.setCornerRadius(0.0);
                    content_view.addSubview_positioned_relativeTo(
                        &glass_view,
                        NSWindowOrderingMode::Below,
                        Some(view),
                    );
                    *slot = Some(glass_view);
                }

                if let Some(glass_view) = slot.as_ref() {
                    glass_view.setHidden(!glass_active);
                    if glass_active {
                        glass_view.setTintColor(Some(&glass_tint));
                    }
                }
            });
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn configure_sidebar_material(_: &Window, _: gpui::Hsla, _: bool, _: bool) {}

#[cfg(target_os = "macos")]
pub fn set_sidebar_material_width(window: &Window, width: f32) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let Some(_main_thread) = MainThreadMarker::new() else {
        return;
    };

    unsafe {
        let view = handle.ns_view.cast::<NSView>().as_ref();
        let Some(native_window) = view.window() else {
            return;
        };
        SIDEBAR_GLASS_VIEW.with_borrow(|slot| {
            let Some(glass_view) = slot.as_ref().filter(|glass_view| {
                glass_view
                    .window()
                    .as_deref()
                    .is_some_and(|window| std::ptr::eq(window, native_window.as_ref()))
            }) else {
                return;
            };
            let mut frame = glass_view.frame();
            frame.size.width = width.into();
            glass_view.setFrame(frame);
        });
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_sidebar_material_width(_: &Window, _: f32) {}

/// The window-server id (`-[NSWindow windowNumber]`) used to snapshot this
/// window's own contents — Big Picture's blurred backdrop.
#[cfg(target_os = "macos")]
pub fn window_capture_id(window: &Window) -> Option<u32> {
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    unsafe {
        let view = handle.ns_view.cast::<NSView>().as_ref();
        u32::try_from(view.window()?.windowNumber()).ok()
    }
}

#[cfg(not(target_os = "macos"))]
pub fn window_capture_id(_: &Window) -> Option<u32> {
    None
}

/// A heavily blurred snapshot of the window's own contents, for painting
/// under an overlay's scrim. Pure CPU work — capture, channel repack,
/// downscale, gaussian — meant for a background executor; the frame only
/// paints the returned `RenderImage`.
#[cfg(target_os = "macos")]
pub fn blurred_window_snapshot(window_id: u32) -> Option<std::sync::Arc<gpui::RenderImage>> {
    use objc2::AnyThread;
    use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep};
    use objc2_core_graphics::CGImage;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    unsafe extern "C" {
        fn CGWindowListCreateImage(
            bounds: NSRect,
            option: u32,
            window_id: u32,
            image_option: u32,
        ) -> *mut CGImage;
    }
    const OPTION_INCLUDING_WINDOW: u32 = 1 << 3;
    const IMAGE_IGNORE_FRAMING: u32 = 1;
    const IMAGE_NOMINAL_RESOLUTION: u32 = 1 << 4;

    // CGRectNull — the whole window rather than a rect of it.
    let image = unsafe {
        let raw = CGWindowListCreateImage(
            NSRect::new(
                NSPoint::new(f64::INFINITY, f64::INFINITY),
                NSSize::new(0.0, 0.0),
            ),
            OPTION_INCLUDING_WINDOW,
            window_id,
            IMAGE_IGNORE_FRAMING | IMAGE_NOMINAL_RESOLUTION,
        );
        objc2::rc::Retained::from_raw(raw)?
    };
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
    if rep.isPlanar() || rep.bitsPerSample() != 8 {
        return None;
    }
    let width = usize::try_from(rep.pixelsWide()).ok()?;
    let height = usize::try_from(rep.pixelsHigh()).ok()?;
    let bytes_per_row = usize::try_from(rep.bytesPerRow()).ok()?;
    let samples = usize::try_from(rep.samplesPerPixel()).ok()?;
    let format = rep.bitmapFormat();
    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, bytes_per_row.checked_mul(height)?) };
    let bgra = crate::browser::bgra_from_bitmap(
        bytes,
        width,
        height,
        bytes_per_row,
        samples,
        format.contains(NSBitmapFormat::AlphaFirst),
        format.contains(NSBitmapFormat::ThirtyTwoBitLittleEndian),
    )?;
    let buffer = image::RgbaImage::from_raw(width as u32, height as u32, bgra)?;
    // A hard downscale plus a small-radius gaussian on the thumbnail reads as
    // a deep blur once the image stretches back across the window.
    let blurred = image::imageops::blur(
        &image::imageops::resize(
            &buffer,
            (width as u32 / 8).max(48),
            (height as u32 / 8).max(48),
            image::imageops::FilterType::Triangle,
        ),
        3.0,
    );
    Some(std::sync::Arc::new(gpui::RenderImage::new(vec![
        image::Frame::new(blurred),
    ])))
}

#[cfg(not(target_os = "macos"))]
pub fn blurred_window_snapshot(_: u32) -> Option<std::sync::Arc<gpui::RenderImage>> {
    None
}

/// Opt-in three-finger trackpad swipe for back/forward, recognized from the
/// window's touch stream by the platform layer.
#[cfg(target_os = "macos")]
pub fn set_trackpad_navigation_swipe_enabled(window: &Window, enabled: bool) {
    window.set_trackpad_navigation_swipe_enabled(enabled);
}

#[cfg(not(target_os = "macos"))]
pub fn set_trackpad_navigation_swipe_enabled(_: &Window, _: bool) {}

/// Follow macOS when `dark` is `None`, otherwise force the native titlebar,
/// traffic lights, menus, and vibrancy to the selected appearance.
#[cfg(target_os = "macos")]
pub fn set_window_appearance(window: &Window, dark: Option<bool>) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSView,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let Some(_main_thread) = MainThreadMarker::new() else {
        return;
    };

    unsafe {
        let view = handle.ns_view.cast::<NSView>().as_ref();
        let Some(native_window) = view.window() else {
            return;
        };
        let appearance = dark.and_then(|dark| {
            NSAppearance::appearanceNamed(if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            })
        });
        native_window.setAppearance(appearance.as_deref());
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_window_appearance(_: &Window, _: Option<bool>) {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::parse_boolean_setting;

    #[test]
    fn boolean_desktop_settings_are_parsed_case_insensitively() {
        assert_eq!(parse_boolean_setting(" true\n"), Some(true));
        assert_eq!(parse_boolean_setting("OFF"), Some(false));
        assert_eq!(parse_boolean_setting("default"), None);
    }

    #[test]
    fn embedded_linux_icon_decodes_at_desktop_size() {
        let icon = super::linux_app_icon().expect("embedded PNG should decode");

        assert_eq!(icon.dimensions(), (256, 256));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use std::{borrow::Cow, path::Path};

    use super::{completion_sound_data, termy_open_url};

    #[test]
    fn termy_projects_use_the_new_tab_deeplink() {
        let url = url::Url::parse(&termy_open_url(Path::new("/tmp/project +%")))
            .expect("Termy deeplink should be valid");

        assert_eq!(url.scheme(), "termy");
        assert_eq!(url.host_str(), Some("new"));
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            vec![(Cow::Borrowed("dir"), Cow::Borrowed("/tmp/project +%"))]
        );
    }

    #[test]
    fn every_bundled_completion_sound_decodes() {
        use objc2::AnyThread;
        use objc2_app_kit::NSSound;
        use objc2_foundation::NSData;
        use waku_client::persistence::CompletionSound;

        for variant in CompletionSound::ALL {
            let data = NSData::with_bytes(completion_sound_data(variant));
            let sound = NSSound::initWithData(NSSound::alloc(), &data)
                .unwrap_or_else(|| panic!("{} should decode", variant.label()));
            assert!(sound.duration() > 0.0, "{}", variant.label());
        }
    }

    #[test]
    fn hears_consent_matches_spoken_variants() {
        for text in [
            "Go ahead.",
            "go-ahead",
            "okay, GO AHEAD please",
            "sure — go ahead",
        ] {
            assert!(super::hears_consent(text), "{text} should consent");
        }
        for text in ["go", "hold on", "go-a", "gopher", "", "no thank you"] {
            assert!(!super::hears_consent(text), "{text} should not consent");
        }
    }
}
