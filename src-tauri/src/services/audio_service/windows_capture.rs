use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use wasapi::{
    AudioCaptureClient, AudioClient, AudioClientProperties, Device, DeviceEnumerator, Direction,
    Handle, SampleType, StreamCategory, StreamMode, WasapiError, WaveFormat, GUID,
};

use super::capture::{mix_to_mono_capped_i16, warn_if_record_cap_reached, MAX_RECORD_SAMPLES};
use super::TARGET_SAMPLE_RATE;

const EVENT_WAIT_MS: u32 = 200;
const PRIME_ATTEMPTS: usize = 5;

const EFFECT_AEC: GUID = GUID::from_u128(0x6f64adbe_8211_11e2_8c70_2c27d7f001fa);
const EFFECT_NOISE_SUPPRESSION: GUID = GUID::from_u128(0x6f64adbf_8211_11e2_8c70_2c27d7f001fa);
const EFFECT_AUTOMATIC_GAIN_CONTROL: GUID = GUID::from_u128(0x6f64adc0_8211_11e2_8c70_2c27d7f001fa);
const EFFECT_BEAMFORMING: GUID = GUID::from_u128(0x6f64adc1_8211_11e2_8c70_2c27d7f001fa);
const EFFECT_DEEP_NOISE_SUPPRESSION: GUID = GUID::from_u128(0x6f64add0_8211_11e2_8c70_2c27d7f001fa);

struct ComGuard;

impl ComGuard {
    fn initialize() -> Result<Self, String> {
        wasapi::initialize_mta()
            .ok()
            .map_err(|err| format!("Failed to initialize COM MTA: {err}"))?;
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        wasapi::deinitialize();
    }
}

pub(super) struct WindowsSpeechCapture {
    client: AudioClient,
    capture_client: AudioCaptureClient,
    event: Handle,
    raw_buffer: Vec<u8>,
    chunk_buffer: Vec<i16>,
    // 必须最后析构，确保所有 COM 音频对象先释放。
    _com: ComGuard,
}

impl WindowsSpeechCapture {
    pub(super) fn start(preferred_name: Option<&str>) -> Result<Self, String> {
        let com = ComGuard::initialize()?;
        let enumerator = DeviceEnumerator::new()
            .map_err(|err| format!("Failed to create device enumerator: {err}"))?;
        let (device, device_name) = select_capture_device(&enumerator, preferred_name)?;

        let categories = [
            (StreamCategory::VoiceTyping, "VoiceTyping"),
            (StreamCategory::UniformSpeech, "UniformSpeech"),
            (StreamCategory::Speech, "Speech"),
            (StreamCategory::Communications, "Communications"),
        ];
        let mut failures = Vec::with_capacity(categories.len());

        for (category, category_name) in categories {
            match Self::start_with_category(&enumerator, &device, category, category_name) {
                Ok((client, capture_client, event, raw_buffer)) => {
                    #[cfg(test)]
                    eprintln!("native_category={category_name}");
                    log::info!(
                        "Recording with Windows native voice processing: device='{}', category={}, {}Hz, 1ch, i16",
                        device_name,
                        category_name,
                        TARGET_SAMPLE_RATE
                    );
                    return Ok(Self {
                        _com: com,
                        client,
                        capture_client,
                        event,
                        raw_buffer,
                        chunk_buffer: Vec::with_capacity(1024),
                    });
                }
                Err(err) => failures.push(format!("{category_name}: {err}")),
            }
        }

        Err(format!(
            "Device '{}' does not support dictation audio stream ({})",
            device_name,
            failures.join("; ")
        ))
    }

    fn start_with_category(
        enumerator: &DeviceEnumerator,
        device: &Device,
        category: StreamCategory,
        category_name: &str,
    ) -> Result<(AudioClient, AudioCaptureClient, Handle, Vec<u8>), String> {
        let mut client = device
            .get_iaudioclient()
            .map_err(|err| format!("Failed to create AudioClient: {err}"))?;
        client
            .set_properties(AudioClientProperties::new().set_category(category))
            .map_err(|err| format!("Failed to set {category_name} category: {err}"))?;

        let desired_format = WaveFormat::new(
            16,
            16,
            &SampleType::Int,
            TARGET_SAMPLE_RATE as usize,
            1,
            None,
        );
        let (_, min_period) = client
            .get_device_period()
            .map_err(|err| format!("Failed to read device period: {err}"))?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: min_period,
        };
        client
            .initialize_client(&desired_format, &Direction::Capture, &mode)
            .map_err(|err| format!("Failed to initialize shared audio stream: {err}"))?;

