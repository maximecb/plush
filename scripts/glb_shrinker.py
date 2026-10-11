#!/usr/bin/env python3

# Shrink a GLB model: drop animations by name, halve the keyframe rate of
# the rest, and pack the animation data into one buffer view.

import argparse
import json
import struct
import sys

JSON_CHUNK = 0x4E4F534A
BIN_CHUNK = 0x004E4942

COMP_SIZE = {5120: 1, 5121: 1, 5122: 2, 5123: 2, 5125: 4, 5126: 4}
NUM_COMPS = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT2': 4, 'MAT3': 9, 'MAT4': 16}

# Keep the odd keys: with keys at (k + 1) / 24 s, the kept ones land on
# multiples of 1/12 s, so the clips start and end on the 12fps grid
KEEP_PARITY = 1

# Animations with these in their name keep their keyframe rate
DEFAULT_FULL_RATE = ['walking', 'running']

def read_glb(data):
    off = 12
    js = None
    bin_chunk = b''
    while off < len(data):
        length, kind = struct.unpack('<II', data[off:off + 8])
        body = data[off + 8:off + 8 + length]
        if kind == JSON_CHUNK:
            js = json.loads(body)
        elif kind == BIN_CHUNK:
            bin_chunk = body
        off += 8 + length
    return js, bin_chunk

def write_glb(path, js, bin_chunk):
    json_bytes = json.dumps(js, separators=(',', ':')).encode()
    json_bytes += b' ' * (-len(json_bytes) % 4)
    total = 12 + 8 + len(json_bytes) + 8 + len(bin_chunk)
    with open(path, 'wb') as f:
        f.write(struct.pack('<4sII', b'glTF', 2, total))
        f.write(struct.pack('<II', len(json_bytes), JSON_CHUNK) + json_bytes)
        f.write(struct.pack('<II', len(bin_chunk), BIN_CHUNK) + bin_chunk)
    return total

def is_full_rate(name, substrings):
    name = (name or '').lower()
    return any(s.lower() in name for s in substrings)

