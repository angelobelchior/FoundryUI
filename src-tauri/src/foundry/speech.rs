use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};

use super::error::{FoundryError, FoundryResult};
use super::jobs::now_millis;

pub const MAX_AUDIO_BYTES: u64 = 25 * 1024 * 1024;
pub const RECORDING_SAMPLE_RATE: u32 = 16_000;
pub const MAX_RECORDING_SECONDS: u64 = 300;
pub const TRANSCRIPTION_TIMEOUT_SECONDS: u64 = 300;

const LEVEL_REFRESH: Duration = Duration::from_millis(20);
const FILE_STEM_LIMIT: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Wav,
    Flac,
    Ogg,
    Webm,
}

impl AudioFormat {
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.trim().to_lowercase().as_str() {
            "mp3" => Some(Self::Mp3),
            "wav" | "wave" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "ogg" | "oga" => Some(Self::Ogg),
            "webm" => Some(Self::Webm),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Webm => "webm",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Wav => "audio/wav",
            Self::Flac => "audio/flac",
            Self::Ogg => "audio/ogg",
            Self::Webm => "audio/webm",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    pub path: String,
    pub name: String,
    pub mime: String,
    pub bytes: u64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MicrophonePermission {
    Granted,
    Denied,
    Restricted,
    Undetermined,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct RecordedAudio {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

#[derive(Debug, Clone)]
pub struct TranscriptionPayload {
    pub text: String,
    pub language: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
struct Mp3Frame {
    sample_rate: u32,
    samples: u32,
    bitrate: u32,
    length: usize,
}

pub fn import_audio(source: &Path, root: &Path) -> FoundryResult<AudioInfo> {
    let metadata = fs::metadata(source)
        .map_err(|error| rejected(format!("the file could not be read: {}", error)))?;

    if !metadata.is_file() {
        return Err(rejected("the path is not a file"));
    }

    if metadata.len() > MAX_AUDIO_BYTES {
        return Err(rejected(format!(
            "the file is larger than {} MB",
            MAX_AUDIO_BYTES / (1024 * 1024)
        )));
    }

    let format = source
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(AudioFormat::from_extension)
        .ok_or_else(|| rejected("the file must be MP3, WAV, FLAC, OGG or WebM"))?;
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("audio")
        .to_string();
    let stem = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| safe_stem(stem, "audio"))
        .unwrap_or_else(|| "audio".to_string());
    let target = unique_path(
        root,
        &format!("{}-{}", now_millis(), stem),
        format.extension(),
    );
    let duration_ms = probe_duration_ms(source).unwrap_or_default();

    fs::create_dir_all(root).map_err(storage)?;
    fs::copy(source, &target).map_err(storage)?;

    Ok(AudioInfo {
        path: target.to_string_lossy().into_owned(),
        name,
        mime: format.mime().to_string(),
        bytes: metadata.len(),
        duration_ms: duration_ms.min(i64::MAX as u64) as i64,
    })
}

pub fn probe_duration_ms(path: &Path) -> Option<u64> {
    let bytes = fs::read(path).ok()?;

    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(AudioFormat::from_extension)
    {
        Some(AudioFormat::Wav) => wav_duration_ms(&bytes),
        Some(AudioFormat::Flac) => flac_duration_ms(&bytes),
        Some(AudioFormat::Ogg) => ogg_duration_ms(&bytes),
        Some(AudioFormat::Mp3) => mp3_duration_ms(&bytes),
        Some(AudioFormat::Webm) => webm_duration_ms(&bytes),
        None => None,
    }
}

pub fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_bytes = samples.len() * 2;
    let mut output = Vec::with_capacity(44 + data_bytes);
    let byte_rate = sample_rate * 2;

    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&((36 + data_bytes) as u32).to_le_bytes());
    output.extend_from_slice(b"WAVE");
    output.extend_from_slice(b"fmt ");
    output.extend_from_slice(&16u32.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&sample_rate.to_le_bytes());
    output.extend_from_slice(&byte_rate.to_le_bytes());
    output.extend_from_slice(&2u16.to_le_bytes());
    output.extend_from_slice(&16u16.to_le_bytes());
    output.extend_from_slice(b"data");
    output.extend_from_slice(&(data_bytes as u32).to_le_bytes());

    for sample in samples {
        output.extend_from_slice(&sample.to_le_bytes());
    }

    output
}

pub fn resample_linear(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if samples.is_empty() || from == 0 || to == 0 {
        return Vec::new();
    }

    if from == to {
        return samples.to_vec();
    }

    let ratio = from as f64 / to as f64;
    let length = ((samples.len() as f64) * to as f64 / from as f64).round() as usize;
    let length = length.max(1);
    let mut output = Vec::with_capacity(length);

    for index in 0..length {
        let position = index as f64 * ratio;
        let left = position.floor() as usize;
        let fraction = (position - left as f64) as f32;
        let a = samples[left.min(samples.len() - 1)];
        let b = samples[(left + 1).min(samples.len() - 1)];

        output.push(a + (b - a) * fraction);
    }

    output
}

pub fn samples_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

pub fn write_recording(root: &Path, recorded: &RecordedAudio) -> FoundryResult<AudioInfo> {
    fs::create_dir_all(root).map_err(storage)?;

    let path = unique_path(root, &format!("recording-{}", now_millis()), "wav");
    let bytes = encode_wav(&recorded.samples, recorded.sample_rate);

    fs::write(&path, &bytes).map_err(storage)?;

    Ok(AudioInfo {
        path: path.to_string_lossy().into_owned(),
        name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("recording.wav")
            .to_string(),
        mime: "audio/wav".to_string(),
        bytes: bytes.len() as u64,
        duration_ms: recorded.samples.len() as i64 * 1000 / recorded.sample_rate as i64,
    })
}

pub fn write_text_export(directory: &Path, stem: &str, text: &str) -> FoundryResult<PathBuf> {
    fs::create_dir_all(directory).map_err(export_error)?;

    let stem = safe_stem(stem, "transcription");
    let mut candidate = directory.join(format!("{stem}.txt"));
    let mut index = 2;

    while candidate.exists() {
        if index > 999 {
            return Err(FoundryError::ExportFailed {
                detail: format!("too many files named {stem} in {}", directory.display()),
            });
        }

        candidate = directory.join(format!("{stem} ({index}).txt"));
        index += 1;
    }

    fs::write(&candidate, text).map_err(export_error)?;

    Ok(candidate)
}

pub fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| safe_stem(stem, "transcription"))
        .unwrap_or_else(|| "transcription".to_string())
}

