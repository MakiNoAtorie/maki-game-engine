"""Makes the placeholder sound files of the game.

    python tools/gen_sounds.py

- `click.wav`: a short click, for a switch.
- `hum.wav`: one second of a mains hum that loops with no step.

The files are WAV, one channel, 16 bits, 22050 Hz. They are the same in each run.
Put a real sound of the same name in their place: the room files do not change.
"""

import math
import struct
import wave
from pathlib import Path

RATE = 22050
OUT = Path(__file__).resolve().parent.parent / "game" / "assets" / "rooms" / "sounds"


def write(name, samples):
    """`samples`: numbers from -1.0 to 1.0."""
    OUT.mkdir(parents=True, exist_ok=True)
    data = b"".join(struct.pack("<h", round(max(-1.0, min(1.0, sample)) * 32767)) for sample in samples)
    with wave.open(str(OUT / name), "wb") as file:
        file.setnchannels(1)
        file.setsampwidth(2)
        file.setframerate(RATE)
        file.writeframes(data)
    print(f"{OUT / name}: {len(samples) / RATE:.3f} s")


def click():
    """Two short tones that fall off fast: the contact, then the body of the switch."""
    count = RATE * 60 // 1000
    samples = []
    for index in range(count):
        time = index / RATE
        contact = math.sin(2 * math.pi * 2400 * time) * math.exp(-time * 220)
        body = math.sin(2 * math.pi * 310 * time) * math.exp(-time * 70)
        samples.append(0.7 * contact + 0.4 * body)
    return samples


def hum():
    """50 Hz and its harmonics. All are whole numbers of waves in one second, so the
    end of the file meets its start."""
    parts = [(50, 0.10), (100, 0.30), (150, 0.12), (200, 0.08), (300, 0.04)]
    samples = []
    for index in range(RATE):
        time = index / RATE
        samples.append(sum(level * math.sin(2 * math.pi * hertz * time) for hertz, level in parts))
    return samples


if __name__ == "__main__":
    write("click.wav", click())
    write("hum.wav", hum())