        log_audio_effects(&client);
        configure_aec(&client, enumerator);

        let event = client
            .set_get_eventhandle()
            .map_err(|err| format!("Failed to create audio event: {err}"))?;
        let buffer_frames = client
            .get_buffer_size()
            .map_err(|err| format!("Failed to read audio buffer size: {err}"))?;
        let capture_client = client
            .get_audiocaptureclient()
            .map_err(|err| format!("Failed to create capture client: {err}"))?;
        client
            .start_stream()
            .map_err(|err| format!("Failed to start audio stream: {err}"))?;

        Ok((
            client,
            capture_client,
            event,
            vec![0; buffer_frames as usize * desired_format.get_blockalign() as usize],
        ))
    }

    pub(super) fn prime(
        &mut self,
        samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    ) -> Result<(), String> {
        for _ in 0..PRIME_ATTEMPTS {
            match self.read_event(samples) {
                Ok(frames) if frames > 0 => return Ok(()),
                Ok(_) | Err(WasapiError::EventTimeout) => continue,
                Err(err) => return Err(err.to_string()),
            }
        }
        Err(format!(
            "No audio data received within {} ms",
            EVENT_WAIT_MS as usize * PRIME_ATTEMPTS
        ))
    }

    pub(super) fn run(
        &mut self,
        stop: &Arc<AtomicBool>,
        samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    ) {
        while !stop.load(Ordering::Relaxed) {
            match self.read_event(samples) {
                Ok(_) | Err(WasapiError::EventTimeout) => {}
                Err(err) => {
                    log::error!("Windows native audio stream error: {}", err);
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
    }

    fn read_event(
        &mut self,
        samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    ) -> Result<usize, WasapiError> {
        self.event.wait_for_event(EVENT_WAIT_MS)?;
        let mut total_frames = 0;

        loop {
            let frames = self.capture_client.get_next_packet_size()?.unwrap_or(0) as usize;
            if frames == 0 {
                break;
            }
            let required_bytes = frames * std::mem::size_of::<i16>();
            if self.raw_buffer.len() < required_bytes {
                self.raw_buffer.resize(required_bytes, 0);
            }
            let (read_frames, info) = self
                .capture_client
                .read_from_device(&mut self.raw_buffer[..required_bytes])?;
            let read_frames = read_frames as usize;
            if read_frames == 0 {
                continue;
            }

            self.chunk_buffer.clear();
            if info.flags.silent {
                self.chunk_buffer.resize(read_frames, 0);
            } else {
                self.chunk_buffer.extend(
                    self.raw_buffer[..read_frames * std::mem::size_of::<i16>()]
                        .chunks_exact(2)
                        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]])),
                );
            }

            let mut locked = samples.lock();
            warn_if_record_cap_reached(locked.len());
            mix_to_mono_capped_i16(&self.chunk_buffer, 1, &mut locked, MAX_RECORD_SAMPLES);
            total_frames += read_frames;

            if info.flags.data_discontinuity {
                log::warn!("Windows native audio stream detected data discontinuity");
            }
        }

        Ok(total_frames)
    }
}

impl Drop for WindowsSpeechCapture {
    fn drop(&mut self) {
        if let Err(err) = self.client.stop_stream() {
            log::debug!(
                "Error returned when stopping Windows native audio stream: {}",
                err
            );
        }
    }
}

fn select_capture_device(
    enumerator: &DeviceEnumerator,
    preferred_name: Option<&str>,
) -> Result<(Device, String), String> {
    if let Some(name) = preferred_name.filter(|name| !name.trim().is_empty()) {
        let collection = enumerator
            .get_device_collection(&Direction::Capture)
            .map_err(|err| format!("Failed to enumerate input devices: {err}"))?;
        match collection.get_device_with_name(name) {
            Ok(device) => return Ok((device, name.to_owned())),
            Err(err) => log::warn!(
                "Windows native capture could not find specified microphone '{}'; falling back to default device: {}",
                name,
                err
            ),
        }
    }

    let device = enumerator
        .get_default_device(&Direction::Capture)
        .map_err(|err| format!("Failed to get default input device: {err}"))?;
    let name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "Unknown Device".into());

    if super::capture::should_avoid_bluetooth_mic() && super::capture::is_bluetooth_mic_name(&name)
    {
        if let Ok(collection) = enumerator.get_device_collection(&Direction::Capture) {
            let count = collection.get_nbr_devices().unwrap_or(0);
            for idx in 0..count {
                if let Ok(candidate) = collection.get_device_at_index(idx) {
                    if let Ok(candidate_name) = candidate.get_friendlyname() {
                        if !super::capture::is_bluetooth_mic_name(&candidate_name) {
                            log::info!(
                                "Avoided Bluetooth default microphone '{}' in favor of '{}'",
                                name,
                                candidate_name
                            );
                            return Ok((candidate, candidate_name));
                        }
                    }
                }
            }
        }
    }

    Ok((device, name))
}

