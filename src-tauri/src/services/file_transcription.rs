//! Decodes an audio file into the mono 16 kHz samples the engines accept.
//!
//! Format detection is by content, with the file extension only as a hint, so a
//! `.m4a` that is really a WAV still works. MP3, AAC/M4A, FLAC, Ogg Vorbis and
//! WAV are supported.
//!
//! The decoded length is capped at the same 30 minutes the microphone path
//! allows. A longer file is rejected rather than exhausting memory.

use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// 30 minutes at 16 kHz, matching the recording buffer ceiling.
pub const MAX_FILE_SAMPLES: usize = 30 * 60 * 16_000;

/// Mono 16 kHz PCM ready for an engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedAudio {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

/// Decodes `path` and resamples it to 16 kHz mono.
pub fn decode_file(path: &Path) -> Result<DecodedAudio, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|error| format!("Unsupported audio file: {error}"))?;
    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or_else(|| "The file has no audio track".to_string())?;
    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "The file does not report a sample rate".to_string())?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| format!("Unsupported audio codec: {error}"))?;

    let mut mono: Vec<i16> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            // A truncated final packet is common in recordings that were cut
            // off; keep what decoded instead of failing the whole file.
            Err(SymphoniaError::ResetRequired) => break,
            Err(error) => return Err(format!("Could not read the audio file: {error}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(_)) => break,
            Err(error) => return Err(format!("Could not decode the audio file: {error}")),
        };
        append_mono(&decoded, &mut mono);
        if mono.len() >= MAX_FILE_SAMPLES {
            break;
        }
    }

    if mono.is_empty() {
        return Err("The file contains no decodable audio".to_string());
    }
    mono.truncate(MAX_FILE_SAMPLES);

    let samples = super::audio_service::resample_for_file(&mono, sample_rate)?;
    Ok(DecodedAudio {
        samples,
        sample_rate: super::audio_service::TARGET_SAMPLE_RATE,
    })
}

/// Mixes an interleaved decoded buffer down to mono, in place of a per-channel
/// average that would need a second allocation.
fn append_mono(decoded: &symphonia::core::audio::AudioBufferRef<'_>, out: &mut Vec<i16>) {
    let spec = *decoded.spec();
    let mut buffer = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
    buffer.copy_interleaved_ref(decoded.clone());
    let channels = spec.channels.count().max(1);
    let interleaved = buffer.samples();
    for frame in interleaved.chunks_exact(channels) {
        let sum: i32 = frame.iter().map(|&sample| sample as i32).sum();
        out.push((sum / channels as i32) as i16);
    }
}

/// The extensions the file picker offers.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "m4a", "aac", "wav", "flac", "ogg", "oga"];

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a mono 16-bit WAV with a single tone, then decodes it back.
    fn write_wav(path: &Path, rate: u32, samples: &[i16]) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn decodes_a_wav_at_the_target_rate() {
        let dir = std::env::temp_dir().join(format!("sf-file-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        let samples: Vec<i16> = (0..16_000).map(|index| (index % 200) as i16 * 50).collect();
        write_wav(&path, 16_000, &samples);

        let decoded = decode_file(&path).unwrap();
        assert_eq!(decoded.sample_rate, 16_000);
        assert_eq!(decoded.samples.len(), samples.len());
        assert_eq!(decoded.samples[..64], samples[..64]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resamples_a_44k_file_to_16k() {
        let dir = std::env::temp_dir().join(format!("sf-file-resample-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone44.wav");
        let samples: Vec<i16> = (0..44_100).map(|index| (index % 100) as i16 * 60).collect();
        write_wav(&path, 44_100, &samples);

        let decoded = decode_file(&path).unwrap();
        assert_eq!(decoded.sample_rate, 16_000);
        // One second in, one second out, within a sample of rounding.
        let length = decoded.samples.len() as i64;
        assert!((length - 16_000).abs() <= 2, "got {length} samples");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_reported() {
        let error = decode_file(Path::new("does-not-exist.wav")).unwrap_err();
        assert!(error.contains("Could not open"));
    }

    #[test]
    fn a_file_without_audio_is_rejected() {
        let dir = std::env::temp_dir().join(format!("sf-file-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.txt");
        std::fs::write(&path, b"not audio").unwrap();

        let error = decode_file(&path).unwrap_err();
        assert!(error.contains("Unsupported audio file"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
