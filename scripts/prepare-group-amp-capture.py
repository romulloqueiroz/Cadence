#!/usr/bin/env python3
"""Build and identity-check native per-voice group amp placement candidates.

Preserves one original Piano Uno zone covering note60, its sample reference and selected
factory group's non-FX data. Program inserts/sends/master and scripts are cleared
using the existing preserved-bank builder. JUMP occupies physical group slot3;
group volume, pan, amplifier boundary and optional zone duplication change.
External filter-target routes orphaned by removing factory filters are removed;
amplitude, velocity-volume, pitch and internal envelope bytes are retained.
Original native sample references are preserved. An artifact-local Content
symlink resolves those same installed files for the standalone Cadence loader.
Candidate generation does not establish native acceptance or audio parity.
"""
from __future__ import annotations

import argparse
import collections
import importlib.util
import json
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("jump_capture", Path(__file__).with_name("prepare-jump-capture.py"))
capture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(capture)
b, v = capture.b, capture.v
SOURCE_NKI = Path('/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki')
ARRAY_MARKER = b'\0\x13\0\x08\0\0\0'


def group_fx(private):
    starts = [i for i in range(len(private)) if private.startswith(ARRAY_MARKER, i)]
    if len(starts) != 1:
        raise ValueError(f'expected one framed group FX array, found {len(starts)}')
    start = starts[0]
    at = start + len(ARRAY_MARKER)
    slots = []
    for index in range(8):
        present = private[at]
        at += 1
        if present not in (0, 1):
            raise ValueError('invalid group FX presence flag')
        if not present:
            continue
        size = b.u32(private, at + 2)
        objects = b.ser_all(private[at:at + 6 + size])
        if len(objects) != 1:
            raise ValueError('invalid group FX object framing')
        obj = objects[0]
        at += 6 + size
        children = b.ser_all(obj.sections()[3])
        slots.append({'slot': index, 'private_hex': obj.sections()[1].hex(),
                      'children': [{'type': child.kind, 'body_hex': child.body.hex()} for child in children]})
    return start, at, slots


def jump_array(enabled):
    if not enabled:
        return capture.empty_array(8)
    raw = b'\0' + struct.pack('<H6f2B', 0x10, *([0.5] * 6), 0, 0)
    child = b.Ser(77, raw)
    slot = capture.structured(0x25, 0x50, struct.pack('<iBiBffi', 31, 0, 0, 0, 1.0, 0.0, -1), children=child.encode())
    return ARRAY_MARKER + b'\0' * 3 + b'\1' + slot.encode() + b'\0' * 4



FILTER_TARGETS = frozenset({
    'filterCutoff', 'filterQ', 'filterBandwidth', 'formantTalk', 'formantSize',
    'formantSharp', 'cutoffLP', 'cutoffHP', 'resoLP', 'resoHP', 'filterFeedback', 'filterFM',
})


def parameter_array(data):
    if data[:3] != b'\0\x13\0':
        raise ValueError('unsupported parameter-array framing')
    count = b.u32(data, 3)
    if count > 64:
        raise ValueError('parameter-array exceeds 64 slots')
    at, objects = 7, []
    for slot in range(count):
        present = data[at]
        at += 1
        if present not in (0, 1):
            raise ValueError('invalid parameter-array presence flag')
        obj = None
        if present:
            size = b.u32(data, at + 2)
            framed = b.ser_all(data[at:at + 6 + size])
            if len(framed) != 1:
                raise ValueError('invalid parameter-array object')
            obj = framed[0]
            at += 6 + size
        objects.append(obj)
    if at != len(data):
        raise ValueError('trailing parameter-array data')
    return objects