fn configure_aec(client: &AudioClient, enumerator: &DeviceEnumerator) {
    let control = match client.get_aec_control() {
        Ok(control) => control,
        Err(_) => {
            log::info!("Windows current device/voice mode does not provide controllable AEC");
            #[cfg(test)]
            eprintln!("native_aec=false");
            return;
        }
    };

    let render_endpoint_id = enumerator
        .get_default_device(&Direction::Render)
        .ok()
        .and_then(|device| device.get_id().ok());
    let endpoint_mode = if render_endpoint_id.is_some() {
        "default playback device"
    } else {
        "Windows automatically selected playback device"
    };
    match control.set_echo_cancellation_render_endpoint(render_endpoint_id) {
        Ok(()) => {
            log::info!("Windows AEC enabled and bound to {}", endpoint_mode);
            #[cfg(test)]
            eprintln!("native_aec=true");
        }
        Err(err) => log::warn!("Windows AEC is available but failed to enable: {}", err),
    }
}

fn log_audio_effects(client: &AudioClient) {
    let effects = match client
        .get_audio_effects_manager()
        .and_then(|manager| manager.get_audio_effects())
    {
        Ok(Some(effects)) => effects,
        Ok(None) => {
            log::info!("Windows native audio stream reported no active audio effects");
            #[cfg(test)]
            eprintln!("native_effects=[] native_effect_count=0");
            return;
        }
        Err(err) => {
            log::info!(
                "Current Windows/driver does not support enumerating active audio effects: {}",
                err
            );
            #[cfg(test)]
            eprintln!("native_effects=unavailable");
            return;
        }
    };

    let known: Vec<_> = effects
        .iter()
        .filter_map(|effect| known_effect_name(&effect.id))
        .collect();
    log::info!(
        "Windows native audio stream active audio effects: recognized=[{}], total={}",
        known.join(", "),
        effects.len()
    );
    #[cfg(test)]
    eprintln!(
        "native_effects=[{}] native_effect_count={}",
        known.join(","),
        effects.len()
    );
}

fn known_effect_name(id: &GUID) -> Option<&'static str> {
    match *id {
        EFFECT_AEC => Some("AEC"),
        EFFECT_NOISE_SUPPRESSION => Some("NS"),
        EFFECT_DEEP_NOISE_SUPPRESSION => Some("Deep NS"),
        EFFECT_AUTOMATIC_GAIN_CONTROL => Some("AGC"),
        EFFECT_BEAMFORMING => Some("Beamforming"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_windows_speech_effect_guids() {
        assert_eq!(known_effect_name(&EFFECT_AEC), Some("AEC"));
        assert_eq!(
            known_effect_name(&EFFECT_DEEP_NOISE_SUPPRESSION),
            Some("Deep NS")
        );
        assert_eq!(known_effect_name(&GUID::zeroed()), None);
    }

    #[test]
    #[ignore = "requires an available Windows microphone"]
    fn native_speech_capture_receives_audio_frames() {
        let started = std::time::Instant::now();
        let samples = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let mut capture = WindowsSpeechCapture::start(None).expect("start native capture");
        capture.prime(&samples).expect("receive first audio packet");

        let stop_later = stop.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            stop_later.store(true, Ordering::Relaxed);
        });
        capture.run(&stop, &samples);
        stopper.join().unwrap();

        let frame_count = samples.lock().len();
        eprintln!(
            "native_frames={} elapsed_ms={:.1}",
            frame_count,
            started.elapsed().as_secs_f64() * 1000.0
        );
        assert!(frame_count >= TARGET_SAMPLE_RATE as usize / 4);
    }
}
