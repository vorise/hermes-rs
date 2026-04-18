# Hermes Agent — TTS, Voice Mode & Audio Systems

This document covers the text-to-speech pipeline (6 providers), CLI push-to-talk voice mode, audio capture/playback, and STT dispatch with Whisper hallucination filtering.

---

## Table of Contents

1. [TTS Tool Architecture](#1-tts-tool-architecture)
2. [TTS Providers](#2-tts-providers)
3. [Streaming TTS Pipeline](#3-streaming-tts-pipeline)
4. [Voice Mode (CLI)](#4-voice-mode-cli)
5. [Audio Capture Backends](#5-audio-capture-backends)
6. [Whisper Hallucination Filter](#6-whisper-hallucination-filter)
7. [Audio Playback](#7-audio-playback)

---

## 1. TTS Tool Architecture

### Location

`tools/tts_tool.py` (~1,072 lines)

### Purpose

Convert text to speech audio files via 6 configurable providers. On messaging platforms, the returned `MEDIA:<path>` tag is intercepted by the send pipeline and delivered as a native voice message. In CLI mode, files are saved to `~/voice-memos/`.

### 1.1 Provider Resolution Order

```
1. Config: ~/.hermes/config.yaml → tts.provider
2. Default: "edge" (free, no API key needed)
3. Fallback: if edge-tts not installed → neutts (if available)
```

### 1.2 Output Format Selection

| Platform | Provider | Format |
|----------|----------|--------|
| Telegram | ElevenLabs, OpenAI, Mistral | Native Opus (.ogg) |
| Telegram | Edge, NeuTTS, MiniMax | MP3 → ffmpeg → .ogg |
| All others | Any | MP3 |

Platform detected via `HERMES_SESSION_PLATFORM` env var. Format determined by file extension: `.ogg` triggers Opus-specific API parameters; `.mp3` triggers MP3 output.

### 1.3 Tool Interface

```python
def text_to_speech_tool(
    text: str,
    output_path: Optional[str] = None,
) -> str:
    """Returns JSON with success, file_path, media_tag, provider, voice_compatible."""
```

**Constraints**:
- Max text length: 4,000 characters (truncated with warning)
- Empty text → error
- Text over 4,000 chars → truncated, logged

### 1.4 Response Format

```json
{
  "success": true,
  "file_path": "/path/to/audio.ogg",
  "media_tag": "[[audio_as_voice]]\nMEDIA:/path/to/audio.ogg",
  "provider": "elevenlabs",
  "voice_compatible": true
}
```

When `voice_compatible` is true (Opus .ogg file), the `[[audio_as_voice]]` directive tells the platform adapter to route via `send_voice()` instead of `send_audio()`.

### 1.5 Lazy Import Pattern

All heavy dependencies (edge_tts, elevenlabs, openai, mistralai, sounddevice, numpy) are lazy-imported at call time to avoid crashing in headless environments (SSH, Docker, WSL without PortAudio).

### 1.6 Managed Gateway Integration

OpenAI TTS supports the managed tool gateway pattern:

```
1. VOICE_TOOLS_OPENAI_KEY env var → direct OpenAI API
2. OPENAI_API_KEY env var → direct OpenAI API (default base URL)
3. Managed gateway (nous_user_token + gateway_origin) → proxied
```

Resolution via `resolve_managed_tool_gateway("openai-audio")` and `resolve_openai_audio_api_key()`.

---

## 2. TTS Providers

### 2.1 Edge TTS (Default, Free)

**Package**: `edge-tts` (no API key required)

**Configuration** (`config.yaml → tts.edge`):
| Field | Default | Purpose |
|-------|---------|---------|
| `voice` | `en-US-AriaNeural` | Voice selection |
| `speed` | 1.0 | Playback speed |

**Execution**: Runs in `ThreadPoolExecutor(max_workers=1)` with `asyncio.run()` to handle edge cases where the gateway already has an event loop. 60-second timeout.

**Speed adjustment**: Converted to percentage rate: `speed=1.5` → `rate="+50%"`.

**Output**: MP3 file. Converted to Opus via ffmpeg if Telegram detected.

### 2.2 ElevenLabs (Premium)

**Package**: `elevenlabs` (requires `ELEVENLABS_API_KEY`)

**Configuration** (`config.yaml → tts.elevenlabs`):
| Field | Default | Purpose |
|-------|---------|---------|
| `voice_id` | `pNInz6obpgDQGcFmaJgB` (Adam) | Voice ID |
| `model_id` | `eleven_multilingual_v2` | Generation model |
| `streaming_model_id` | `eleven_flash_v2_5` | Streaming model |

**Output format detection**:
- `.ogg` → `opus_48000_64` (native Opus)
- `.mp3` → `mp3_44100_128`

**Audio generation**: `client.text_to_speech.convert()` yields audio chunks written to file.

### 2.3 OpenAI TTS

**Package**: `openai` (requires API key or managed gateway)

**Configuration** (`config.yaml → tts.openai`):
| Field | Default | Purpose |
|-------|---------|---------|
| `model` | `gpt-4o-mini-tts` | TTS model |
| `voice` | `alloy` | Voice selection |
| `speed` | 1.0 | Speed (0.25–4.0, clamped) |
| `base_url` | `https://api.openai.com/v1` | API endpoint |

**Output format**:
- `.ogg` → `response_format="opus"`
- Others → `response_format="mp3"`

Uses idempotency key (`uuid4`) per request. Client closed after use.

### 2.4 MiniMax TTS

**Package**: `requests` (requires `MINIMAX_API_KEY`)

**Configuration** (`config.yaml → tts.minimax`):
| Field | Default | Purpose |
|-------|---------|---------|
| `model` | `speech-2.8-hd` | Model |
| `voice_id` | `English_Graceful_Lady` | Voice |
| `speed` | 1 | Speed multiplier |
| `vol` | 1 | Volume |
| `pitch` | 0 | Pitch offset |

**API**: POST to `https://api.minimax.io/v1/t2a_v2`

**Key detail**: MiniMax returns **hex-encoded** audio (not base64). Decoded via `bytes.fromhex()`.

**Audio settings**: 32kHz sample rate, 128kbps bitrate, mono channel. Format determined by output extension (wav/flac/mp3).

### 2.5 Mistral Voxtral TTS

**Package**: `mistralai` (requires `MISTRAL_API_KEY`)

**Configuration** (`config.yaml → tts.mistral`):
| Field | Default | Purpose |
|-------|---------|---------|
| `model` | `voxtral-mini-tts-2603` | TTS model |
| `voice_id` | `c69964a6-ab8b-4f8a-9465-ec0925096ec8` (Paul - Neutral) | Voice |

**API**: `client.audio.speech.complete()`

**Key detail**: API returns **base64-encoded** audio. Decoded via `base64.b64decode()`.

**Output formats**: opus, wav, flac, mp3 — determined by file extension.

### 2.6 NeuTTS (Local, On-Device)

**Package**: `neutts` (~500MB model, subprocess-based)

**Availability check**: `importlib.util.find_spec("neutts")`

**Configuration** (`config.yaml → tts.neutts`):
| Field | Default | Purpose |
|-------|---------|---------|
| `ref_audio` | `tools/neutts_samples/jo.wav` | Voice reference audio |
| `ref_text` | `tools/neutts_samples/jo.txt` | Reference transcript |
| `model` | `neuphonic/neutts-air-q4-gguf` | Model path |
| `device` | `cpu` | Device (cpu/cuda) |

**Execution**: Runs synthesis in separate subprocess via `tools/neutts_synth.py` to keep the 500MB model isolated. 120-second timeout.

**Output**: WAV natively. Caller converts to MP3/OGG via ffmpeg if needed.

**Fallback role**: Used when Edge TTS is unavailable and no other provider is configured.

---

## 3. Streaming TTS Pipeline

### Purpose

Real-time sentence-by-sentence TTS via ElevenLabs for CLI voice mode. Consumes text deltas from a queue, buffers into sentences, and plays each as it's generated.

### 3.1 Interface

```python
def stream_tts_to_speaker(
    text_queue: queue.Queue,
    stop_event: threading.Event,
    tts_done_event: threading.Event,
    display_callback: Optional[Callable[[str], None]] = None,
):
```

**Protocol**:
- Producer puts `str` deltas onto `text_queue`
- `None` sentinel = end-of-text (flush buffer)
- `stop_event` = abort early (user interrupt)
- `tts_done_event` = set in `finally` block (playback finished)

### 3.2 Sentence Boundary Detection

```python
_SENTENCE_BOUNDARY_RE = re.compile(r'(?<=[.!?])(?:\s|\n)|(?:\n\n)')
```

Matches: punctuation (`.`, `!`, `?`) followed by space/newline, or double newline.

### 3.3 Accumulation Logic

```
while not stop_event:
  delta = text_queue.get(timeout=0.5s)
  sentence_buf += delta
  strip <think>...</think> blocks from buffer
  if '<think' in buf and '</think>' not in buf: continue  # wait for close

  while sentence boundary found:
    sentence = buf[:boundary]
    buf = buf[boundary:]
    if len(sentence) < 20:  # merge short fragments
      buf = sentence + buf; break
    _speak_sentence(sentence)

  if len(buf) > 100:  # long buffer without boundary → flush
    _speak_sentence(buf); buf = ""
```

### 3.4 Think Block Filtering

Complete `<think>...</think>` blocks are stripped from the buffer using regex with `re.DOTALL`. If an incomplete `<think` tag is at the end of the buffer, sentence extraction is paused until the closing `</think>` arrives.

### 3.5 Duplicate Suppression

Tracks spoken sentences in `_spoken_sentences` list. Before speaking, compares normalized (lowercased, stripped trailing punctuation) against all previously spoken sentences. Skips duplicates.

### 3.6 Audio Output

**Primary**: ElevenLabs `text_to_speech.convert(voice_id, model_id, "pcm_24000")` → sounddevice `OutputStream(samplerate=24000, channels=1, dtype="int16")`.

**Fallback**: If sounddevice unavailable, writes PCM chunks to temp WAV file and plays via `play_audio_file()`.

### 3.7 Markdown Stripping for TTS

Before speaking, markdown is stripped:
- Code blocks → removed
- Links `[text](url)` → `text`
- URLs → removed
- Bold/italic → plain text
- Headers → plain text
- List items → removed
- Horizontal rules → removed
- Triple+ newlines → double newline

---

## 4. Voice Mode (CLI)

### Location

`tools/voice_mode.py` (~1,017 lines)

### Purpose

Push-to-talk audio recording and playback for CLI TUI. Captures microphone audio, detects silence to auto-stop, dispatches to STT, and plays back TTS responses.

### 4.1 Recording Parameters

| Constant | Value | Purpose |
|----------|-------|---------|
| `SAMPLE_RATE` | 16000 Hz | Whisper native sample rate |
| `CHANNELS` | 1 | Mono |
| `DTYPE` | `int16` | 16-bit PCM |
| `SAMPLE_WIDTH` | 2 bytes | Bytes per sample |
| `SILENCE_RMS_THRESHOLD` | 200 | RMS below this = silence |
| `SILENCE_DURATION_SECONDS` | 3.0 | Continuous silence to auto-stop |
| `MAX_WAIT` | 15.0s | Max wait for speech before auto-stop |
| `MIN_SPEECH_DURATION` | 0.3s | Minimum speech to confirm |
| `MAX_DIP_TOLERANCE` | 0.3s | Max brief dip during speech |

### 4.2 Environment Detection

`detect_audio_environment()` returns dict with `available`, `warnings`, `notices`.

**Hard-fail warnings** (block voice mode):
- SSH: `SSH_CLIENT`, `SSH_TTY`, `SSH_CONNECTION` env vars present
- Docker/Podman: `is_container()` returns true
- WSL without PulseAudio: `/proc/version` contains "microsoft" but `PULSE_SERVER` not set
- No audio libraries: sounddevice/numpy not installed, PortAudio missing
- Termux without API app: `termux-microphone-record` exists but `com.termux.api` not installed

**Informational notices** (don't block):
- WSL with PulseAudio bridge: `PULSE_SERVER` is set
- Termux:API microphone available (sounddevice not required)

---

## 5. Audio Capture Backends

### 5.1 AudioRecorder (sounddevice)

**Primary backend** for desktop environments.

**Persistent Stream Pattern**: The `InputStream` is created once in `_ensure_stream()` and kept alive for the lifetime of the recorder. Between recordings, the callback discards audio chunks. This avoids the macOS CoreAudio bug where closing and re-opening an `InputStream` hangs indefinitely.

**Audio Callback** (runs in sounddevice thread):
```python
def _callback(indata, frames, time_info, status):
  if not self._recording: return
  self._frames.append(indata.copy())
  rms = sqrt(mean(indata^2))  # int16 range 0-32767
  self._current_rms = rms
  if rms > self._peak_rms: self._peak_rms = rms

  # Silence detection state machine (see below)
```

**Silence Detection State Machine**:

```
IDLE → rms > threshold → SPEECH_ATTEMPT
SPEECH_ATTEMPT → sustained 0.3s above threshold → SPEECH_CONFIRMED
SPEECH_CONFIRMED → rms < threshold for 3.0s → FIRE_SILENCE_CALLBACK
SPEECH_ATTEMPT → rms < threshold for 0.3s → reset to IDLE
No speech at all for 15s → FIRE_SILENCE_CALLBACK
```

**Dip tolerance**: After speech is confirmed, brief dips below threshold (micro-pauses between syllables) are tolerated up to 0.3s. Sustained dips reset the resume tracker.

**Resume tracking**: After silence timer starts, if speech resumes, a secondary `_resume_start` tracker mirrors the initial speech detection pattern — requires 0.3s sustained speech before clearing the silence timer.

**Thread-safe close**: `_close_stream_with_timeout()` closes the audio stream in a daemon thread with 3.0s timeout, polling `t.join(0.1)` to avoid blocking Ctrl+C. Prevents deadlock with the audio callback (which holds the stream lock).

### 5.2 TermuxAudioRecorder

**Android backend** using Termux:API microphone capture commands.

**Command**: `termux-microphone-record -f {path} -l 0 -e aac -r 16000 -c 1`
**Stop**: `termux-microphone-record -q`

**Differences from AudioRecorder**:
- Records AAC format (not WAV via numpy)
- No live silence detection (Termux:API doesn't expose raw audio stream)
- No RMS tracking (`current_rms` always 0)
- Records to `.aac` files, not `.wav`

**Availability check**: Both `termux-microphone-record` command exists AND `com.termux.api` package is installed.

### 5.3 Backend Selection

```python
def create_audio_recorder():
  if _termux_voice_capture_available():
    return TermuxAudioRecorder()
  return AudioRecorder()
```

### 5.4 Audio Cues (Beep Tones)

```python
def play_beep(frequency=880, duration=0.12, count=1):
```

Generates 880Hz sine wave with fade-in/out (1% of duration) to avoid click artifacts. Gap between beeps: 0.06s. Amplitude: 30% of max (0.3 * 32767).

**Polling wait**: Uses `time.monotonic()` deadline with 2.0s ceiling instead of `sd.wait()` (which calls `Event.wait()` without timeout and can hang forever if audio device stalls).

---

## 6. Whisper Hallucination Filter

### Purpose

Whisper commonly hallucinates specific phrases on silent or near-silent audio. This filter catches and suppresses them.

### 6.1 Known Hallucinations (26 phrases)

**English**: "thank you", "thanks for watching", "subscribe to my channel", "like and subscribe", "please subscribe", "thank you for watching", "bye", "you", "the end"

**Non-English**:
- Russian: "продолжение следует" (to be continued)
- French: "sous-titres", "sous-titres réalisés par la communauté d'amara.org"
- Italian: "sottotitoli creati dalla comunità amara.org"
- German: "untertitel von stephanie geiges"
- Japanese: "ご視聴ありがとうございました"
- Generic: "amara.org", "www.mooji.org"

### 6.2 Detection Logic

```python
def is_whisper_hallucination(transcript: str) -> bool:
  cleaned = transcript.strip().lower()
  if not cleaned: return True  # empty = hallucination
  if cleaned.rstrip('.!') in WHISPER_HALLUCINATIONS: return True
  if _HALLUCINATION_REPEAT_RE.match(cleaned): return True  # repetitive
```

**Repetitive pattern regex**: `^(?:thank you|thanks|bye|you|ok|okay|the end|\.|\s|,|!)+$`

Matches: "Thank you. Thank you. Thank you. you", "bye bye bye", etc.

### 6.3 STT Dispatch

```python
def transcribe_recording(wav_path: str, model: Optional[str] = None):
  result = transcribe_audio(wav_path, model=model)
  if result["success"] and is_whisper_hallucination(result["transcript"]):
    return {"success": True, "transcript": "", "filtered": True}
  return result
```

Filters hallucinations to empty transcript with `"filtered": True` flag.

---

## 7. Audio Playback

### Purpose

Play audio files through the default output device with interrupt support.

### 7.1 Playback Strategy (priority order)

1. **WAV via sounddevice**: `sd.play(audio_data, samplerate=...)` with polling wait (avoids `sd.wait()` hang)
2. **macOS**: `afplay {file}`
3. **Cross-platform**: `ffplay -nodisp -autoexit -loglevel quiet {file}`
4. **Linux ALSA**: `aplay -q {file}`

### 7.2 Interruptible Playback

```python
_active_playback: Optional[subprocess.Popen] = None
_playback_lock = threading.Lock()

def stop_playback():
  with _playback_lock:
    proc = _active_playback; _active_playback = None
  if proc and proc.poll() is None:
    proc.terminate()
  sd.stop()  # also stop sounddevice playback
```

Global reference to active Popen process allows interruption from another thread (e.g., user sends new message while TTS is playing).

### 7.3 sounddevice Wait Fix

```python
# WRONG: sd.wait()  # Event.wait() without timeout — hangs forever
# RIGHT:
deadline = time.monotonic() + duration_secs + 2.0
while sd.get_stream() and sd.get_stream().active and time.monotonic() < deadline:
    time.sleep(0.01)
sd.stop()
```

### 7.4 Temp File Cleanup

```python
def cleanup_temp_recordings(max_age_seconds=3600):
```

Removes old temporary voice recordings from `/tmp/hermes_voice/` older than 1 hour (default).

---

## Key Numbers

| Metric | Value |
|--------|-------|
| TTS providers | 6 (Edge, ElevenLabs, OpenAI, MiniMax, Mistral, NeuTTS) |
| Max TTS text length | 4,000 characters |
| Default TTS provider | Edge TTS |
| Streaming TTS model | eleven_flash_v2_5 |
| ElevenLabs default voice | Adam (pNInz6obpgDQGcFmaJgB) |
| MiniMax audio encoding | Hex (not base64) |
| Mistral audio encoding | Base64 |
| NeuTTS model size | ~500MB |
| NeuTTS subprocess timeout | 120 seconds |
| Edge TTS asyncio timeout | 60 seconds |
| Streaming sentence boundary | `(?<=[.!?])(?:\s|\n)` |
| Streaming min sentence length | 20 chars |
| Streaming long flush length | 100 chars |
| Streaming queue timeout | 0.5s |
| Whisper hallucination phrases | 26 |
| Audio sample rate | 16,000 Hz |
| Audio channels | 1 (mono) |
| Audio bit depth | 16-bit PCM |
| Silence RMS threshold | 200 |
| Silence auto-stop duration | 3.0 seconds |
| Max wait for speech | 15.0 seconds |
| Min speech duration | 0.3 seconds |
| Max dip tolerance | 0.3 seconds |
| Beep frequency | 880 Hz |
| Beep duration | 0.12 seconds |
| Beep gap | 0.06 seconds |
| Beep fade | 1% of duration |
| Playback timeout ceiling | duration + 2.0 seconds |
| Temp cleanup default age | 3,600 seconds (1 hour) |
| Opus conversion timeout | 30 seconds |
| Opus bitrate | 64kbps VBR off |

---

*Generated from source analysis of the Hermes Agent codebase.*