def external_target(obj):
    if obj.kind != 0x0c:
        raise ValueError('unexpected object in external modulation array')
    version, private, public, children = obj.sections()
    if not 0x100 <= version <= 0x105 or public or children:
        raise ValueError('unsupported external modulation fixture layout')
    # These factory objects each have one target. Do not drop an entire mixed
    # route object if a future donor also routes amplitude/pitch on that object.
    if b.u32(private) != 1:
        raise ValueError('controlled fixture requires single-target external routes')
    length = b.u32(private, 4)
    if length > 4096 or 8 + length > len(private):
        raise ValueError('invalid external target name')
    return private[8:8 + length].decode('utf-8')


def external_routes(group):
    records = []
    for array in b.ser_all(group.sections()[3]):
        if array.kind != 0x3c:
            continue
        for slot, obj in enumerate(parameter_array(array.body)):
            if obj is not None:
                records.append({'slot': slot, 'target': external_target(obj),
                                'serialized_sha256': b.sha(obj.encode())})
    return records


def remove_orphaned_filter_routes(group):
    # Every factory filter was removed from the controlled chain. JUMP is not a
    # filter target. Preserve physical modulation slot numbers and every byte
    # of the remaining external routes and all internal/envelope arrays.
    children = b.ser_all(group.sections()[3])
    removed, retained = [], []
    for index, array in enumerate(children):
        if array.kind != 0x3c:
            continue
        objects = parameter_array(array.body)
        for slot, obj in enumerate(objects):
            if obj is None:
                continue
            target = external_target(obj)
            record = {'slot': slot, 'target': target, 'serialized_sha256': b.sha(obj.encode())}
            if target in FILTER_TARGETS:
                removed.append(record)
                objects[slot] = None
            else:
                retained.append(record)
        body = array.body[:7] + b''.join(b'\0' if obj is None else b'\1' + obj.encode() for obj in objects)
        children[index] = b.Ser(array.kind, body)
    return group.replace_sections(children=b''.join(c.encode() for c in children)), {
        'removed_orphaned_external_filter_routes': removed,
        'retained_external_routes': retained,
        'internal_envelope_arrays_preserved': True,
    }

def modify(source, split, voices, enabled, volume, pan):
    item, used = b.parse_item(source)
    assert used == len(source)
    node, preset = b.preset_item(item)
    top = b.ser_all(preset)
    program = v.extract_programs(top)[0]['program']
    children = b.ser_all(program.sections()[3])
    groups = v.array_objects(next(n for n in children if n.kind == 0x33))
    zones = v.array_objects(next(n for n in children if n.kind == 0x34), True)
    assert len(groups) == 1 and len(zones) == 1
    group = groups[0][1]
    private = group.sections()[1]
    start, end, _ = group_fx(private)
    private = private[:start] + jump_array(enabled) + private[end:]
    public = bytearray(group.sections()[2])
    at = 4 + b.u32(public) * 2
    struct.pack_into('<2f', public, at, volume, pan)
    struct.pack_into('<i', public, at + 26, split)
    group = group.replace_sections(private=private, public=bytes(public))
    group, modulation_changes = remove_orphaned_filter_routes(group)
    for index, child in enumerate(children):
        if child.kind == 0x33:
            children[index] = b.Ser(child.kind, b.p32(1) + group.body)
        elif child.kind == 0x34:
            children[index] = b.Ser(child.kind, b.p32(voices) + (b.p32(0) + zones[0][1].body) * voices)
    program = program.replace_sections(children=b''.join(n.encode() for n in children))
    top = capture.replace_program(top, program)
    output = b''.join(n.encode() for n in top)
    old, old_length = node.payload, b.u32(node.payload, 12)
    node.payload = old[:12] + b.p32(len(output)) + old[16:20] + output + old[20 + old_length:]
    return item.encode(), modulation_changes