pub fn read_audio_data_url(root: &Path, path: &Path) -> FoundryResult<String> {
    ensure_within(root, path)?;
    let bytes = fs::read(path).map_err(storage)?;
    let mime = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(AudioFormat::from_extension)
        .map(AudioFormat::mime)
        .unwrap_or("application/octet-stream");

    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

pub fn remove_audio(root: &Path, path: &Path) -> FoundryResult<()> {
    if !path.exists() {
        return Ok(());
    }

    ensure_within(root, path)?;

    fs::remove_file(path).map_err(storage)
}

pub fn ensure_within(root: &Path, path: &Path) -> FoundryResult<()> {
    let root = fs::canonicalize(root).map_err(storage)?;
    let target = fs::canonicalize(path).map_err(storage)?;

    if target.starts_with(&root) {
        Ok(())
    } else {
        Err(rejected("the audio file is outside the application data"))
    }
}

#[cfg(target_os = "macos")]
pub fn microphone_permission() -> MicrophonePermission {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    let Some(media_type) = (unsafe { AVMediaTypeAudio }) else {
        return MicrophonePermission::Unknown;
    };
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) };

    if status == AVAuthorizationStatus::Authorized {
        MicrophonePermission::Granted
    } else if status == AVAuthorizationStatus::Denied {
        MicrophonePermission::Denied
    } else if status == AVAuthorizationStatus::Restricted {
        MicrophonePermission::Restricted
    } else if status == AVAuthorizationStatus::NotDetermined {
        MicrophonePermission::Undetermined
    } else {
        MicrophonePermission::Unknown
    }
}

#[cfg(not(target_os = "macos"))]
pub fn microphone_permission() -> MicrophonePermission {
    MicrophonePermission::Undetermined
}

pub async fn transcribe(
    endpoint: &super::local_inference::VerifiedLocalEndpoint,
    model: &str,
    file: &Path,
    language: Option<&str>,
    temperature: f32,
) -> FoundryResult<TranscriptionPayload> {
    let format = file
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(AudioFormat::from_extension)
        .ok_or_else(|| rejected("the file must be MP3, WAV, FLAC, OGG or WebM"))?;
    let bytes = fs::read(file).map_err(storage)?;
    let file_name = file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("audio")
        .to_string();
    let part = Part::bytes(bytes)
        .file_name(file_name)
        .mime_str(format.mime())
        .map_err(|error| FoundryError::BadResponse {
            detail: error.to_string(),
        })?;
    let mut form = Form::new()
        .part("file", part)
        .text("model", model.to_string())
        .text("temperature", format!("{temperature}"))
        .text("response_format", "verbose_json");

    if let Some(language) = language.map(str::trim).filter(|value| !value.is_empty()) {
        form = form.text("language", language.to_string());
    }

    let response = endpoint
        .post_multipart(
            "/v1/audio/transcriptions",
            form,
            Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECONDS),
        )
        .await
        .map_err(super::http::local_inference_error)?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        if expects_json_contract_error(status, &body) {
            return transcribe_json_contract(endpoint, model, file, language, temperature).await;
        }

        return Err(transcription_http_error(status, &body, model));
    }

    parse_transcription(&body)
}