def shrink(js, bin_chunk, remove_names, full_rate_substrings):
    accs = js['accessors']
    bvs = js['bufferViews']

    def acc_bytes(a):
        assert 'sparse' not in a
        bv = bvs[a['bufferView']]
        esz = COMP_SIZE[a['componentType']] * NUM_COMPS[a['type']]
        assert bv.get('byteStride', esz) == esz
        o = bv.get('byteOffset', 0) + a.get('byteOffset', 0)
        return bin_chunk[o:o + esz * a['count']], esz

    all_anims = js.get('animations', [])
    anims = [an for an in all_anims if an.get('name') not in remove_names]

    # Accessors used outside animations, which must not be decimated
    other_accs = set()
    for m in js.get('meshes', []):
        for p in m['primitives']:
            other_accs.update(p['attributes'].values())
            if 'indices' in p:
                other_accs.add(p['indices'])
            for t in p.get('targets', []):
                other_accs.update(t.values())
    for s in js.get('skins', []):
        if 'inverseBindMatrices' in s:
            other_accs.add(s['inverseBindMatrices'])

    # Accessors of animations that keep their keyframe rate
    full_accs = set()
    for an in anims:
        if is_full_rate(an.get('name'), full_rate_substrings):
            for s in an['samplers']:
                full_accs.update((s['input'], s['output']))

    # Decimated data: old accessor index -> (bytes, new count, keys, per key)
    dec = {}

    def decimate(ai, per, n):
        a = accs[ai]
        assert ai not in other_accs
        assert ai not in full_accs, 'accessor shared with a full rate animation'
        assert a['count'] == n * per
        if ai in dec:
            assert dec[ai][2] == n and dec[ai][3] == per
            return
        raw, esz = acc_bytes(a)
        keep = [k for k in range(n) if k % 2 == KEEP_PARITY]
        out = b''.join(raw[k * per * esz:(k * per + per) * esz] for k in keep)
        dec[ai] = (out, len(keep) * per, n, per)

    for an in anims:
        if is_full_rate(an.get('name'), full_rate_substrings):
            continue
        samplers = an['samplers']
        for s in samplers:
            assert s.get('interpolation', 'LINEAR') != 'CUBICSPLINE'
        big = max(accs[s['input']]['count'] for s in samplers)
        for s in samplers:
            n = accs[s['input']]['count']
            if n == big:
                per = accs[s['output']]['count'] // n
                decimate(s['input'], 1, n)
                decimate(s['output'], per, n)

        # Time range of the decimated clip
        ts = None
        for s in samplers:
            if s['input'] in dec and dec[s['input']][2] == big:
                raw = dec[s['input']][0]
                ts = struct.unpack('<%df' % (len(raw) // 4), raw)
                break
        assert ts is not None

        # Constant two-key tracks span the same range, so that the clip
        # keeps a duration on the 12fps grid
        for s in samplers:
            n = accs[s['input']]['count']
            if n == big:
                continue
            assert n == 2 and s['input'] not in other_accs
            out = accs[s['output']]
            raw, _ = acc_bytes(out)
            assert out['componentType'] == 5126
            fv = struct.unpack('<%df' % (len(raw) // 4), raw)
            half = len(fv) // 2
            assert all(abs(fv[i] - fv[half + i]) < 1e-5 for i in range(half)), \
                'animated track with few keys'
            new_t = struct.pack('<2f', ts[0], ts[-1])
            if s['input'] in dec:
                assert dec[s['input']][0] == new_t
            dec[s['input']] = (new_t, 2, 2, 1)

    # Accessors still referenced
    anim_accs = set()
    for an in anims:
        for s in an['samplers']:
            anim_accs.update((s['input'], s['output']))
    used = other_accs | anim_accs
    anim_accs -= other_accs
    acc_map = {old: new for new, old in enumerate(sorted(used))}

    # Rebuild the buffer from the buffer views that are still needed
    new_bin = bytearray()
    new_bvs = []
    bv_map = {}

    def add_bv(raw, extra=None):
        new_bin.extend(b'\0' * (-len(new_bin) % 4))
        bv = {'buffer': 0, 'byteOffset': len(new_bin), 'byteLength': len(raw)}
        if extra:
            bv.update(extra)
        new_bin.extend(raw)
        new_bvs.append(bv)
        return len(new_bvs) - 1

    def keep_bv(i):
        if i not in bv_map:
            old = bvs[i]
            o = old.get('byteOffset', 0)
            extra = {k: v for k, v in old.items()
                     if k not in ('buffer', 'byteOffset', 'byteLength')}
            bv_map[i] = add_bv(bin_chunk[o:o + old['byteLength']], extra)
        return bv_map[i]

    # Animation accessors are packed into one shared buffer view
    packed = bytearray()
    packed_accs = []
    new_accs = []
    for old in sorted(used):
        a = dict(accs[old])
        if old in dec:
            raw, count, _, _ = dec[old]
            a['count'] = count
            a.pop('min', None)
            a.pop('max', None)
            if a['componentType'] == 5126 and count > 0:
                nc = NUM_COMPS[a['type']]
                vals = struct.unpack('<%df' % (count * nc), raw)
                a['min'] = [min(vals[c::nc]) for c in range(nc)]
                a['max'] = [max(vals[c::nc]) for c in range(nc)]
        if old in anim_accs:
            if old not in dec:
                raw = acc_bytes(a)[0]
            packed.extend(b'\0' * (-len(packed) % 4))
            a['byteOffset'] = len(packed)
            packed.extend(raw)
            packed_accs.append(a)
        else:
            a['bufferView'] = keep_bv(a['bufferView'])
        new_accs.append(a)

    if packed:
        shared_bv = add_bv(bytes(packed))
        for a in packed_accs:
            a['bufferView'] = shared_bv

    for im in js.get('images', []):
        if 'bufferView' in im:
            im['bufferView'] = keep_bv(im['bufferView'])

    # Remap accessor indices
    for m in js.get('meshes', []):
        for p in m['primitives']:
            p['attributes'] = {k: acc_map[v] for k, v in p['attributes'].items()}
            if 'indices' in p:
                p['indices'] = acc_map[p['indices']]
            for t in p.get('targets', []):
                for k in t:
                    t[k] = acc_map[t[k]]
    for s in js.get('skins', []):
        if 'inverseBindMatrices' in s:
            s['inverseBindMatrices'] = acc_map[s['inverseBindMatrices']]
    for an in anims:
        for s in an['samplers']:
            s['input'] = acc_map[s['input']]
            s['output'] = acc_map[s['output']]

    new_bin.extend(b'\0' * (-len(new_bin) % 4))
    js['animations'] = anims
    js['accessors'] = new_accs
    js['bufferViews'] = new_bvs
    js['buffers'] = [{'byteLength': len(new_bin)}]
    return bytes(new_bin)

def main():
    parser = argparse.ArgumentParser(description="Shrink a GLB model")
    parser.add_argument("input", help="input .glb file")
    parser.add_argument("output", help="output .glb file")
    parser.add_argument("--remove-anim", action="append", default=[], metavar="NAME",
                        help="remove the animation with this exact name (can be repeated)")
    parser.add_argument("--keep-full-rate", action="append", metavar="TEXT",
                        help="keep the keyframe rate of animations with TEXT in their name, "
                             "ignoring case (can be repeated, replaces the default: "
                             + ", ".join(DEFAULT_FULL_RATE) + ")")
    args = parser.parse_args()
    if args.keep_full_rate is None:
        args.keep_full_rate = DEFAULT_FULL_RATE

    with open(args.input, 'rb') as f:
        data = f.read()
    js, bin_chunk = read_glb(data)

    names = [an.get('name') for an in js.get('animations', [])]
    for name in args.remove_anim:
        if name not in names:
            listing = "\n".join(f"  {n!r}" for n in names)
            sys.exit(f"error: no animation named {name!r}, available:\n{listing}")

    new_bin = shrink(js, bin_chunk, set(args.remove_anim), args.keep_full_rate)
    total = write_glb(args.output, js, new_bin)

    print(f"{len(data)} -> {total} bytes")
    for an in js.get('animations', []):
        rate = "full rate" if is_full_rate(an.get('name'), args.keep_full_rate) else "halved"
        print(f"kept {an.get('name')!r} ({rate})")

if __name__ == "__main__":
    main()