def select_zone(source, group_name):
    item, used = b.parse_item(source)
    assert used == len(source)
    node, preset = b.preset_item(item)
    top = b.ser_all(preset)
    program = v.extract_programs(top)[0]['program']
    children = b.ser_all(program.sections()[3])
    groups = v.array_objects(next(n for n in children if n.kind == 0x33))
    zones = v.array_objects(next(n for n in children if n.kind == 0x34), True)
    selected = []
    for index, (group_index, zone) in enumerate(zones):
        public = zone.sections()[2]
        vl, vh, kl, kh = struct.unpack_from('<4H', public, 12)
        if kl <= 60 <= kh and vl <= 100 <= vh and v.text_at(groups[group_index][1].sections()[2]) == group_name:
            selected.append((index, group_index, zone))
    if len(selected) != 1:
        raise ValueError(f'expected one note60/velocity100 zone in {group_name!r}, got {len(selected)}')
    index, group_index, zone = selected[0]
    for i, child in enumerate(children):
        if child.kind == 0x33:
            children[i] = b.Ser(child.kind, b.p32(1) + groups[group_index][1].body)
        elif child.kind == 0x34:
            children[i] = b.Ser(child.kind, b.p32(1) + b.p32(0) + zone.body)
    program = program.replace_sections(children=b''.join(n.encode() for n in children))
    top = capture.replace_program(top, program)
    output = b''.join(n.encode() for n in top)
    old, old_length = node.payload, b.u32(node.payload, 12)
    node.payload = old[:12] + b.p32(len(output)) + old[16:20] + output + old[20 + old_length:]
    return item.encode(), {'original_zone_index': index, 'original_group': group_index,
                           'original_group_name': group_name, 'root': b.u16(zone.sections()[2], 28),
                           'original_attachment_retained': True}

def inspect(path):
    data = path.read_bytes()
    state = v.inspect_state(path, SOURCE_NKI)
    item, used = b.parse_item(data)
    assert used == len(data)
    _, preset = b.preset_item(item)
    top = b.ser_all(preset)
    program = v.extract_programs(top)[0]['program']
    children = b.ser_all(program.sections()[3])
    groups = []
    for _, group in v.array_objects(next(n for n in children if n.kind == 0x33)):
        public = group.sections()[2]
        at = 4 + b.u32(public) * 2
        _, _, fx = group_fx(group.sections()[1])
        groups.append({'name': v.text_at(public), 'volume_pan_tune': list(struct.unpack_from('<3f', public, at)),
                       'amp_split_point': struct.unpack_from('<i', public, at + 26)[0], 'fx': fx,
                       'external_routes': external_routes(group)})
    program_public = program.sections()[2]
    at = 4 + b.u32(program_public) * 2
    controls = list(struct.unpack_from('<3f', program_public, at + 9))
    arrays = [n.body.hex() for n in children if n.kind == 0x3a]
    return {'sha256': b.sha(data), 'groups': groups, 'program_volume_pan_tune': controls,
            'program_fx_arrays': arrays, 'state': state}