async fn transcribe_json_contract(
    endpoint: &super::local_inference::VerifiedLocalEndpoint,
    model: &str,
    file: &Path,
    language: Option<&str>,
    temperature: f32,
) -> FoundryResult<TranscriptionPayload> {
    let mut request = serde_json::json!({
        "model": model,
        "filename": file.to_string_lossy().to_string(),
        "temperature": temperature,
        "response_format": "verbose_json",
    });

    if let Some(language) = language.map(str::trim).filter(|value| !value.is_empty()) {
        request["language"] = serde_json::Value::String(language.to_string());
    }

    let response = endpoint
        .post_json(
            "/v1/audio/transcriptions",
            &request,
            Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECONDS),
        )
        .await
        .map_err(super::http::local_inference_error)?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(transcription_http_error(status, &body, model));
    }

    parse_transcription(&body)
}

pub fn input_devices() -> FoundryResult<Vec<InputDevice>> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let devices = host.input_devices().map_err(recording_error)?;
    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();

    for device in devices {
        let Ok(id) = device.id() else {
            continue;
        };
        let id = id.to_string();
        let name = device.to_string().trim().to_string();

        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }

        let is_default = default_id.as_deref() == Some(id.as_str());

        found.push(InputDevice {
            id,
            name,
            is_default,
        });
    }

    Ok(found)
}

fn resolve_input_device(host: &cpal::Host, requested: Option<&str>) -> FoundryResult<cpal::Device> {
    if let Some(requested) = requested.map(str::trim).filter(|value| !value.is_empty())
        && let Ok(id) = requested.parse::<cpal::DeviceId>()
        && let Some(device) = host.device_by_id(&id)
    {
        return Ok(device);
    }

    host.default_input_device()
        .ok_or_else(|| FoundryError::RecordingFailed {
            detail: "no microphone was found".to_string(),
        })
}

pub struct Recorder {
    active: Mutex<Option<ActiveRecording>>,
}

struct ActiveRecording {
    stop: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
    started_at: i64,
    handle: JoinHandle<FoundryResult<RecordedAudio>>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    pub fn start(&self, device: Option<&str>) -> FoundryResult<i64> {
        if microphone_permission() == MicrophonePermission::Denied
            || microphone_permission() == MicrophonePermission::Restricted
        {
            return Err(FoundryError::MicrophoneDenied);
        }

        let mut active = lock(&self.active);

        if let Some(recording) = active.as_ref() {
            return Ok(recording.started_at);
        }

        let stop = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0));
        let thread_stop = stop.clone();
        let thread_level = level.clone();
        let target = device
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let handle = std::thread::spawn(move || record_blocking(thread_stop, thread_level, target));
        let started_at = now_millis();

        *active = Some(ActiveRecording {
            stop,
            level,
            started_at,
            handle,
        });

        Ok(started_at)
    }

    pub fn started_at(&self) -> Option<i64> {
        lock(&self.active)
            .as_ref()
            .map(|recording| recording.started_at)
    }

    pub fn level(&self) -> f32 {
        let active = lock(&self.active);
        let Some(recording) = active.as_ref() else {
            return 0.0;
        };

        f32::from_bits(recording.level.load(Ordering::Relaxed)).clamp(0.0, 1.0)
    }

    pub fn stop(&self) -> FoundryResult<RecordedAudio> {
        let Some(recording) = lock(&self.active).take() else {
            return Err(FoundryError::RecordingFailed {
                detail: "there is no recording in progress".to_string(),
            });
        };

        recording.stop.store(true, Ordering::Relaxed);

        recording
            .handle
            .join()
            .map_err(|_| FoundryError::RecordingFailed {
                detail: "the microphone thread stopped unexpectedly".to_string(),
            })?
    }

    pub fn cancel(&self) {
        let Some(recording) = lock(&self.active).take() else {
            return;
        };

        recording.stop.store(true, Ordering::Relaxed);
        let _ = recording.handle.join();
    }

    pub fn is_recording(&self) -> bool {
        lock(&self.active).is_some()
    }
}

