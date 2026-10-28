"""Generates Murmur's three UI sounds (spec 6.5) into src-tauri/sounds.

    python scripts/gen_sounds.py

44.1 kHz, 16 bit mono. Needs numpy. The WAVs are committed, so this only
needs rerunning when the sounds change.
"""

import pathlib
import wave

import numpy as np

RATE = 44_100
OUT = pathlib.Path(__file__).resolve().parent.parent / "src-tauri" / "sounds"


def tone(freq_hz, ms, attack_ms=10, release_ms=40, gain=0.5):
    """A sine tone with a linear attack and a smooth (cosine) release."""
    n = int(RATE * ms / 1000)
    t = np.arange(n) / RATE
    wave_ = np.sin(2 * np.pi * freq_hz * t)
    env = np.ones(n)
    a = int(RATE * attack_ms / 1000)
    r = int(RATE * release_ms / 1000)
    env[:a] = np.linspace(0.0, 1.0, a, endpoint=False)
    env[n - r :] = 0.5 * (1 + np.cos(np.linspace(0, np.pi, r)))
    return wave_ * env * gain


def write(name, samples):
    OUT.mkdir(parents=True, exist_ok=True)
    pcm = np.clip(samples, -1.0, 1.0)
    pcm = (pcm * 32767).astype("<i2")
    with wave.open(str(OUT / name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.tobytes())
    print(f"{name}: {len(samples) / RATE * 1000:.0f} ms")


def main():
    low = tone(740, 70)
    high = tone(988, 70)
    write("start.wav", np.concatenate([low, high]))
    write("stop.wav", np.concatenate([high, low]))
    # Single lower tone, quieter than the others.
    write("cancel.wav", tone(440, 90, gain=0.3))


if __name__ == "__main__":
    main()