def validate(candidate_path, returned_path, report_path):
    wanted, actual = inspect(candidate_path), inspect(returned_path)
    errors = []
    for key in ['groups', 'program_volume_pan_tune', 'program_fx_arrays']:
        if wanted[key] != actual[key]:
            errors.append(key + ' differs')
    wp, ap = wanted['state']['programs'], actual['state']['programs']
    if len(wp) != len(ap):
        errors.append('program count differs')
    else:
        for expected, observed in zip(wp, ap):
            if expected['name'] != observed['name']:
                errors.append('program name differs')
            if collections.Counter(map(v.mapping, expected['zones'])) != collections.Counter(map(v.mapping, observed['zones'])):
                errors.append('zone/sample mapping differs')
    report = {'status': 'confirmed' if not errors else 'rejected', 'errors': errors,
              'candidate': str(candidate_path.resolve()), 'candidate_sha256': wanted['sha256'],
              'returned': str(returned_path.resolve()), 'returned_sha256': actual['sha256'],
              'groups': actual['groups'], 'program_volume_pan_tune': actual['program_volume_pan_tune'],
              'program_fx_arrays': actual['program_fx_arrays'],
              'zones': [len(p['zones']) for p in ap],
              'unique_zone_assets': [len({z['asset_key'] for z in p['zones']}) for p in ap],
              'zone_assets': [sorted({z['asset_key'] for z in p['zones']}) for p in ap],
              'zone_sample_paths': [[actual['state']['filename_table'][index]['paths']
                                     for index in sorted({z['filename_id'] for z in p['zones']})] for p in ap],
              'limitations': ['state identity alone does not prove execution or audio parity',
                              'two-voice variants duplicate one original zone at the same MIDI note, rather than scheduling two different notes']}
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    return bool(errors)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--donor', type=Path, default=ROOT / 'app/build/verification/native-van51-2026-11-09/unity-dry-candidate.nkm')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--group-name', default='Grand Piano')
    parser.add_argument('--volume', type=float, default=0.25)
    parser.add_argument('--pan', type=float, default=0.35)
    parser.add_argument('--validate', nargs=2, type=Path, metavar=('CANDIDATE', 'RETURNED'))
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    if args.validate:
        if args.report is None:
            parser.error('--validate requires --report')
        return validate(*args.validate, args.report)
    if args.output is None:
        parser.error('--output required for generation')
    if not 0 < args.volume <= 1 or not -1 <= args.pan <= 1:
        parser.error('expected volume in (0,1] and pan in [-1,1]')
    args.output.mkdir(parents=True, exist_ok=True)
    content_root = next(parent for parent in SOURCE_NKI.parents if parent.name == 'Content')
    content_link = args.output / 'Content'
    if content_link.exists() or content_link.is_symlink():
        if not content_link.is_symlink() or content_link.resolve() != content_root:
            raise ValueError('existing output Content path does not match original installed content')
    else:
        content_link.symlink_to(content_root, target_is_directory=True)
    sample_paths = {'policy': 'original native relative sample/resource references and filename metadata retained; artifact-local Content symlink resolves the same installed files for Cadence',
                    'content_link': str(content_link.absolute()),
                    'content_target': str(content_root)}
    source = args.donor.read_bytes()
    normalized = capture.build_preserved(source, False)
    one_zone, selected = select_zone(normalized, args.group_name)
    cases = []
    for voices in [1, 2]:
        for name, split, enabled in [('dry', 6, False), ('post', 0, True), ('pre', 8, True)]:
            data, modulation_changes = modify(one_zone, split, voices, enabled, args.volume, args.pan)
            path = args.output / f'{name}-{voices}voice-candidate.nkm'
            path.write_bytes(data)
            check = inspect(path)
            assert len(check['groups']) == 1 and len(check['state']['programs'][0]['zones']) == voices
            cases.append({'name': f'{name}-{voices}voice', 'path': str(path.resolve()), 'sha256': b.sha(data),
                          'amp': 'JUMP' if enabled else None, 'slot': 3 if enabled else None,
                          'split': split, 'voices_per_note': voices, 'identity': check['groups'],
                          'modulation_changes': modulation_changes})
    manifest = {'status': 'candidates_generated_native_acceptance_unverified',
                'source_state': str(args.donor.resolve()), 'source_sha256': b.sha(source),
                'selected_factory_zone': selected, 'group_volume': args.volume, 'group_pan': args.pan,
                'sample_reference_resolution': sample_paths,
                'amp': {'kind': 'JUMP', 'fix_type': 31, 'child_type': 77, 'values': [0.5] * 6, 'flags': [False, False]},
                'cases': cases,
                'capture_schedule': {'note': 60, 'velocity': 100, 'note_on_frame': 4800,
                                     'note_off_frame': 292800, 'frames': 480000, 'rate': 48000, 'block': 256},
                'limitations': ['native capture host has a fixed single-note schedule',
                                'polyphony is two duplicate retained zones at that same note',
                                'internal envelopes and non-FX external routes remain byte-preserved factory data; orphaned external filter routes are removed',
                                'this generator does not establish native acceptance, DSP execution, or audio parity']}
    (args.output / 'candidate-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