fn record_blocking(
    stop: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
    device_name: Option<String>,
) -> FoundryResult<RecordedAudio> {
    let host = cpal::default_host();
    let device = resolve_input_device(&host, device_name.as_deref())?;
    let config = device.default_input_config().map_err(recording_error)?;
    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let callback_samples = samples.clone();
    let callback_level = level.clone();
    let error_level = level.clone();
    let error_callback = move |_error: cpal::Error| {
        error_level.store(0, Ordering::Relaxed);
    };
    let stream = match sample_format {
        SampleFormat::I8 => build_stream::<i8>(
            &device,
            &stream_config,
            callback_samples,
            callback_level,
            channels,
            error_callback,
        ),
        SampleFormat::I16 => build_stream::<i16>(
            &device,
            &stream_config,
            callback_samples,
            callback_level,
            channels,
            error_callback,
        ),
        SampleFormat::I32 => build_stream::<i32>(
            &device,
            &stream_config,
            callback_samples,
            callback_level,
            channels,
            error_callback,
        ),
        SampleFormat::I64 => build_stream::<i64>(
            &device,
            &stream_config,
            callback_samples,
            callback_level,
            channels,
            error_callback,
        ),
        SampleFormat::U8 => build_stream::<u8>(
            &device,
            &stream_config,
            callback_samples,
            callback_level,
            channels,
            error_callback,
        ),
        SampleFormat::U16 => build_stream::<u16>(
            &device,
            &stream_config,
            callback_samples,
            callback_level,
            channels,
            error_callback,
        ),
        SampleFormat::U32 => build_stream::<u32>(
            &device,
            &stream_config,
            callback_samples,
            callback_level,
            channels,
            error_callback,
        ),
        SampleFormat::U64 => build_stream::<u64>(
            &device,
            &stream_config,
            callback_samples,
            callback_level,
            channels,
            error_callback,
        ),
        SampleFormat::F32 => build_stream::<f32>(
            &device,
            &stream_config,
            callback_samples,
            callback_level,
            channels,
            error_callback,
        ),
        SampleFormat::F64 => build_stream::<f64>(
            &device,
            &stream_config,
            callback_samples,
            callback_level,
            channels,
            error_callback,
        ),
        format => {
            return Err(FoundryError::RecordingFailed {
                detail: format!("the microphone uses an unsupported sample format ({format})"),
            });
        }
    }?;

    stream.play().map_err(recording_error)?;

    let deadline = Instant::now() + Duration::from_secs(MAX_RECORDING_SECONDS);

    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        std::thread::sleep(LEVEL_REFRESH);
    }

    drop(stream);

    let captured = lock(&samples);

    if captured.is_empty() {
        return Err(FoundryError::RecordingFailed {
            detail: "the microphone returned no audio".to_string(),
        });
    }

    let resampled = resample_linear(&captured, sample_rate, RECORDING_SAMPLE_RATE);

    Ok(RecordedAudio {
        samples: samples_to_i16(&resampled),
        sample_rate: RECORDING_SAMPLE_RATE,
    })
}

fn build_stream<T: SizedSample + Send + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
    channels: usize,
    error_callback: impl FnMut(cpal::Error) + Send + 'static,
) -> FoundryResult<cpal::Stream>
where
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            *config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                push_mono(data, channels, &samples, &level);
            },
            error_callback,
            None,
        )
        .map_err(recording_error)
}

