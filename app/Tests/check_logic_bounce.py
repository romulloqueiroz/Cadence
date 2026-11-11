"""Check the eight-note pipeline-validation.mid bounce without audio dependencies."""
import argparse
import array
import json
import struct
import sys
from pathlib import Path


def analyse(path):
    blob = path.read_bytes()
    if blob[:4] != b'RIFF' or blob[8:12] != b'WAVE':
        raise ValueError('Expected a RIFF WAVE file')
    chunks = {}
    offset = 12
    while offset + 8 <= len(blob):
        size = struct.unpack_from('<I', blob, offset + 4)[0]
        chunks[blob[offset:offset + 4]] = blob[offset + 8:offset + 8 + size]
        offset += 8 + size + (size & 1)
    fmt, channels, rate, _, _, bits = struct.unpack_from('<HHIIHH', chunks[b'fmt '])
    if (fmt, channels, rate, bits) != (3, 2, 48000, 32):
        raise ValueError('Use 48 kHz stereo 32-bit float WAVE, normalization off')
    samples = array.array('f')
    samples.frombytes(chunks[b'data'])
    if sys.byteorder != 'little':
        samples.byteswap()
    segments = []
    start = None
    last = 0
    for frame, value in enumerate(samples[::channels]):
        if abs(value) > 1e-5:
            if start is None:
                start = frame
            last = frame
        elif start is not None and frame - last > 100:
            segments.append([start, last + 1])
            start = None
    if start is not None:
        segments.append([start, last + 1])
    expected = [i * rate * 2 for i in range(8)]
    errors = [segment[0] - target for segment, target in zip(segments, expected)]
    return {
        'file': str(path), 'frames': len(samples) // channels,
        'sample_rate': rate, 'channels': channels,
        'peak': max(map(abs, samples), default=0),
        'active_segments_frames': segments,
        'expected_note_on_frames': expected, 'onset_error_frames': errors,
        'pass': len(samples) == rate * channels * 16 and len(segments) == 8
                and all(abs(error) <= 1 for error in errors),
        'method': 'Amplitude >1e-5, merging gaps of up to 100 frames; onset tolerance 1 frame',
    }


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('wav', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    result = analyse(args.wav)
    encoded = json.dumps(result, indent=2) + '\n'
    if args.output:
        args.output.write_text(encoded)
    print(encoded, end='')
    raise SystemExit(0 if result['pass'] else 1)