fn push_mono<T: SizedSample>(
    data: &[T],
    channels: usize,
    samples: &Arc<Mutex<Vec<f32>>>,
    level: &Arc<AtomicU32>,
) where
    f32: FromSample<T>,
{
    let channels = channels.max(1);
    let mut buffer = lock(samples);
    let mut peak = 0.0_f32;

    for frame in data.chunks(channels) {
        let sum: f32 = frame.iter().map(|sample| f32::from_sample(*sample)).sum();
        let mono = sum / channels as f32;

        peak = peak.max(mono.abs());
        buffer.push(mono);
    }

    level.store(peak.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

fn wav_duration_ms(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let mut position = 12;
    let mut sample_rate = 0_u32;
    let mut block_align = 0_u16;
    let mut channels = 0_u16;
    let mut bits = 0_u16;
    let mut data_bytes = 0_usize;

    while position + 8 <= bytes.len() {
        let id = &bytes[position..position + 4];
        let size = u32::from_le_bytes(bytes[position + 4..position + 8].try_into().ok()?) as usize;
        let body = position + 8;

        if id == b"fmt " && size >= 16 && body + 16 <= bytes.len() {
            let format = u16::from_le_bytes(bytes[body..body + 2].try_into().ok()?);
            channels = u16::from_le_bytes(bytes[body + 2..body + 4].try_into().ok()?);
            sample_rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().ok()?);
            block_align = u16::from_le_bytes(bytes[body + 12..body + 14].try_into().ok()?);
            bits = u16::from_le_bytes(bytes[body + 14..body + 16].try_into().ok()?);

            if format == 0xFFFE && size >= 18 && body + 18 <= bytes.len() {
                bits = u16::from_le_bytes(bytes[body + 18..body + 20].try_into().ok()?);
            }
        }

        if id == b"data" {
            let available = bytes.len().saturating_sub(body);
            data_bytes = size.min(available);

            break;
        }

        position = body.checked_add(size)?;
        position += size & 1;
    }

    let frame_bytes = if block_align > 0 {
        block_align as usize
    } else {
        channels as usize * (bits as usize / 8)
    };

    if sample_rate == 0 || frame_bytes == 0 || data_bytes == 0 {
        return None;
    }

    Some(data_bytes as u64 * 1000 / (frame_bytes as u64 * sample_rate as u64))
}

fn flac_duration_ms(bytes: &[u8]) -> Option<u64> {
    if !bytes.starts_with(b"fLaC") {
        return None;
    }

    let mut position = 4;

    while position + 4 <= bytes.len() {
        let header = bytes[position];
        let last = header & 0x80 != 0;
        let kind = header & 0x7F;
        let size = ((bytes[position + 1] as usize) << 16)
            | ((bytes[position + 2] as usize) << 8)
            | bytes[position + 3] as usize;
        let body = position + 4;

        if kind == 0 && size >= 34 && body + 34 <= bytes.len() {
            let packed = u64::from_be_bytes(bytes[body + 10..body + 18].try_into().ok()?);
            let sample_rate = (packed >> 44) & 0xFFFFF;
            let total_samples = packed & 0xF_FFFF_FFFF;

            if sample_rate > 0 && total_samples > 0 {
                return Some(total_samples * 1000 / sample_rate);
            }
        }

        if last {
            break;
        }

        position = body.checked_add(size)?;
    }

    None
}

fn ogg_duration_ms(bytes: &[u8]) -> Option<u64> {
    let first = find(bytes, b"OggS")?;
    let segments = *bytes.get(first + 26)? as usize;
    let payload = first + 27 + segments;
    let packet = bytes.get(payload..)?;
    let sample_rate = if packet.starts_with(b"\x01vorbis") {
        u32::from_le_bytes(packet.get(12..16)?.try_into().ok()?)
    } else if packet.starts_with(b"OpusHead") {
        48_000
    } else {
        return None;
    };

    if sample_rate == 0 {
        return None;
    }

    let mut end = bytes.len();

    while let Some(position) = rfind(&bytes[..end], b"OggS") {
        if let Some(granule) = bytes
            .get(position + 6..position + 14)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            && granule != u64::MAX
        {
            return Some(granule * 1000 / sample_rate as u64);
        }

        end = position;
    }

    None
}

fn mp3_duration_ms(bytes: &[u8]) -> Option<u64> {
    let start = id3_offset(bytes);
    let audio = bytes.get(start..)?;
    let mut scan = 0;

    while scan + 4 <= audio.len().min(65_536) {
        if let Some(frame) = mp3_frame(audio, scan) {
            if let Some(frames) = xing_frames(audio, scan, frame) {
                return Some(
                    frames as u64 * frame.samples as u64 * 1000 / frame.sample_rate as u64,
                );
            }

            let audio_bytes = audio.len().saturating_sub(scan) as u64;

            if frame.bitrate > 0 {
                return Some(audio_bytes * 8 / frame.bitrate as u64);
            }
        }

        scan += 1;
    }

    None
}

fn id3_offset(bytes: &[u8]) -> usize {
    if bytes.len() < 10 || &bytes[0..3] != b"ID3" {
        return 0;
    }

    let size = ((bytes[6] as usize & 0x7F) << 21)
        | ((bytes[7] as usize & 0x7F) << 14)
        | ((bytes[8] as usize & 0x7F) << 7)
        | (bytes[9] as usize & 0x7F);
    let footer = if bytes[5] & 0x10 != 0 { 10 } else { 0 };

    10 + size + footer
}

fn mp3_frame(bytes: &[u8], offset: usize) -> Option<Mp3Frame> {
    let header = bytes.get(offset..offset + 4)?;

    if header[0] != 0xFF || header[1] & 0xE0 != 0xE0 {
        return None;
    }

    let version = ((header[1] >> 3) & 0x03) as u32;
    let layer = ((header[1] >> 1) & 0x03) as u32;
    let bitrate_index = ((header[2] >> 4) & 0x0F) as usize;
    let sample_index = ((header[2] >> 2) & 0x03) as usize;
    let padding = ((header[2] >> 1) & 0x01) as u32;

    if version == 1 || layer == 0 || bitrate_index == 0 || bitrate_index == 15 || sample_index == 3
    {
        return None;
    }

    let sample_rate = match version {
        3 => [44_100, 48_000, 32_000][sample_index],
        2 => [22_050, 24_000, 16_000][sample_index],
        _ => [11_025, 12_000, 8_000][sample_index],
    };
    let bitrate = mp3_bitrate(version, layer, bitrate_index)?;
    let samples = match (layer, version) {
        (3, _) => 384,
        (2, _) => 1152,
        (1, 3) => 1152,
        _ => 576,
    };
    let coefficient = match layer {
        3 => 12,
        1 if version != 3 => 72,
        _ => 144,
    };
    let mut length = coefficient * bitrate / sample_rate + padding;

    if layer == 3 {
        length *= 4;
    }

    Some(Mp3Frame {
        sample_rate,
        samples,
        bitrate,
        length: length as usize,
    })
}

fn mp3_bitrate(version: u32, layer: u32, index: usize) -> Option<u32> {
    let table: [u32; 15] = match (version, layer) {
        (3, 3) => [
            0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
        ],
        (3, 2) => [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
        ],
        (3, 1) => [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ],
        (_, 3) => [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
        ],
        _ => [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
    };
    let value = *table.get(index)?;

    (value > 0).then_some(value * 1000)
}

fn xing_frames(bytes: &[u8], offset: usize, frame: Mp3Frame) -> Option<u32> {
    let header = bytes.get(offset..offset + 4)?;
    let version = (header[1] >> 3) & 0x03;
    let mono = (header[3] >> 6) & 0x03 == 3;
    let side = if version == 3 {
        if mono { 17 } else { 32 }
    } else if mono {
        9
    } else {
        17
    };
    let marker = bytes.get(offset + 4 + side..offset + 8 + side)?;

    if marker != b"Xing" && marker != b"Info" {
        return None;
    }

    let flags = u32::from_be_bytes(
        bytes
            .get(offset + 8 + side..offset + 12 + side)?
            .try_into()
            .ok()?,
    );

    if flags & 0x01 == 0 {
        return None;
    }

    let frames = u32::from_be_bytes(
        bytes
            .get(offset + 12 + side..offset + 16 + side)?
            .try_into()
            .ok()?,
    );

    if frames == 0 || frame.length == 0 {
        None
    } else {
        Some(frames)
    }
}

fn webm_duration_ms(bytes: &[u8]) -> Option<u64> {
    let scale = ebml_number(bytes, &[0x2A, 0xD7, 0xB1]).unwrap_or(1_000_000.0);
    let duration = ebml_float(bytes, &[0x44, 0x89])?;

    if duration <= 0.0 || !duration.is_finite() {
        return None;
    }

    Some((duration * scale / 1_000_000.0).round() as u64)
}

fn ebml_number(bytes: &[u8], id: &[u8]) -> Option<f64> {
    let position = find(bytes, id)?;
    let (size, size_length) = ebml_size(bytes, position + id.len())?;
    let start = position + id.len() + size_length;
    let body = bytes.get(start..start + size)?;

    Some(match size {
        1..=8 => body
            .iter()
            .fold(0_u64, |value, byte| (value << 8) | *byte as u64) as f64,
        _ => return None,
    })
}

fn ebml_float(bytes: &[u8], id: &[u8]) -> Option<f64> {
    let position = find(bytes, id)?;
    let (size, size_length) = ebml_size(bytes, position + id.len())?;
    let start = position + id.len() + size_length;
    let body = bytes.get(start..start + size)?;

    match size {
        4 => Some(f32::from_be_bytes(body.try_into().ok()?) as f64),
        8 => Some(f64::from_be_bytes(body.try_into().ok()?)),
        _ => None,
    }
}

fn ebml_size(bytes: &[u8], position: usize) -> Option<(usize, usize)> {
    let first = *bytes.get(position)?;
    let length = first.leading_zeros() as usize + 1;

    if length > 8 {
        return None;
    }

    let mut value = (first as usize) & (0xFF >> length);

    for index in 1..length {
        value = (value << 8) | *bytes.get(position + index)? as usize;
    }

    Some((value, length))
}

fn parse_transcription(body: &str) -> FoundryResult<TranscriptionPayload> {
    #[derive(Deserialize)]
    struct VerboseTranscription {
        #[serde(default)]
        text: String,
        #[serde(default)]
        language: Option<String>,
        #[serde(default)]
        duration: Option<f64>,
    }

    if let Ok(transcription) = serde_json::from_str::<VerboseTranscription>(body) {
        return Ok(TranscriptionPayload {
            text: transcription.text,
            language: transcription
                .language
                .filter(|value| !value.trim().is_empty()),
            duration_ms: transcription
                .duration
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|value| (value * 1000.0).round() as i64),
        });
    }

    let text = body.trim();

    if text.is_empty() {
        return Err(FoundryError::BadResponse {
            detail: "the transcription response was empty".to_string(),
        });
    }

    Ok(TranscriptionPayload {
        text: text.to_string(),
        language: None,
        duration_ms: None,
    })
}

fn is_missing_model(detail: &str) -> bool {
    let lower = detail.to_lowercase();

    lower.contains("not loaded")
        || lower.contains("model not found")
        || lower.contains("no model matching")
        || lower.contains("model is not loaded")
}

fn expects_json_contract_error(status: reqwest::StatusCode, detail: &str) -> bool {
    if status != reqwest::StatusCode::BAD_REQUEST {
        return false;
    }

    let lower = detail.to_lowercase();

    lower.contains("invalid json") || lower.contains("parse error")
}

fn transcription_http_error(
    status: reqwest::StatusCode,
    detail: &str,
    model: &str,
) -> FoundryError {
    if status == reqwest::StatusCode::NOT_FOUND || is_missing_model(detail) {
        return FoundryError::ModelNotLoaded {
            alias: model.to_string(),
        };
    }

    FoundryError::BadResponse {
        detail: format!("HTTP {status}: {detail}"),
    }
}

fn recording_error(error: cpal::Error) -> FoundryError {
    FoundryError::RecordingFailed {
        detail: error.to_string(),
    }
}

fn safe_stem(value: &str, fallback: &str) -> String {
    let mut stem = String::new();

    for character in value.chars() {
        if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_' | '(' | ')') {
            stem.push(character);
        } else {
            stem.push('-');
        }
    }

    let stem = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    let stem = stem
        .trim_matches(|character| character == '-' || character == ' ')
        .chars()
        .take(FILE_STEM_LIMIT)
        .collect::<String>();
    let stem = stem
        .trim_matches(|character| character == '-' || character == ' ')
        .to_string();

    if stem.is_empty() {
        fallback.to_string()
    } else {
        stem
    }
}

fn unique_path(directory: &Path, stem: &str, extension: &str) -> PathBuf {
    let mut candidate = directory.join(format!("{stem}.{extension}"));
    let mut index = 2;

    while candidate.exists() {
        candidate = directory.join(format!("{stem} ({index}).{extension}"));
        index += 1;
    }

    candidate
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }

    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }

    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

fn rejected(reason: impl Into<String>) -> FoundryError {
    FoundryError::AudioRejected {
        reason: reason.into(),
    }
}

fn storage(error: impl std::fmt::Display) -> FoundryError {
    FoundryError::Storage {
        detail: error.to_string(),
    }
}

fn export_error(error: std::io::Error) -> FoundryError {
    FoundryError::ExportFailed {
        detail: error.to_string(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;

    fn wav_bytes(samples: &[i16], sample_rate: u32) -> Vec<u8> {
        encode_wav(samples, sample_rate)
    }

    #[test]
    fn wav_round_trip_reports_the_duration() {
        let samples = vec![0_i16; 32_000];
        let bytes = wav_bytes(&samples, 16_000);

        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(wav_duration_ms(&bytes), Some(2_000));
    }

    #[test]
    fn flac_streaminfo_reports_the_duration() {
        let sample_rate = 44_100_u64;
        let total_samples = 88_200_u64;
        let packed = (sample_rate << 44) | (2 << 41) | (16 << 36) | total_samples;
        let mut bytes = b"fLaC".to_vec();

        bytes.extend_from_slice(&[0x80, 0, 0, 34]);
        let mut info = [0_u8; 34];
        info[10..18].copy_from_slice(&packed.to_be_bytes());
        bytes.extend_from_slice(&info);

        assert_eq!(flac_duration_ms(&bytes), Some(2_000));
    }

    #[test]
    fn ogg_opus_reports_the_duration_from_the_last_granule() {
        let mut bytes = Vec::new();
        let mut first = Vec::new();
        first.extend_from_slice(b"OggS");
        first.push(0);
        first.push(0x02);
        first.extend_from_slice(&0_u64.to_le_bytes());
        first.extend_from_slice(&1_u32.to_le_bytes());
        first.extend_from_slice(&0_u32.to_le_bytes());
        first.extend_from_slice(&0_u32.to_le_bytes());
        first.push(1);
        first.push(19);
        let mut packet = Vec::new();
        packet.extend_from_slice(b"OpusHead");
        packet.push(1);
        packet.push(1);
        packet.extend_from_slice(&0_u16.to_le_bytes());
        packet.extend_from_slice(&48_000_u32.to_le_bytes());
        packet.extend_from_slice(&0_u16.to_le_bytes());
        packet.push(0);
        first.extend_from_slice(&packet);
        bytes.extend_from_slice(&first);

        let mut last = Vec::new();
        last.extend_from_slice(b"OggS");
        last.push(0);
        last.push(0x04);
        last.extend_from_slice(&96_000_u64.to_le_bytes());
        last.extend_from_slice(&1_u32.to_le_bytes());
        last.extend_from_slice(&1_u32.to_le_bytes());
        last.extend_from_slice(&0_u32.to_le_bytes());
        last.push(0);
        bytes.extend_from_slice(&last);

        assert_eq!(ogg_duration_ms(&bytes), Some(2_000));
    }

    #[test]
    fn mp3_xing_header_reports_the_duration() {
        let mut bytes = vec![0_u8; 500];

        bytes[0] = 0xFF;
        bytes[1] = 0xFB;
        bytes[2] = 0x90;
        bytes[3] = 0x00;
        bytes[36..40].copy_from_slice(b"Xing");
        bytes[40..44].copy_from_slice(&1_u32.to_be_bytes());
        bytes[44..48].copy_from_slice(&100_u32.to_be_bytes());

        assert_eq!(mp3_duration_ms(&bytes), Some(2_612));
    }

    #[test]
    fn webm_duration_uses_the_timecode_scale() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x2A, 0xD7, 0xB1, 0x83, 0x0F, 0x42, 0x40]);
        bytes.extend_from_slice(&[0x44, 0x89, 0x88]);
        bytes.extend_from_slice(&2_000.0_f64.to_be_bytes());

        assert_eq!(webm_duration_ms(&bytes), Some(2_000));
    }

    #[test]
    fn resampling_keeps_the_expected_length() {
        let samples = vec![0.25_f32; 4_800];
        let output = resample_linear(&samples, 48_000, 16_000);

        assert_eq!(output.len(), 1_600);
        assert!(output.iter().all(|sample| (*sample - 0.25).abs() < 0.0001));
    }

    #[test]
    fn importing_copies_the_file_and_measures_it() {
        let directory = tempfile::tempdir().expect("temp dir");
        let source = directory.path().join("meeting.wav");
        let root = directory.path().join("audio");
        let bytes = wav_bytes(&vec![0_i16; 16_000], 16_000);

        fs::write(&source, &bytes).expect("source");

        let info = import_audio(&source, &root).expect("imported");

        assert_eq!(info.name, "meeting.wav");
        assert_eq!(info.mime, "audio/wav");
        assert_eq!(info.bytes, bytes.len() as u64);
        assert_eq!(info.duration_ms, 1_000);
        assert!(Path::new(&info.path).starts_with(&root));
        assert!(Path::new(&info.path).exists());
    }

    #[test]
    fn importing_rejects_unknown_and_missing_files() {
        let directory = tempfile::tempdir().expect("temp dir");
        let text = directory.path().join("notes.txt");
        fs::write(&text, "hello").expect("text");

        assert_eq!(
            import_audio(&text, &directory.path().join("audio"))
                .expect_err("rejected")
                .code(),
            "AudioRejected"
        );
        assert_eq!(
            import_audio(
                &directory.path().join("missing.wav"),
                &directory.path().join("audio")
            )
            .expect_err("rejected")
            .code(),
            "AudioRejected"
        );
    }

    #[test]
    fn recordings_are_written_as_wav_with_a_duration() {
        let directory = tempfile::tempdir().expect("temp dir");
        let recorded = RecordedAudio {
            samples: vec![0_i16; 8_000],
            sample_rate: 16_000,
        };
        let info = write_recording(directory.path(), &recorded).expect("recording");

        assert_eq!(info.mime, "audio/wav");
        assert_eq!(info.duration_ms, 500);
        assert_eq!(probe_duration_ms(Path::new(&info.path)), Some(500));
    }

    #[test]
    fn text_exports_never_overwrite() {
        let directory = tempfile::tempdir().expect("temp dir");
        let first = write_text_export(directory.path(), "notes: one", "one").expect("first");
        let second = write_text_export(directory.path(), "notes: one", "two").expect("second");

        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some("notes- one.txt")
        );
        assert_eq!(
            second.file_name().and_then(|name| name.to_str()),
            Some("notes- one (2).txt")
        );
        assert_eq!(fs::read_to_string(first).expect("first"), "one");
    }

    #[test]
    fn parsing_falls_back_to_plain_text() {
        let payload = parse_transcription("  just words  ").expect("payload");

        assert_eq!(payload.text, "just words");
        assert_eq!(payload.language, None);
        assert_eq!(payload.duration_ms, None);
    }

    #[tokio::test]
    async fn transcribing_posts_multipart_and_parses_verbose_json() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("connection");
            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 4096];

            loop {
                let read = socket.read(&mut chunk).expect("read");

                if read == 0 {
                    break;
                }

                buffer.extend_from_slice(&chunk[..read]);

                if let Some(end) = find(&buffer, b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&buffer[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);

                    if buffer.len() >= end + 4 + length {
                        break;
                    }
                }
            }

            let body = String::from_utf8_lossy(&buffer).to_string();

            assert!(body.contains("name=\"model\""));
            assert!(body.contains("whisper-tiny"));
            assert!(body.contains("response_format"));
            assert!(body.contains("verbose_json"));
            assert!(body.contains("name=\"language\""));
            assert!(body.contains("name=\"file\""));

            let payload = r#"{"text":"hello there","language":"en","duration":1.5}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );

            socket.write_all(response.as_bytes()).expect("response");
        });
        let directory = tempfile::tempdir().expect("temp dir");
        let file = directory.path().join("sample.wav");

        fs::write(&file, wav_bytes(&vec![0_i16; 16_000], 16_000)).expect("audio");

        let endpoint = verified_fixture_endpoint(&format!("http://{address}"));
        let payload = transcribe(&endpoint, "whisper-tiny", &file, Some("en"), 0.0)
            .await
            .expect("transcription");

        assert_eq!(payload.text, "hello there");
        assert_eq!(payload.language.as_deref(), Some("en"));
        assert_eq!(payload.duration_ms, Some(1_500));
        server.join().expect("server");
    }

    #[tokio::test]
    async fn transcribing_maps_a_missing_model_to_the_typed_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("connection");
            let mut buffer = [0_u8; 4096];
            let _ = socket.read(&mut buffer);
            let body = r#"{"error":"model not loaded"}"#;
            let response = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );

            socket.write_all(response.as_bytes()).expect("response");
        });
        let directory = tempfile::tempdir().expect("temp dir");
        let file = directory.path().join("sample.wav");

        fs::write(&file, wav_bytes(&[0_i16; 1_600], 16_000)).expect("audio");

        let endpoint = verified_fixture_endpoint(&format!("http://{address}"));
        let error = transcribe(&endpoint, "whisper-tiny", &file, None, 0.0)
            .await
            .expect_err("missing model");

        assert_eq!(error.code(), "ModelNotLoaded");
        server.join().expect("server");
    }

    fn verified_fixture_endpoint(
        endpoint: &str,
    ) -> super::super::local_inference::VerifiedLocalEndpoint {
        use super::super::local_inference::{
            RuntimeBinding, RuntimeObservation, VerifiedLocalEndpoint,
        };
        let binding = RuntimeBinding::default();
        binding.replace(Some(
            RuntimeObservation::new(
                endpoint,
                std::process::id(),
                std::env::current_exe().expect("test executable"),
                1,
            )
            .expect("fixture observation"),
        ));
        VerifiedLocalEndpoint::verify(&binding).expect("native fixture proof")
    }
}
