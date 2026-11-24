#!/usr/bin/env python3

# Convert a textured model to a GLB file with one mesh, one material and its
# texture embedded, for the model loaders under examples/.
#
# Reads Wavefront .obj files (with the texture named in their .mtl, or given
# with --texture) and Blender .blend files, read directly, without Blender:
# 2.4x to 3.4 files, gzip compressed or not. From a .blend, the texture is the
# image the mesh's faces or material use, packed or next to the file, and an
# Edge Split modifier or auto smooth gives hard edges; no other modifier is
# applied. Blender 3.5 and later, and zstd compressed files, are not read:
# export those to .obj. Images that materials only use as normal maps are not
# taken as the texture.
#
# A .blend mesh deformed by an armature is written with its skeleton and the
# armature's actions as animations, posed as Blender poses them, constraints
# included (see the section on armatures for which). --action picks actions,
# --static leaves them out, and --skin-from gives a mesh with no weights those
# of another, such as a low poly version of a rigged mesh.
#
# Textures larger than --max-tex-size are box filtered down to fit, and are
# stored as baseline JPEG by default, which lib/std/jpeg.psh decodes, or as
# PNG with --tex-format png or when they have transparency. A JPEG that fits
# already is kept as it is. JPEGs are decoded, to be scaled down, with
# lib/std/jpeg.psh, which needs plush built.
#
# With --merge, adjacent faces that are close to coplanar, with texturing and
# shading that carry across, are merged and cut up again from their outline,
# which changes nothing that can be seen.
#
# The model can be turned so that given axes of the source become up (+Y) and
# front (+Z), then rotated further, scaled and centred. Axes are named in
# glTF's frame, Y up, which a .blend is converted to first.
#
# Usage:
#   model_to_glb.py key.blend key.glb --max-tex-size 128 --ignore-object-rotation \
#       --front=+y --up=-z --rotate z:120 --center
#   model_to_glb.py keycard.obj keycard.glb --max-tex-size 256 --front=+x --up=-y --center
#   model_to_glb.py chickenV2.blend chicken.glb --object Cylinder --skin-from Chicken \
#       --drop-alpha --center --action Walk --action Idle1
#
# Negative axes need the = form, --up=-z, or they read as another option.

import argparse
import gzip
import json
import math
import os
import re
import struct
import subprocess
import sys
import tempfile
import zlib

# --- Blender files ---

class Blend:
    """The file blocks of a .blend file, and the SDNA that describes them"""

    PRIM = {'char': 'b', 'uchar': 'B', 'short': 'h', 'ushort': 'H', 'int': 'i', 'float': 'f',
            'double': 'd', 'int64_t': 'q', 'uint64_t': 'Q', 'int8_t': 'b', 'uint8_t': 'B',
            'int16_t': 'h', 'uint16_t': 'H', 'int32_t': 'i', 'uint32_t': 'I'}

    def __init__(self, path):
        d = open(path, 'rb').read()
        if d[:2] == b'\x1f\x8b':
            d = gzip.decompress(d)
        if d[:4] == b'\x28\xb5\x2f\xfd':
            sys.exit(path + ': zstd compressed .blend files are not supported, save it uncompressed')
        if d[:7] != b'BLENDER':
            sys.exit(path + ': not a .blend file')
        if d[7:8] not in (b'-', b'_'):
            sys.exit(path + ': this .blend header format (Blender 5+?) is not supported')
        self.psize = 8 if d[7:8] == b'-' else 4
        self.end = '<' if d[8:9] == b'v' else '>'
        self.version = int(d[9:12])
        self.data = d
        self.blocks = []
        self.by_ptr = {}
        pfmt = 'Q' if self.psize == 8 else 'I'
        hfmt = self.end + '4si' + pfmt + 'ii'
        hsize = struct.calcsize(hfmt)
        pos = 12
        while pos + hsize <= len(d):
            code, size, old, sdna, count = struct.unpack_from(hfmt, d, pos)
            code = code.rstrip(b'\0').decode('latin1')
            blk = dict(code=code, size=size, sdna=sdna, count=count, off=pos + hsize)
            pos += hsize + size
            if code == 'ENDB':
                break
            self.blocks.append(blk)
            if old:
                self.by_ptr[old] = blk
            if code == 'DNA1':
                self.parse_sdna(blk['off'])

    def parse_sdna(self, off):
        d = self.data
        e = self.end

        def strings(tag, p):
            assert d[p:p + 4] == tag
            n = struct.unpack_from(e + 'i', d, p + 4)[0]
            p += 8
            out = []
            for _ in range(n):
                q = d.index(b'\0', p)
                out.append(d[p:q].decode('latin1'))
                p = q + 1
            return out, (p + 3) & ~3

        assert d[off:off + 4] == b'SDNA'
        names, p = strings(b'NAME', off + 4)
        types, p = strings(b'TYPE', p)
        assert d[p:p + 4] == b'TLEN'
        tlen = struct.unpack_from(e + '%dh' % len(types), d, p + 4)
        p = (p + 4 + 2 * len(types) + 3) & ~3
        assert d[p:p + 4] == b'STRC'
        num = struct.unpack_from(e + 'i', d, p + 4)[0]
        p += 8
        self.structs = []
        self.struct_by_name = {}
        for _ in range(num):
            t, nf = struct.unpack_from(e + 'hh', d, p)
            p += 4
            fields = {}
            off = 0
            for _ in range(nf):
                ft, fn = struct.unpack_from(e + 'hh', d, p)
                p += 4
                name = names[fn]
                dims = [int(x) for x in re.findall(r'\[(\d+)\]', name)]
                count = 1
                for x in dims:
                    count *= x
                is_ptr = name.startswith('*') or name.startswith('(*')
                size = self.psize if is_ptr else tlen[ft]
                base = re.sub(r'\[.*', '', name).replace('(', '').replace(')', '').lstrip('*')
                fields[base] = dict(type=types[ft], off=off, ptr=is_ptr, dims=dims, count=count)
                off += size * count
            s = dict(name=types[t], fields=fields, size=tlen[t])
            self.structs.append(s)
            self.struct_by_name[s['name']] = s

    def has(self, sname, fname):
        return sname in self.struct_by_name and fname in self.struct_by_name[sname]['fields']

    def size_of(self, sname):
        return self.struct_by_name[sname]['size']

    def read(self, sname, off, fname):
        """A field of a struct at a file offset: a number, a list for an array,
        a string for a char array, or the offset of an embedded struct"""
        f = self.struct_by_name[sname]['fields'][fname]
        at = off + f['off']
        if f['ptr']:
            fmt = 'Q' if self.psize == 8 else 'I'
        elif f['type'] == 'char' and f['dims']:
            return self.data[at:at + f['count']].split(b'\0')[0].decode('latin1')
        elif f['type'] in self.PRIM:
            fmt = self.PRIM[f['type']]
        else:
            return at
        vals = struct.unpack_from(self.end + str(f['count']) + fmt, self.data, at)
        return vals[0] if f['count'] == 1 else list(vals)

    def block_at(self, ptr):
        return self.by_ptr.get(ptr) if ptr else None

    def id_name(self, sname, off):
        return self.read('ID', self.read(sname, off, 'id'), 'name')[2:]

def blender_to_gltf(v):
    # Blender is Z up, glTF Y up
    return [v[0], v[2], -v[1]]

def blend_image_data(b, im, blend_path):
    """The bytes of an image, packed or in a file next to the .blend, or None"""
    pf = None
    if b.has('Image', 'packedfile'):
        pf = b.block_at(b.read('Image', im, 'packedfile'))
    if pf is None and b.has('Image', 'packedfiles'):
        first = b.block_at(b.read('ListBase', b.read('Image', im, 'packedfiles'), 'first'))
        if first:
            pf = b.block_at(b.read('ImagePackedFile', first['off'], 'packedfile'))
    if pf:
        size = b.read('PackedFile', pf['off'], 'size')
        data = b.block_at(b.read('PackedFile', pf['off'], 'data'))
        if data:
            return b.data[data['off']:data['off'] + size]
    name = b.read('Image', im, 'name').replace('\\', '/')
    if name.startswith('//'):
        name = os.path.join(os.path.dirname(blend_path), name[2:])
    if name and os.path.isfile(name):
        return open(name, 'rb').read()
    return None

def blend_mesh_images(b, me):
    """Offsets of the images a mesh is textured with, most likely first: the
    one assigned to its faces, then the colour textures of its material"""
    out = []
    for field, sname in (('mtpoly', 'MTexPoly'), ('mtface', 'MTFace')):
        if b.has('Mesh', field):
            blk = b.block_at(b.read('Mesh', me, field))
            if blk:
                im = b.block_at(b.read(sname, blk['off'], 'tpage'))
                if im:
                    out.append(im['off'])
    mats = b.block_at(b.read('Mesh', me, 'mat'))
    if mats and b.has('Material', 'mtex'):
        pfmt = b.end + ('Q' if b.psize == 8 else 'I')
        for k in range(b.read('Mesh', me, 'totcol')):
            ma = b.block_at(struct.unpack_from(pfmt, b.data, mats['off'] + k * b.psize)[0])
            if ma is None:
                continue
            mtex = b.read('Material', ma['off'], 'mtex')
            for slot in (mtex if isinstance(mtex, list) else [mtex]):
                mt = b.block_at(slot)
                if mt is None or not (b.read('MTex', mt['off'], 'mapto') & 1):
                    continue
                tex = b.block_at(b.read('MTex', mt['off'], 'tex'))
                im = b.block_at(b.read('Tex', tex['off'], 'ima')) if tex else None
                if im:
                    out.append(im['off'])
    return out

def blend_normal_maps(b):
    """Offsets of the images that materials only use as normal maps, which
    faces can still name as their image"""
    colour, normal = set(), set()
    if not b.has('Material', 'mtex'):
        return normal
    for blk in b.blocks:
        if blk['code'] != 'MA':
            continue
        mtex = b.read('Material', blk['off'], 'mtex')
        for slot in (mtex if isinstance(mtex, list) else [mtex]):
            mt = b.block_at(slot)
            tex = b.block_at(b.read('MTex', mt['off'], 'tex')) if mt else None
            im = b.block_at(b.read('Tex', tex['off'], 'ima')) if tex else None
            if im is None:
                continue
            # Mapped to colour, or a texture flagged as a normal map
            if b.read('MTex', mt['off'], 'mapto') & 1:
                colour.add(im['off'])
            elif b.read('MTex', mt['off'], 'mapto') & 2 or b.read('Tex', tex['off'], 'imaflag') & 2048:
                normal.add(im['off'])
    return normal - colour

def blend_texture(b, path, meshes, image_name):
    """The texture as (bytes, name): the one asked for by name, else the
    mesh's, else one named like a diffuse map, else the first image"""
    images = [blk['off'] for blk in b.blocks if blk['code'] == 'IM']
    if image_name:
        images = [im for im in images if b.id_name('Image', im) == image_name]
        if not images:
            sys.exit('%s: no image named %r' % (path, image_name))
    else:
        named = [im for im in images if re.search(r'diff|albedo|color|colour|base', b.id_name('Image', im), re.I)]
        found = [im for me in meshes for im in blend_mesh_images(b, me)]
        normal = blend_normal_maps(b)
        images = [im for im in found + named + images if im not in normal]
    for im in images:
        data = blend_image_data(b, im, path)
        if data:
            return data, b.id_name('Image', im)
    return None, None

def blend_split_angle(b, o, me):
    """The angle above which an edge is sharp, in radians, whether marked
    sharp edges split, from an Edge Split modifier or auto smooth; or None"""
    ptr = b.read('ListBase', b.read('Object', o, 'modifiers'), 'first')
    while ptr:
        md = b.block_at(ptr)
        if md is None:
            break
        if b.read('ModifierData', md['off'], 'type') == 13:
            angle = b.read('EdgeSplitModifierData', md['off'], 'split_angle')
            flags = b.read('EdgeSplitModifierData', md['off'], 'flags')
            # Older files hold degrees
            if angle > 3.2:
                angle = math.radians(angle)
            return (angle if flags & 2 else math.pi, bool(flags & 4))
        ptr = b.read('ModifierData', md['off'], 'next')
    if b.has('Mesh', 'smoothresh') and b.read('Mesh', me, 'flag') & 32:
        angle = b.read('Mesh', me, 'smoothresh')
        if b.struct_by_name['Mesh']['fields']['smoothresh']['type'] == 'short':
            angle = math.radians(angle)
        return (angle, True)
    return None

def load_blend(path, ignore_object_rotation, object_names, image_name, animate=True, action_names=(),
               skin_from=None):
    """Polygons of the mesh objects, as lists of corners (pos, normal or
    None, uv, vertex id), in glTF's frame, the texture as (bytes, name), and
    the skin and animations from the armature deforming them, or None"""
    b = Blend(path)
    if not b.has('Mesh', 'mvert') and b.has('Mesh', 'vdata'):
        sys.exit(path + ': Blender 3.5+ meshes are not supported, export to .obj')
    polys = []
    vid_base = 0
    meshes = []
    names = []
    rigged = []
    for blk in b.blocks:
        if blk['code'] != 'OB' or b.read('Object', blk['off'], 'type') != 1:
            continue
        o = blk['off']
        name = b.id_name('Object', o)
        names.append(name)
        if object_names and name not in object_names:
            continue
        me_blk = b.block_at(b.read('Object', o, 'data'))
        if me_blk is None:
            continue
        meshes.append(me_blk['off'])
        M = blend_object_matrix(b, o, ignore_object_rotation)
        split = blend_split_angle(b, o, me_blk['off'])
        if split:
            print('%s: hard edges above %.0f degrees%s' % (name, math.degrees(split[0]),
                                                           ', and edges marked sharp' if split[1] else ''))
        polys += blend_mesh_polys(b, me_blk['off'], M, split, vid_base)
        rigged.append((o, me_blk['off'], vid_base))
        vid_base += b.read('Mesh', me_blk['off'], 'totvert')
    if not meshes:
        sys.exit('%s: no mesh objects%s; it has: %s' % (path, ' named ' + ', '.join(object_names) if object_names else '',
                                                        ', '.join(names) or 'none'))
    data, name = blend_texture(b, path, meshes, image_name)
    skin = blend_skin(b, rigged, polys, ignore_object_rotation, action_names, skin_from) if animate else None
    return polys, data, name, skin

def blend_object_matrix(b, o, ignore_object_rotation=False):
    mat_field = 'obmat' if b.has('Object', 'obmat') else 'object_to_world'
    M = m4_from_blend(b.read('Object', o, mat_field))
    if ignore_object_rotation:
        s = b.read('Object', o, 'size')
        M = [[s[0], 0, 0, 0], [0, s[1], 0, 0], [0, 0, s[2], 0], [0, 0, 0, 1]]
    return M

def blend_skin(b, rigged, polys, ignore_object_rotation, action_names, skin_from):
    """Weights by vertex id, joints and animations for the meshes, from the
    armature that deforms them, or the one deforming the object skin_from"""
    src = None
    if skin_from:
        for blk in b.blocks:
            if blk['code'] == 'OB' and b.id_name('Object', blk['off']) == skin_from:
                src = blk['off']
        if src is None or b.read('Object', src, 'type') != 1:
            sys.exit('no mesh object named %r to take weights from' % skin_from)
    arms = {blend_armature_of(b, o) for o, _, _ in rigged} - {None}
    if src is not None:
        arms = {blend_armature_of(b, src)} - {None}
        if not arms:
            sys.exit('%s is not deformed by an armature' % skin_from)
    if not arms:
        return None
    if len(arms) > 1:
        sys.exit('the meshes are deformed by more than one armature; convert them one at a time with --object')
    if ignore_object_rotation:
        print('warning: --ignore-object-rotation also leaves out the armature\'s placement')
    rig = BlendRig(b, arms.pop())

    vid_weights = {}
    stris = None
    pts = {c[3]: c[0] for p in polys for c in p}
    for o, me, base in rigged:
        nv = b.read('Mesh', me, 'totvert')
        own = blend_vertex_weights(b, o, me) if blend_armature_of(b, o) is not None else [{}] * nv
        if src is None or any(own):
            for k, w in enumerate(own):
                vid_weights[base + k] = w
            continue
        # Weights of the closest point on the other mesh's surface, both as
        # placed in the scene
        if stris is None:
            sme = b.block_at(b.read('Object', src, 'data'))['off']
            sw = blend_vertex_weights(b, src, sme)
            stris = [[(c[0], sw[c[3]]) for c in t] for t in fan(blend_mesh_polys(b, sme, blend_object_matrix(b, src), None, 0))]
        ids = sorted(v for v in pts if base <= v < base + nv)
        print('%s: weights from the closest points of %s' % (b.id_name('Object', o), skin_from))
        for v, w in zip(ids, transfer_weights([pts[v] for v in ids], stris)):
            vid_weights[v] = w
    return build_skin(rig, vid_weights, action_names)

def split_normals(pos, faces, sharp_edges, split):
    """Per corner normals of smooth faces, smoothed across the edges that are
    not sharp, as Blender's Edge Split or auto smooth would. Flat faces and
    their corners get None."""
    angle, use_marked = split
    fnorm = []
    for vi, _, _ in faces:
        # Newell's method, for polygons that are not quite flat
        n = [0.0, 0.0, 0.0]
        for k in range(len(vi)):
            a = pos[vi[k]]
            c = pos[vi[(k + 1) % len(vi)]]
            n[0] += (a[1] - c[1]) * (a[2] + c[2])
            n[1] += (a[2] - c[2]) * (a[0] + c[0])
            n[2] += (a[0] - c[0]) * (a[1] + c[1])
        fnorm.append(normalize(n))

    edge_faces = {}
    for f, (vi, _, _) in enumerate(faces):
        for k in range(len(vi)):
            e = tuple(sorted((vi[k], vi[(k + 1) % len(vi)])))
            edge_faces.setdefault(e, []).append(f)
    cos_limit = math.cos(angle)

    def smooth_across(e):
        fs = edge_faces[e]
        if len(fs) != 2 or not faces[fs[0]][2] or not faces[fs[1]][2]:
            return False
        if use_marked and e in sharp_edges:
            return False
        return sum(fnorm[fs[0]][i] * fnorm[fs[1]][i] for i in range(3)) >= cos_limit - 1e-6

    # Around each vertex, faces joined by smooth edges share one normal,
    # weighted by the angle of each face at the vertex
    corner = [[None] * len(vi) for vi, _, _ in faces]
    vert_faces = {}
    for f, (vi, _, _) in enumerate(faces):
        for k, v in enumerate(vi):
            vert_faces.setdefault(v, []).append((f, k))
    for v, fk in vert_faces.items():
        parent = {f: f for f, _ in fk}

        def find(f):
            while parent[f] != f:
                parent[f] = parent[parent[f]]
                f = parent[f]
            return f
        for f, k in fk:
            vi = faces[f][0]
            for w in (vi[k - 1], vi[(k + 1) % len(vi)]):
                e = tuple(sorted((v, w)))
                if smooth_across(e):
                    for g in edge_faces[e]:
                        if g in parent:
                            parent[find(g)] = find(f)
        sums = {}
        for f, k in fk:
            vi = faces[f][0]
            a = [pos[vi[k - 1]][i] - pos[v][i] for i in range(3)]
            c = [pos[vi[(k + 1) % len(vi)]][i] - pos[v][i] for i in range(3)]
            la = math.sqrt(sum(x * x for x in a)) or 1.0
            lc = math.sqrt(sum(x * x for x in c)) or 1.0
            w = math.acos(max(-1.0, min(1.0, sum(a[i] * c[i] for i in range(3)) / (la * lc))))
            s = sums.setdefault(find(f), [0.0, 0.0, 0.0])
            for i in range(3):
                s[i] += fnorm[f][i] * w
        for f, k in fk:
            if faces[f][2]:
                corner[f][k] = normalize(sums[find(f)])
    return corner

def blend_mesh_polys(b, me, M, split, vid_base):
    def xf(p, w):
        return [sum(M[r][c] * (p[c] if c < 3 else w) for c in range(4)) for r in range(3)]

    # Normals turn with the inverse transpose; for a scale that is not
    # uniform this only matters for smooth faces, which use stored normals
    nv = b.read('Mesh', me, 'totvert')
    vblk = b.block_at(b.read('Mesh', me, 'mvert'))
    vs = b.size_of('MVert')
    pos = []
    nor = []
    for i in range(nv):
        off = vblk['off'] + i * vs
        pos.append(blender_to_gltf(xf(b.read('MVert', off, 'co'), 1.0)))
        if b.has('MVert', 'no'):
            n = blender_to_gltf(xf([x / 32767.0 for x in b.read('MVert', off, 'no')], 0.0))
            nor.append(normalize(n))
        else:
            nor.append(None)

    # Faces as (vertex indices, uvs, smooth)
    faces = []
    totpoly = b.read('Mesh', me, 'totpoly') if b.has('Mesh', 'totpoly') else 0
    if totpoly > 0 and b.block_at(b.read('Mesh', me, 'mpoly')):
        pblk = b.block_at(b.read('Mesh', me, 'mpoly'))
        lblk = b.block_at(b.read('Mesh', me, 'mloop'))
        ublk = b.block_at(b.read('Mesh', me, 'mloopuv')) if b.has('Mesh', 'mloopuv') else None
        ps = b.size_of('MPoly')
        ls = b.size_of('MLoop')
        us = b.size_of('MLoopUV') if ublk else 0
        for i in range(totpoly):
            off = pblk['off'] + i * ps
            start = b.read('MPoly', off, 'loopstart')
            n = b.read('MPoly', off, 'totloop')
            smooth = b.read('MPoly', off, 'flag') & 1
            vi = [b.read('MLoop', lblk['off'] + (start + k) * ls, 'v') for k in range(n)]
            uv = [b.read('MLoopUV', ublk['off'] + (start + k) * us, 'uv') if ublk else [0.0, 0.0] for k in range(n)]
            faces.append((vi, uv, smooth))
    else:
        nf = b.read('Mesh', me, 'totface')
        fblk = b.block_at(b.read('Mesh', me, 'mface'))
        tblk = b.block_at(b.read('Mesh', me, 'mtface'))
        fs = b.size_of('MFace')
        ts = b.size_of('MTFace')
        for i in range(nf):
            off = fblk['off'] + i * fs
            vi = [b.read('MFace', off, 'v%d' % k) for k in range(1, 5)]
            n = 4 if vi[3] != 0 else 3
            uvs = b.read('MTFace', tblk['off'] + i * ts, 'uv') if tblk else [0.0] * 8
            uv = [uvs[k * 2:k * 2 + 2] for k in range(n)]
            faces.append((vi[:n], uv, b.read('MFace', off, 'flag') & 1))

    # Edges marked sharp, by their two vertices
    sharp = set()
    eblk = b.block_at(b.read('Mesh', me, 'medge'))
    if split and eblk:
        es = b.size_of('MEdge')
        for i in range(b.read('Mesh', me, 'totedge')):
            off = eblk['off'] + i * es
            if b.read('MEdge', off, 'flag') & (1 << 9):
                sharp.add(tuple(sorted((b.read('MEdge', off, 'v1'), b.read('MEdge', off, 'v2')))))
    corner_nor = split_normals(pos, faces, sharp, split) if split else None

    polys = []
    for f, (vi, uv, smooth) in enumerate(faces):
        corners = []
        for k, v in enumerate(vi):
            n = corner_nor[f][k] if corner_nor else (nor[v] if smooth else None)
            corners.append((pos[v], n, (uv[k][0], 1.0 - uv[k][1]), vid_base + v))
        polys.append(corners)
    return polys

# --- Blender armatures and actions ---
#
# A mesh deformed by an armature is written with its skeleton, and the
# actions of the armature as animations. Each frame of an action is posed as
# Blender would: the F-curves give each bone's own transform, bones follow
# their parents, then constraints apply. Of these, Limit Rotation, Copy
# Rotation and IK are done; IK as Blender's legacy solver does it, turning the
# chain to its pole target first, then reaching for the goal by damped least
# squares, which lands where Blender does for chains of two bones and close by
# for longer ones. The poses are stored at every frame, as the model loaders
# take the key rate of a clip from its keys.

def m4_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]

def m4_inv(m):
    a = [list(m[i]) + [1.0 if i == j else 0.0 for j in range(4)] for i in range(4)]
    for c in range(4):
        p = max(range(c, 4), key=lambda r: abs(a[r][c]))
        a[c], a[p] = a[p], a[c]
        d = a[c][c]
        a[c] = [x / d for x in a[c]]
        for r in range(4):
            if r != c and a[r][c] != 0.0:
                f = a[r][c]
                a[r] = [x - f * y for x, y in zip(a[r], a[c])]
    return [row[4:] for row in a]

def m4_ident():
    return [[1.0 if i == j else 0.0 for j in range(4)] for i in range(4)]

def m4_from_blend(flat):
    # Blender stores matrices by column
    return [[flat[c * 4 + r] for c in range(4)] for r in range(4)]

def m4_compose(t, r3, s):
    return [[r3[i][j] * s[j] for j in range(3)] + [t[i]] for i in range(3)] + [[0.0, 0.0, 0.0, 1.0]]

def m4_split(m):
    """Location, rotation as a 3x3 matrix, and scale of a matrix"""
    s = [math.sqrt(sum(m[i][j] ** 2 for i in range(3))) for j in range(3)]
    det = (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
           + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))
    if det < 0:
        s[0] = -s[0]
    r = [[m[i][j] / s[j] if s[j] else 0.0 for j in range(3)] for i in range(3)]
    return [m[0][3], m[1][3], m[2][3]], r, s

def quat_to_m3(q):
    """Rotation of a quaternion as (w, x, y, z), normalized first"""
    n = math.sqrt(sum(x * x for x in q)) or 1.0
    w, x, y, z = (v / n for v in q)
    return [[1 - 2 * (y * y + z * z), 2 * (x * y - w * z), 2 * (x * z + w * y)],
            [2 * (x * y + w * z), 1 - 2 * (x * x + z * z), 2 * (y * z - w * x)],
            [2 * (x * z - w * y), 2 * (y * z + w * x), 1 - 2 * (x * x + y * y)]]

def m3_to_quat(r):
    """(w, x, y, z) of a rotation matrix"""
    tr = r[0][0] + r[1][1] + r[2][2]
    if tr > 0:
        s = math.sqrt(tr + 1.0) * 2
        q = [0.25 * s, (r[2][1] - r[1][2]) / s, (r[0][2] - r[2][0]) / s, (r[1][0] - r[0][1]) / s]
    elif r[0][0] > r[1][1] and r[0][0] > r[2][2]:
        s = math.sqrt(1.0 + r[0][0] - r[1][1] - r[2][2]) * 2
        q = [(r[2][1] - r[1][2]) / s, 0.25 * s, (r[0][1] + r[1][0]) / s, (r[0][2] + r[2][0]) / s]
    elif r[1][1] > r[2][2]:
        s = math.sqrt(1.0 + r[1][1] - r[0][0] - r[2][2]) * 2
        q = [(r[0][2] - r[2][0]) / s, (r[0][1] + r[1][0]) / s, 0.25 * s, (r[1][2] + r[2][1]) / s]
    else:
        s = math.sqrt(1.0 + r[2][2] - r[0][0] - r[1][1]) * 2
        q = [(r[1][0] - r[0][1]) / s, (r[0][2] + r[2][0]) / s, (r[1][2] + r[2][1]) / s, 0.25 * s]
    n = math.sqrt(sum(x * x for x in q))
    return [x / n for x in q]

def m3_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]

def m3_t(a):
    return [[a[j][i] for j in range(3)] for i in range(3)]

def m3_axis_angle(axis, angle):
    n = math.sqrt(sum(x * x for x in axis))
    if n < 1e-12:
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    h = angle / 2
    return quat_to_m3([math.cos(h)] + [x / n * math.sin(h) for x in axis])

# Blender's Euler orders, by rotmode, as the axes in the order they apply
EULER_ORDERS = {1: 'XYZ', 2: 'XZY', 3: 'YXZ', 4: 'YZX', 5: 'ZXY', 6: 'ZYX'}

def eul_to_m3(e, order='XYZ'):
    r = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    for ax in order:
        i = 'XYZ'.index(ax)
        r = m3_mul(m3_axis_angle([1.0 if k == i else 0.0 for k in range(3)], e[i]), r)
    return r

def m3_to_eul(m):
    """XYZ Euler angles of a rotation matrix, of the two choices the one
    with the smaller angles, as Blender picks"""
    cy = math.hypot(m[0][0], m[1][0])
    if cy > 16 * 1.2e-7:
        e1 = [math.atan2(m[2][1], m[2][2]), math.atan2(-m[2][0], cy), math.atan2(m[1][0], m[0][0])]
        e2 = [math.atan2(-m[2][1], -m[2][2]), math.atan2(-m[2][0], -cy), math.atan2(-m[1][0], -m[0][0])]
    else:
        e1 = [math.atan2(-m[1][2], m[1][1]), math.atan2(-m[2][0], cy), 0.0]
        e2 = e1
    return e1 if sum(map(abs, e1)) <= sum(map(abs, e2)) else e2

def slerp(a, b, t):
    d = sum(x * y for x, y in zip(a, b))
    if d < 0:
        b = [-x for x in b]
        d = -d
    if d > 0.9995:
        q = [x + (y - x) * t for x, y in zip(a, b)]
    else:
        th = math.acos(d)
        sa, sb = math.sin((1 - t) * th), math.sin(t * th)
        q = [(x * sa + y * sb) / math.sin(th) for x, y in zip(a, b)]
    n = math.sqrt(sum(x * x for x in q))
    return [x / n for x in q]

def m4_blend(a, b, t):
    """Location, rotation and scale of a matrix taken a share t towards another"""
    ta, ra, sa = m4_split(a)
    tb, rb, sb = m4_split(b)
    q = slerp(m3_to_quat(ra), m3_to_quat(rb), t)
    lerp = lambda x, y: [u + (v - u) * t for u, v in zip(x, y)]
    return m4_compose(lerp(ta, tb), quat_to_m3(q), lerp(sa, sb))

def blend_list(b, listbase):
    """The blocks of a ListBase, each starting with its next pointer"""
    out = []
    ptr = b.read('ListBase', listbase, 'first')
    pfmt = b.end + ('Q' if b.psize == 8 else 'I')
    while ptr:
        blk = b.block_at(ptr)
        if blk is None:
            break
        out.append(blk)
        ptr = struct.unpack_from(pfmt, b.data, blk['off'])[0]
    return out

def blend_cstring(b, ptr):
    blk = b.block_at(ptr)
    if blk is None:
        return None
    return b.data[blk['off']:b.data.index(b'\0', blk['off'])].decode('latin1')

def bezier_value(p0, p1, p2, p3, x):
    """The height of a Bezier segment of an F-curve at a frame, with its
    handles shortened, as Blender does, so that it can't loop back"""
    h1 = (p0[0] - p1[0], p0[1] - p1[1])
    h2 = (p3[0] - p2[0], p3[1] - p2[1])
    span = p3[0] - p0[0]
    l1, l2 = abs(h1[0]), abs(h2[0])
    if l1 + l2 > span > 0:
        f = span / (l1 + l2)
        p1 = (p0[0] - f * h1[0], p0[1] - f * h1[1])
        p2 = (p3[0] - f * h2[0], p3[1] - f * h2[1])
    cub = lambda a, b_, c, d, t: (((1 - t) ** 3) * a + 3 * (1 - t) ** 2 * t * b_
                                  + 3 * (1 - t) * t * t * c + t ** 3 * d)
    lo, hi = 0.0, 1.0
    for _ in range(60):
        mid = (lo + hi) / 2
        if cub(p0[0], p1[0], p2[0], p3[0], mid) < x:
            lo = mid
        else:
            hi = mid
    return cub(p0[1], p1[1], p2[1], p3[1], (lo + hi) / 2)

def fcurve_value(keys, extend, x):
    """The value of an F-curve at a frame, from its keys as (handle before,
    key, handle after, interpolation)"""
    first, last = keys[0], keys[-1]
    if x <= first[1][0] or x >= last[1][0]:
        k, inner = (first, 2) if x <= first[1][0] else (last, 0)
        if extend == 1 and len(keys) > 1:
            # Linear extrapolation, along the handle or to the next key
            other = k[inner] if k[3] == 2 else (keys[1][1] if k is first else keys[-2][1])
            dx = other[0] - k[1][0]
            if dx:
                return k[1][1] + (x - k[1][0]) * (other[1] - k[1][1]) / dx
        return k[1][1]
    lo, hi = 0, len(keys) - 1
    while hi - lo > 1:
        mid = (lo + hi) // 2
        if keys[mid][1][0] <= x:
            lo = mid
        else:
            hi = mid
    a, b = keys[lo], keys[hi]
    if a[3] == 0:
        return a[1][1]
    if a[3] == 1 or b[1][0] == a[1][0]:
        return a[1][1] + (b[1][1] - a[1][1]) * (x - a[1][0]) / (b[1][0] - a[1][0])
    return bezier_value(a[1], a[2], b[0], b[1], x)

class BlendRig:
    """An armature of a .blend file, its actions, and the poses they give"""

    def __init__(self, b, arm_ob):
        self.b = b
        self.name = b.id_name('Object', arm_ob)
        self.obmat = blend_object_matrix(b, arm_ob)
        self.inv_obmat = m4_inv(self.obmat)
        pose = b.block_at(b.read('Object', arm_ob, 'pose'))
        if pose is None:
            sys.exit('%s: armature %s has no pose' % ('blend', self.name))
        if b.has('bPose', 'iksolver') and b.read('bPose', pose['off'], 'iksolver') != 0:
            print('warning: %s uses the iTaSC IK solver; solving as the standard one' % self.name)

        # Bones, in the order of the pose channels, which puts parents first
        self.bones = []
        index = {}
        warned = set()
        for pc in blend_list(b, b.read('bPose', pose['off'], 'chanbase')):
            o = pc['off']
            bone = b.block_at(b.read('bPoseChannel', o, 'bone'))
            if bone is None:
                continue
            bo = bone['off']
            name = b.read('bPoseChannel', o, 'name')
            par = b.block_at(b.read('Bone', bo, 'parent'))
            flag = b.read('Bone', bo, 'flag')
            # Hinge, no scale inheritance, no local location
            if flag & (512 | 32768 | (1 << 22)) and 'inherit' not in warned:
                warned.add('inherit')
                print('warning: some bones of %s do not inherit all of their parent\'s transform; '
                      'posing them as if they did' % self.name)
            self.bones.append(dict(
                name=name, pc=o,
                parent_name=b.read('Bone', par['off'], 'name') if par else None,
                arm_mat=m4_from_blend(b.read('Bone', bo, 'arm_mat')),
                length=b.read('Bone', bo, 'length'), deform=not flag & 4096,
                rotmode=b.read('bPoseChannel', o, 'rotmode'),
                loc=b.read('bPoseChannel', o, 'loc'), quat=b.read('bPoseChannel', o, 'quat'),
                eul=b.read('bPoseChannel', o, 'eul'), size=b.read('bPoseChannel', o, 'size'),
                constraints=self.read_constraints(o, warned)))
            index[name] = len(self.bones) - 1
        self.index = index
        for bn in self.bones:
            bn['parent'] = index.get(bn['parent_name'], -1)
            p = bn['parent']
            bn['offs'] = m4_mul(m4_inv(self.bones[p]['arm_mat']), bn['arm_mat']) if p >= 0 else bn['arm_mat']

        # IK chains, from the bone with the constraint up, by the bone at
        # their top, where Blender solves them
        self.chains = {}
        for i, bn in enumerate(self.bones):
            for c in bn['constraints']:
                if c['type'] != 'ik':
                    continue
                chain = [i]
                while self.bones[chain[-1]]['parent'] >= 0 and (c['chain_len'] == 0 or len(chain) < c['chain_len']):
                    chain.append(self.bones[chain[-1]]['parent'])
                c['chain'] = chain[::-1]
                self.chains.setdefault(chain[-1], []).append(c)
        self.fps = 24.0
        for blk in b.blocks:
            if blk['code'] == 'SC':
                r = b.read('Scene', blk['off'], 'r')
                self.fps = b.read('RenderData', r, 'frs_sec') / (b.read('RenderData', r, 'frs_sec_base') or 1.0)
                break

    def read_constraints(self, pc, warned):
        b = self.b
        out = []
        for blk in blend_list(b, b.read('bPoseChannel', pc, 'constraints')):
            o = blk['off']
            kind = b.read('bConstraint', o, 'type')
            flag = b.read('bConstraint', o, 'flag')
            infl = b.read('bConstraint', o, 'enforce')
            # Disabled or turned off
            if flag & (4 | 512) or infl <= 0.0:
                continue
            d = b.block_at(b.read('bConstraint', o, 'data'))
            c = dict(influence=infl, own=b.read('bConstraint', o, 'ownspace'),
                     tar=b.read('bConstraint', o, 'tarspace'), name=b.read('bConstraint', o, 'name'))
            if kind == 3:
                k = d['off']
                tar = b.block_at(b.read('bKinematicConstraint', k, 'tar'))
                pole = b.block_at(b.read('bKinematicConstraint', k, 'poletar'))
                c.update(type='ik', target=b.read('bKinematicConstraint', k, 'subtarget'),
                         target_ob=b.id_name('Object', tar['off']) if tar else None,
                         pole=b.read('bKinematicConstraint', k, 'polesubtarget') if pole else None,
                         pole_angle=b.read('bKinematicConstraint', k, 'poleangle'),
                         chain_len=b.read('bKinematicConstraint', k, 'rootbone'),
                         ik_flag=b.read('bKinematicConstraint', k, 'flag'))
                if c['ik_flag'] & 2:
                    print('warning: IK constraint %r also aims for the target\'s rotation, which is left out' % c['name'])
            elif kind == 5:
                k = d['off']
                c.update(type='limit_rot', flag=b.read('bRotLimitConstraint', k, 'flag'),
                         lo=[b.read('bRotLimitConstraint', k, a + 'min') for a in 'xyz'],
                         hi=[b.read('bRotLimitConstraint', k, a + 'max') for a in 'xyz'])
            elif kind == 8:
                k = d['off']
                tar = b.block_at(b.read('bRotateLikeConstraint', k, 'tar'))
                c.update(type='copy_rot', flag=b.read('bRotateLikeConstraint', k, 'flag'),
                         target=b.read('bRotateLikeConstraint', k, 'subtarget'),
                         target_ob=b.id_name('Object', tar['off']) if tar else None)
            else:
                if ('c', kind) not in warned:
                    warned.add(('c', kind))
                    print('warning: constraint %r (type %d) is not supported; left out' % (c['name'], kind))
                continue
            if c.get('target_ob') not in (None, self.name):
                print('warning: constraint %r targets another object, %s; left out' % (c['name'], c['target_ob']))
                continue
            out.append(c)
        return out

    def actions(self):
        """The actions that animate bones of this armature, as (name,
        {(bone, property): [curve per component]}, first frame, last frame)"""
        b = self.b
        out = []
        for blk in b.blocks:
            if blk['code'] != 'AC':
                continue
            curves = {}
            lo, hi = math.inf, -math.inf
            for fc in blend_list(b, b.read('bAction', blk['off'], 'curves')):
                o = fc['off']
                path = blend_cstring(b, b.read('FCurve', o, 'rna_path')) or ''
                m = re.fullmatch(r'pose\.bones\["(.+)"\]\.(\w+)', path)
                if not m or m.group(1) not in self.index:
                    continue
                n = b.read('FCurve', o, 'totvert')
                bz = b.block_at(b.read('FCurve', o, 'bezt'))
                if not n or bz is None:
                    continue
                bs = b.size_of('BezTriple')
                keys = []
                for k in range(n):
                    v = b.read('BezTriple', bz['off'] + k * bs, 'vec')
                    keys.append(((v[0], v[1]), (v[3], v[4]), (v[6], v[7]), b.read('BezTriple', bz['off'] + k * bs, 'ipo')))
                if blend_list(b, b.read('FCurve', o, 'modifiers')):
                    print('warning: an F-curve of %s has modifiers, which are left out' % b.id_name('bAction', blk['off']))
                lo = min(lo, keys[0][1][0])
                hi = max(hi, keys[-1][1][0])
                key = (self.index[m.group(1)], m.group(2))
                curves.setdefault(key, {})[b.read('FCurve', o, 'array_index')] = (keys, b.read('FCurve', o, 'extend'))
            if curves:
                out.append((b.id_name('bAction', blk['off']), curves, lo, hi))
        return out

    def channel_mat(self, i, curves, frame):
        """A bone's own transform at a frame, from its curves or its pose"""
        bn = self.bones[i]
        def prop(name, default):
            cs = curves.get((i, name))
            if not cs:
                return list(default)
            return [fcurve_value(cs[k][0], cs[k][1], frame) if k in cs else default[k] for k in range(len(default))]
        loc = prop('location', bn['loc'])
        size = prop('scale', bn['size'])
        if bn['rotmode'] == 0:
            rot = quat_to_m3(prop('rotation_quaternion', bn['quat']))
        elif bn['rotmode'] in EULER_ORDERS:
            rot = eul_to_m3(prop('rotation_euler', bn['eul']), EULER_ORDERS[bn['rotmode']])
        else:
            aa = prop('rotation_axis_angle', [0.0, 0.0, 1.0, 0.0])
            rot = m3_axis_angle(aa[1:], aa[0])
        return m4_compose(loc, rot, size)

    def fk(self, i, pose, curves, frame):
        p = self.bones[i]['parent']
        base = m4_mul(pose[p], self.bones[i]['offs']) if p >= 0 else self.bones[i]['offs']
        return m4_mul(base, self.channel_mat(i, curves, frame))

    def to_space(self, i, m, space, pose):
        if space == 0:
            return m4_mul(self.obmat, m)
        if space == 1:
            p = self.bones[i]['parent']
            base = m4_mul(pose[p], self.bones[i]['offs']) if p >= 0 else self.bones[i]['offs']
            return m4_mul(m4_inv(base), m)
        return m

    def from_space(self, i, m, space, pose):
        if space == 0:
            return m4_mul(self.inv_obmat, m)
        if space == 1:
            p = self.bones[i]['parent']
            base = m4_mul(pose[p], self.bones[i]['offs']) if p >= 0 else self.bones[i]['offs']
            return m4_mul(base, m)
        return m

    def apply_constraints(self, i, pose):
        for c in self.bones[i]['constraints']:
            if c['type'] == 'ik':
                continue
            before = pose[i]
            m = self.to_space(i, before, c['own'], pose)
            t, r, s = m4_split(m)
            if c['type'] == 'limit_rot':
                e = m3_to_eul(r)
                for a in range(3):
                    if c['flag'] & (1 << a):
                        e[a] = min(max(e[a], c['lo'][a]), c['hi'][a])
                r = eul_to_m3(e)
            elif c['type'] == 'copy_rot':
                j = self.index.get(c['target'])
                if j is None:
                    continue
                tr = m4_split(self.to_space(j, pose[j], c['tar'], pose))[1]
                if c['flag'] & 7 == 7 and not c['flag'] & (16 | 32 | 64 | 128):
                    r = tr
                else:
                    e, te = m3_to_eul(r), m3_to_eul(tr)
                    for a in range(3):
                        if c['flag'] & (1 << a):
                            v = te[a] + (e[a] if c['flag'] & 128 else 0.0)
                            e[a] = -v if c['flag'] & (16 << a) else v
                    r = eul_to_m3(e)
            after = self.from_space(i, m4_compose(t, r, s), c['own'], pose)
            pose[i] = after if c['influence'] >= 1.0 else m4_blend(before, after, c['influence'])

    def solve_ik(self, c, pose):
        chain = c['chain']
        tgt = self.index.get(c['target'])
        if tgt is None:
            return
        goal = [pose[tgt][r][3] for r in range(3)]
        tip = chain[-1]
        # Where the tip reaches from, in its bone's space: its tail, or its head
        reach = [0.0, self.bones[tip]['length'] if c['ik_flag'] & 1 else 0.0, 0.0, 1.0]
        heads = [[pose[k][r][3] for r in range(3)] for k in chain]
        rots, scales = [], []
        for k in chain:
            _, r, s = m4_split(pose[k])
            rots.append(r)
            scales.append(s)
        # The head of each bone and the tip, in the space of the bone before
        inv0 = [m4_inv(pose[k]) for k in chain]
        links = [[sum(inv0[n][r][q] * (heads[n + 1] + [1.0])[q] for q in range(4)) for r in range(3)]
                 for n in range(len(chain) - 1)]
        links.append(reach[:3])

        def place():
            # Heads and tip from the root's head and each bone's rotation
            pts = [heads[0]]
            for n in range(len(chain)):
                l = links[n]
                pts.append([pts[n][r] + sum(rots[n][r][q] * scales[n][q] * l[q] for q in range(3)) for r in range(3)])
            return pts

        def turn(n0, rot, pivot):
            for n in range(n0, len(chain)):
                rots[n] = m3_mul(rot, rots[n])

        pts = place()
        size = sum(math.dist(pts[n], pts[n + 1]) for n in range(len(chain))) or 1.0

        # Turn the chain about its root so that, seen from the root, the tip
        # points at the goal and the pole direction of the root bone at the
        # pole target
        pole = self.index.get(c['pole']) if c.get('pole') else None
        if pole is not None:
            root = pts[0]
            norm = lambda v: [x / (math.sqrt(sum(y * y for y in v)) or 1.0) for x in v]
            def frame(d, up):
                x = norm(cross(d, up))
                return [x, cross(x, d), [-v for v in d]]
            a = c['pole_angle']
            rx = [rots[0][r][0] for r in range(3)]
            rz = [rots[0][r][2] for r in range(3)]
            up = [rx[r] * math.cos(a) + rz[r] * math.sin(a) for r in range(3)]
            mat = frame(norm([pts[-1][r] - root[r] for r in range(3)]), up)
            polepos = [pose[pole][r][3] for r in range(3)]
            polemat = frame(norm([goal[r] - root[r] for r in range(3)]), norm([polepos[r] - root[r] for r in range(3)]))
            turn(0, m3_mul(m3_t(polemat), mat), root)
            pts = place()

        # Damped least squares, a joint of three axes at each bone's head.
        # A step is only kept if it brings the tip closer, with more damping
        # after one that does not, so that a goal out of reach ends with the
        # chain stretched towards it, as in Blender, rather than flailing.
        damp = (0.01 * size) ** 2
        dist = lambda ps: math.sqrt(sum((goal[r] - ps[-1][r]) ** 2 for r in range(3)))
        cur = dist(pts)
        for _ in range(400):
            if cur < 1e-6 * size or damp > 1e6 * size * size:
                break
            tip_pos = pts[-1]
            err = [goal[r] - tip_pos[r] for r in range(3)]
            cols = []
            for n in range(len(chain)):
                arm = [tip_pos[r] - pts[n][r] for r in range(3)]
                for ax in range(3):
                    cols.append(cross([1.0 if q == ax else 0.0 for q in range(3)], arm))
            jjt = [[sum(col[r] * col[q] for col in cols) + (damp if r == q else 0.0) for q in range(3)] for r in range(3)]
            inv = m4_inv([row + [0.0] for row in jjt] + [[0.0, 0.0, 0.0, 1.0]])
            y = [sum(inv[r][q] * err[q] for q in range(3)) for r in range(3)]
            dth = [sum(col[r] * y[r] for r in range(3)) for col in cols]
            saved = list(rots)
            for n in range(len(chain) - 1, -1, -1):
                w = dth[n * 3:n * 3 + 3]
                ang = math.sqrt(sum(x * x for x in w))
                if ang > 1e-12:
                    turn(n, m3_axis_angle(w, min(ang, 0.2)), pts[n])
            new = place()
            d = dist(new)
            if d < cur:
                pts, cur = new, d
                damp *= 0.5
            else:
                rots[:] = saved
                damp *= 10.0

        for n, k in enumerate(chain):
            pose[k] = m4_compose(pts[n], rots[n], scales[n])

    def pose_at(self, curves, frame):
        """Each bone's transform in the armature's space at a frame"""
        pose = [None] * len(self.bones)
        for i in range(len(self.bones)):
            if pose[i] is not None:
                continue
            for c in self.chains.get(i, []):
                for k in c['chain']:
                    if pose[k] is None:
                        pose[k] = self.fk(k, pose, curves, frame)
                        self.apply_constraints(k, pose)
            if i in self.chains:
                for c in self.chains[i]:
                    self.solve_ik(c, pose)
                # Bones below the chain's bones, but in it, follow them
                continue
            pose[i] = self.fk(i, pose, curves, frame)
            self.apply_constraints(i, pose)
        return pose

def blend_armature_of(b, o):
    """The armature object deforming a mesh object, by modifier or parent"""
    for md in blend_list(b, b.read('Object', o, 'modifiers')):
        if b.read('ModifierData', md['off'], 'type') == 8:
            ob = b.block_at(b.read('ArmatureModifierData', md['off'], 'object'))
            if ob:
                return ob['off']
    par = b.block_at(b.read('Object', o, 'parent'))
    if par and b.read('Object', o, 'partype') == 4 and b.read('Object', par['off'], 'type') == 25:
        return par['off']
    return None

def blend_vertex_weights(b, o, me):
    """Each vertex's weights, as {group name: weight}"""
    if b.has('Mesh', 'vertex_group_names'):
        groups = [b.read('bDeformGroup', g['off'], 'name') for g in blend_list(b, b.read('Mesh', me, 'vertex_group_names'))]
    else:
        groups = [b.read('bDeformGroup', g['off'], 'name') for g in blend_list(b, b.read('Object', o, 'defbase'))]
    nv = b.read('Mesh', me, 'totvert')
    out = [{} for _ in range(nv)]
    dblk = b.block_at(b.read('Mesh', me, 'dvert'))
    if dblk is None or not groups:
        return out
    ds, ws = b.size_of('MDeformVert'), b.size_of('MDeformWeight')
    for i in range(nv):
        off = dblk['off'] + i * ds
        wblk = b.block_at(b.read('MDeformVert', off, 'dw'))
        for k in range(b.read('MDeformVert', off, 'totweight') if wblk else 0):
            g = b.read('MDeformWeight', wblk['off'] + k * ws, 'def_nr')
            w = b.read('MDeformWeight', wblk['off'] + k * ws, 'weight')
            if 0 <= g < len(groups) and w > 0:
                out[i][groups[g]] = out[i].get(groups[g], 0.0) + w
    return out

def closest_on_triangle(p, a, b, c):
    """The closest point of a triangle to p, as barycentric weights"""
    ab, ac, ap = sub(b, a), sub(c, a), sub(p, a)
    d1, d2 = dot(ab, ap), dot(ac, ap)
    if d1 <= 0 and d2 <= 0:
        return (1.0, 0.0, 0.0)
    bp = sub(p, b)
    d3, d4 = dot(ab, bp), dot(ac, bp)
    if d3 >= 0 and d4 <= d3:
        return (0.0, 1.0, 0.0)
    vc = d1 * d4 - d3 * d2
    if vc <= 0 and d1 >= 0 and d3 <= 0:
        v = d1 / (d1 - d3)
        return (1 - v, v, 0.0)
    cp = sub(p, c)
    d5, d6 = dot(ab, cp), dot(ac, cp)
    if d6 >= 0 and d5 <= d6:
        return (0.0, 0.0, 1.0)
    vb = d5 * d2 - d1 * d6
    if vb <= 0 and d2 >= 0 and d6 <= 0:
        w = d2 / (d2 - d6)
        return (1 - w, 0.0, w)
    va = d3 * d6 - d5 * d4
    if va <= 0 and (d4 - d3) >= 0 and (d5 - d6) >= 0:
        w = (d4 - d3) / ((d4 - d3) + (d5 - d6))
        return (0.0, 1 - w, w)
    den = 1.0 / (va + vb + vc)
    v, w = vb * den, vc * den
    return (1 - v - w, v, w)

def transfer_weights(points, src_tris):
    """Weights for points from the closest point of the surface of other
    triangles, given as three (position, weights) corners each"""
    out = []
    for p in points:
        best = None
        for t in src_tris:
            bc = closest_on_triangle(p, t[0][0], t[1][0], t[2][0])
            q = [sum(bc[k] * t[k][0][r] for k in range(3)) for r in range(3)]
            d = sum((q[r] - p[r]) ** 2 for r in range(3))
            if best is None or d < best[0]:
                best = (d, bc, t)
        w = {}
        for k in range(3):
            for g, x in best[2][k][1].items():
                w[g] = w.get(g, 0.0) + x * best[1][k]
        out.append(w)
    return out

def build_skin(rig, vid_weights, action_names):
    """The joints, weights and animations to write for a rigged mesh"""
    bones = rig.bones
    # Bones that move vertices, and those above them, become joints
    deform = {bn['name'] for bn in bones if bn['deform']}
    vid_weights = {v: {g: x for g, x in w.items() if g in deform} for v, w in vid_weights.items()}
    used = set()
    for w in vid_weights.values():
        used.update(w)
    keep = set()
    for name in used:
        i = rig.index[name]
        while i >= 0 and i not in keep:
            keep.add(i)
            i = bones[i]['parent']
    if not keep:
        sys.exit('no vertex is weighted to a bone of ' + rig.name)
    order = [i for i in range(len(bones)) if i in keep]
    joint_of = {i: n for n, i in enumerate(order)}

    # Up to four bones per vertex, as Blender divides by their sum; a vertex
    # with none stays with the topmost bone, as it would stay put in Blender
    weights = {}
    loose = 0
    for vid, w in vid_weights.items():
        pairs = sorted(((x, joint_of[rig.index[g]]) for g, x in w.items() if g in rig.index), reverse=True)[:4]
        total = sum(x for x, _ in pairs)
        if total <= 0:
            loose += 1
            pairs, total = [(1.0, 0)], 1.0
        weights[vid] = [(j, x / total) for x, j in pairs]
    if loose:
        print('warning: %d vertices have no weights; they follow bone %s' % (loose, bones[order[0]]['name']))

    def local(pose, i):
        p = bones[i]['parent']
        return m4_mul(m4_inv(pose[p]), pose[i]) if p >= 0 else pose[i]

    rest = [bn['arm_mat'] for bn in bones]
    joints = []
    for i in order:
        t, r, s = m4_split(local(rest, i))
        joints.append(dict(name=bones[i]['name'], parent=joint_of.get(bones[i]['parent'], -1),
                           rest=rest[i], t=t, q=m3_to_quat(r), s=s))

    anims = []
    acts = rig.actions()
    names = [a[0] for a in acts]
    for want in action_names:
        if want not in names:
            sys.exit('no action named %r; there are: %s' % (want, ', '.join(names)))
    for name, curves, lo, hi in acts:
        if action_names and name not in action_names:
            continue
        frames = list(range(int(math.floor(lo)), int(math.ceil(hi)) + 1))
        tracks = [[] for _ in order]
        for f in frames:
            pose = rig.pose_at(curves, float(f))
            for n, i in enumerate(order):
                t, r, s = m4_split(local(pose, i))
                tracks[n].append((t, m3_to_quat(r), s))
        times = [(f - frames[0]) / rig.fps for f in frames]
        anims.append(dict(name=name, times=times, tracks=tracks))
        print('animation %s: %d frames, %.2f s' % (name, len(frames), times[-1]))
    return dict(joints=joints, weights=weights, anims=anims, obmat=rig.obmat)

# --- OBJ files ---

def load_obj(path):
    V, T, N = [], [], []
    polys = []
    mtllibs = []
    for line in open(path, encoding='utf-8', errors='replace'):
        p = line.split()
        if not p:
            continue
        if p[0] == 'v':
            V.append([float(x) for x in p[1:4]])
        elif p[0] == 'vt':
            T.append([float(x) for x in p[1:3]])
        elif p[0] == 'vn':
            N.append(normalize([float(x) for x in p[1:4]]))
        elif p[0] == 'mtllib':
            mtllibs.append(line.split(None, 1)[1].strip())
        elif p[0] == 'f':
            corners = []
            for c in p[1:]:
                ix = c.split('/')

                def get(lst, k):
                    if len(ix) <= k or not ix[k]:
                        return None
                    i = int(ix[k])
                    return lst[i - 1 if i > 0 else len(lst) + i]
                t = get(T, 1)
                vi = int(ix[0])
                vid = vi - 1 if vi > 0 else len(V) + vi
                corners.append((get(V, 0), get(N, 2), (t[0], 1.0 - t[1]) if t else (0.0, 0.0), vid))
            polys.append(corners)

    # The first diffuse map of the materials
    tex = None
    for lib in mtllibs:
        mp = os.path.join(os.path.dirname(path), lib)
        if not os.path.isfile(mp):
            continue
        for line in open(mp, encoding='utf-8', errors='replace'):
            p = line.split(None, 1)
            if len(p) == 2 and p[0] == 'map_Kd' and tex is None:
                tex = os.path.join(os.path.dirname(mp), p[1].strip().split()[-1])
    data = open(tex, 'rb').read() if tex and os.path.isfile(tex) else None
    return polys, data, tex

# --- Decimation ---

def quadric_of_plane(n, d, w):
    # The 4x4 symmetric matrix of (n.p + d)^2, times w, as its 10 entries
    a, b, c = n
    return [w * a * a, w * a * b, w * a * c, w * a * d, w * b * b, w * b * c, w * b * d,
            w * c * c, w * c * d, w * d * d]

def quadric_error(q, p):
    x, y, z = p
    return (q[0] * x * x + 2 * q[1] * x * y + 2 * q[2] * x * z + 2 * q[3] * x
            + q[4] * y * y + 2 * q[5] * y * z + 2 * q[6] * y
            + q[7] * z * z + 2 * q[8] * z + q[9])

def optimal_point(q):
    """The point where a quadric is least, or None if it has no single one"""
    a = [[q[0], q[1], q[2]], [q[1], q[4], q[5]], [q[2], q[5], q[7]]]
    b = [-q[3], -q[6], -q[8]]
    det = (a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
           + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]))
    scale = max(abs(x) for row in a for x in row) or 1.0
    if abs(det) < 1e-9 * scale ** 3:
        return None
    out = []
    for col in range(3):
        m = [row[:] for row in a]
        for i in range(3):
            m[i][col] = b[i]
        out.append((m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                    + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])) / det)
    return out

def segment_hits_triangle(p, q, a, b, c):
    """Does the segment from p to q pass through the inside of triangle abc?"""
    d = sub(q, p)
    e1 = sub(b, a)
    e2 = sub(c, a)
    h = cross(d, e2)
    det = dot(e1, h)
    if abs(det) < 1e-15:
        return False
    s = sub(p, a)
    u = dot(s, h) / det
    if u <= 1e-6 or u >= 1 - 1e-6:
        return False
    qv = cross(s, e1)
    v = dot(d, qv) / det
    if v <= 1e-6 or u + v >= 1 - 1e-6:
        return False
    t = dot(e2, qv) / det
    return 1e-6 < t < 1 - 1e-6

def triangles_cross(t0, t1):
    for x, y in ((t0, t1), (t1, t0)):
        for k in range(3):
            if segment_hits_triangle(x[k], x[(k + 1) % 3], *y):
                return True
    return False

def min_angle(pts):
    """The smallest angle of a triangle, in radians"""
    out = math.pi
    for k in range(3):
        u = sub(pts[(k + 1) % 3], pts[k])
        v = sub(pts[(k + 2) % 3], pts[k])
        lu = math.sqrt(dot(u, u))
        lv = math.sqrt(dot(v, v))
        if lu == 0 or lv == 0:
            return 0.0
        out = min(out, math.acos(max(-1.0, min(1.0, dot(u, v) / (lu * lv)))))
    return out

def decimate(polys, target, ignore_normals=False, texels=256, seam_texels=1.0, keep=None):
    """Collapse edges, cheapest first by quadric error, until at most target
    triangles remain. A corner's uv and normal together are its wedge, or its
    uv alone with ignore_normals; edges where wedges change, and open edges,
    are seams, which a vertex only slides along, where they are straight on a
    texture of this many texels to within seam_texels, and where seams meet
    it stays. Vertices for which keep(pos) is true stay as they are, and so
    do the faces between them. Returns triangles as polygons of corners (pos,
    normal, uv, vertex id)."""
    import heapq
    tris = []
    pos = {}
    for corners in polys:
        for c in corners:
            pos[c[3]] = c[0]
        for k in range(1, len(corners) - 1):
            tris.append([corners[0], corners[k], corners[k + 1]])
    alive = [True] * len(tris)
    vtris = {}
    for t, tri in enumerate(tris):
        for c in tri:
            vtris.setdefault(c[3], set()).add(t)

    # Vertices that are kept are neither removed nor moved
    locked = {v for v, p in pos.items() if keep(p)} if keep else set()

    def wedge(c):
        n = tuple(round(x, 3) for x in c[1]) if c[1] is not None and not ignore_normals else None
        return (n, round(c[2][0], 5), round(c[2][1], 5))

    def tri_normal(a, b, c):
        n = cross(sub(b, a), sub(c, a))
        ln = math.sqrt(dot(n, n))
        return [x / ln for x in n] if ln > 0 else None, ln * 0.5

    def corner_of(t, v):
        for c in tris[t]:
            if c[3] == v:
                return c
        return None

    def is_seam(a, b):
        shared = [t for t in vtris[a] if t in vtris[b]]
        if len(shared) != 2:
            return True
        t0, t1 = shared
        return (wedge(corner_of(t0, a)) != wedge(corner_of(t1, a))
                or wedge(corner_of(t0, b)) != wedge(corner_of(t1, b)))

    def neighbours(v):
        return {c[3] for t in vtris[v] for c in tris[t]} - {v}

    # Quadrics: the planes of the faces around each vertex, and planes across
    # seams, which hold them in place. Each counts the same whatever its size,
    # so that the cost, divided by the edge's length squared below, is
    # relative, and small parts aren't flattened first for being small.
    allp = list(pos.values())
    diag = math.sqrt(sum((max(p[i] for p in allp) - min(p[i] for p in allp)) ** 2 for i in range(3)))
    Q = {v: [0.0] * 10 for v in pos}
    for tri in tris:
        n, area = tri_normal(*[c[0] for c in tri])
        if n is None:
            continue
        q = quadric_of_plane(n, -dot(n, tri[0][0]), 1.0)
        for c in tri:
            Q[c[3]] = [x + y for x, y in zip(Q[c[3]], q)]
    seam_weight = 3.0
    for tri in tris:
        n, _ = tri_normal(*[c[0] for c in tri])
        if n is None:
            continue
        for k in range(3):
            a, b = tri[k], tri[(k + 1) % 3]
            if not is_seam(a[3], b[3]):
                continue
            e = sub(b[0], a[0])
            m = cross(e, n)
            lm = math.sqrt(dot(m, m))
            if lm == 0:
                continue
            m = [x / lm for x in m]
            q = quadric_of_plane(m, -dot(m, a[0]), seam_weight)
            for v in (a[3], b[3]):
                Q[v] = [x + y for x, y in zip(Q[v], q)]

    def plan(u, v):
        """The cost of moving u onto v, and the wedge each of u's wedges
        becomes, or None if that collapse isn't allowed"""
        shared = [t for t in vtris[u] if t in vtris[v]]
        if not shared or u in locked:
            return None
        # Only the faces on the edge may disappear, or the surface folds
        third = {c[3] for t in shared for c in tris[t]} - {u, v}
        if neighbours(u) & neighbours(v) != third:
            return None
        # A vertex on seams slides along one of exactly two
        seams_u = [w for w in neighbours(u) if is_seam(u, w)]
        if seams_u and (len(seams_u) != 2 or v not in seams_u):
            return None
        mapping = {}
        for t in shared:
            wu = wedge(corner_of(t, u))
            cv = corner_of(t, v)
            if wu in mapping and wedge(mapping[wu]) != wedge(cv):
                return None
            mapping[wu] = cv
        for t in vtris[u]:
            if wedge(corner_of(t, u)) not in mapping:
                return None
        # Sliding along a seam straightens it in the texture too, and where it
        # bends into its island the new edge would cut across the space
        # beyond, so it has to be straight there to within a texel
        for w in seams_u:
            if w == v:
                continue
            for t in vtris[u]:
                cw = corner_of(t, w)
                if cw is None:
                    continue
                cu = corner_of(t, u)
                a = cw[2]
                b = mapping[wedge(cu)][2]
                p = cu[2]
                d = (b[0] - a[0], b[1] - a[1])
                ld = math.hypot(*d)
                off = abs((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]) / ld if ld > 0 else math.hypot(p[0] - a[0], p[1] - a[1])
                if off * texels > seam_texels:
                    return None
        # Away from seams, the merged vertex can go where the quadrics say
        # is best rather than onto v, which keeps rounded parts from caving
        # in; its uv and normal are taken along the edge
        q = [x + y for x, y in zip(Q[u], Q[v])]
        e = sub(pos[v], pos[u])
        place = pos[v]
        attrs = None
        if not seams_u and v not in locked and not any(is_seam(v, w) for w in neighbours(v)):
            best = optimal_point(q)
            if best is not None:
                # Not out of the box of the two vertices and their neighbours,
                # where the solve is unsteady and the part would swell
                ring = [pos[w] for w in neighbours(u) | neighbours(v)]
                for i in range(3):
                    if not min(p[i] for p in ring) <= best[i] <= max(p[i] for p in ring):
                        best = None
                        break
            for cand in ([best] if best is not None else []) + [[(pos[u][i] + pos[v][i]) * 0.5 for i in range(3)]]:
                if quadric_error(q, cand) < quadric_error(q, place):
                    place = cand
            if place is not pos[v]:
                el = dot(e, e)
                t = max(0.0, min(1.0, dot(sub(place, pos[u]), e) / el)) if el > 0 else 0.0
                cu = corner_of(next(iter(vtris[u])), u)
                cv = corner_of(next(iter(vtris[v])), v)
                uv = (cu[2][0] + (cv[2][0] - cu[2][0]) * t, cu[2][1] + (cv[2][1] - cu[2][1]) * t)
                n = None
                if cu[1] is not None and cv[1] is not None:
                    n = normalize([cu[1][i] + (cv[1][i] - cu[1][i]) * t for i in range(3)])
                attrs = (place, n, uv, v)

        # No face may turn over or collapse, in space or on the texture
        def new_uv(c):
            if c[3] == u:
                c = mapping[wedge(c)]
            if attrs is not None and c[3] == v:
                return attrs[2]
            return c[2]

        def uv_area(uvs):
            return ((uvs[1][0] - uvs[0][0]) * (uvs[2][1] - uvs[0][1])
                    - (uvs[1][1] - uvs[0][1]) * (uvs[2][0] - uvs[0][0]))
        for t in vtris[u] | vtris[v]:
            if t in shared:
                continue
            a0 = uv_area([c[2] for c in tris[t]])
            a1 = uv_area([new_uv(c) for c in tris[t]])
            if a0 * a1 <= 0 and a0 != 0:
                return None
            old = [c[0] for c in tris[t]]
            new = [place if c[3] in (u, v) else c[0] for c in tris[t]]
            n0, s0 = tri_normal(*old)
            n1, s1 = tri_normal(*new)
            if n0 is None or n1 is None or dot(n0, n1) < 0.3 or s1 < 1e-12 * diag * diag:
                return None
            # Nor stretch the texture over it much more or less than before,
            # as a face does when its uvs close up into a line
            if a0 != 0:
                stretch = (abs(a1) / s1) / (abs(a0) / s0)
                if stretch < 0.4 or stretch > 2.5:
                    return None
            # Nor become a sliver, which samples the texture in streaks
            q1 = min_angle(new)
            if q1 < 0.05 and q1 < min_angle(old):
                return None
        return max(0.0, quadric_error(q, place)) / max(dot(e, e), 1e-12 * diag * diag), mapping, attrs

    stamp = {v: 0 for v in pos}
    heap = []

    def push(u, v):
        r = plan(u, v)
        if r is not None:
            heapq.heappush(heap, (r[0], u, v, stamp[u], stamp[v]))

    for v in pos:
        for w in neighbours(v):
            push(v, w)
    count = len(tris)
    while count > target and heap:
        cost, u, v, su, sv = heapq.heappop(heap)
        if u not in vtris or v not in vtris or stamp[u] != su or stamp[v] != sv:
            continue
        r = plan(u, v)
        if r is None:
            continue
        _, mapping, attrs = r

        # The faces it leaves may not pass through any others, as the two
        # sides of a thin part would when pushed together
        place = attrs[0] if attrs is not None else pos[v]
        changed = [[place if c[3] in (u, v) else c[0] for c in tris[t]]
                   for t in vtris[u] | vtris[v] if not (u in {c[3] for c in tris[t]} and v in {c[3] for c in tris[t]})]
        near = vtris[u] | vtris[v]
        crossing = False
        for new in changed:
            lo = [min(p[i] for p in new) for i in range(3)]
            hi = [max(p[i] for p in new) for i in range(3)]
            for t, tri in enumerate(tris):
                if not alive[t] or t in near:
                    continue
                pts = [c[0] for c in tri]
                if any(max(p[i] for p in pts) < lo[i] or min(p[i] for p in pts) > hi[i] for i in range(3)):
                    continue
                if triangles_cross(new, pts):
                    crossing = True
                    break
            if crossing:
                break
        if crossing:
            continue
        for t in list(vtris[u]):
            if v in {c[3] for c in tris[t]}:
                alive[t] = False
                count -= 1
                for c in tris[t]:
                    vtris[c[3]].discard(t)
            else:
                tris[t] = [mapping[wedge(c)] if c[3] == u else c for c in tris[t]]
                vtris[v].add(t)
        del vtris[u]
        if attrs is not None:
            # The one wedge of v, moved
            pos[v] = attrs[0]
            for t in vtris[v]:
                tris[t] = [attrs if c[3] == v else c for c in tris[t]]
        Q[v] = [x + y for x, y in zip(Q[u], Q[v])]
        ring = neighbours(v) | {v}
        for w in ring:
            stamp[w] += 1
        for w in ring:
            for x in neighbours(w):
                push(w, x)
                push(x, w)
    return [tri for t, tri in enumerate(tris) if alive[t]]

# --- Merging coplanar faces ---

def fan(polys):
    """Triangles of each polygon as it is, fanned from its first corner"""
    tris = []
    for corners in polys:
        c = [x[:4] for x in corners]
        for k in range(1, len(c) - 1):
            tris.append([c[0], c[k], c[k + 1]])
    return tris

def sub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]

def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]

def poly_normal(pts):
    # Newell's method, for polygons that are not quite flat
    n = [0.0, 0.0, 0.0]
    for k in range(len(pts)):
        a = pts[k]
        c = pts[(k + 1) % len(pts)]
        n[0] += (a[1] - c[1]) * (a[2] + c[2])
        n[1] += (a[2] - c[2]) * (a[0] + c[0])
        n[2] += (a[0] - c[0]) * (a[1] + c[1])
    area = math.sqrt(dot(n, n)) * 0.5
    return normalize(n), area

def plane_basis(n):
    a = [1.0, 0.0, 0.0] if abs(n[0]) < 0.9 else [0.0, 1.0, 0.0]
    e1 = normalize(cross(a, n))
    return e1, cross(n, e1)

def fit_uv_map(pts, uvs, e1, e2):
    """Least squares affine map from plane coordinates to uv, as two rows of
    (a, b, c) with u = a s + b t + c, or None when the points are degenerate"""
    rows = [(dot(p, e1), dot(p, e2), 1.0) for p in pts]
    ata = [[sum(r[i] * r[j] for r in rows) for j in range(3)] for i in range(3)]
    det = (ata[0][0] * (ata[1][1] * ata[2][2] - ata[1][2] * ata[2][1])
           - ata[0][1] * (ata[1][0] * ata[2][2] - ata[1][2] * ata[2][0])
           + ata[0][2] * (ata[1][0] * ata[2][1] - ata[1][1] * ata[2][0]))
    if abs(det) < 1e-18:
        return None
    out = []
    for comp in range(2):
        atb = [sum(r[i] * uv[comp] for r, uv in zip(rows, uvs)) for i in range(3)]
        sol = []
        for col in range(3):
            m = [row[:] for row in ata]
            for i in range(3):
                m[i][col] = atb[i]
            d = (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
                 - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                 + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))
            sol.append(d / det)
        out.append(sol)
    return out

def same_normal(a, b, tol=1e-3):
    if a is None or b is None:
        return a is None and b is None
    return abs(a[0] - b[0]) < tol and abs(a[1] - b[1]) < tol and abs(a[2] - b[2]) < tol

def ear_clip(loop2d):
    """Triangles, as index triples, of a simple polygon given counter-
    clockwise, or None if it can't be cut up"""
    idx = list(range(len(loop2d)))
    out = []

    def area2(a, b, c):
        return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    guard = 0
    while len(idx) > 3:
        guard += 1
        if guard > 10000:
            return None
        best = None
        for k in range(len(idx)):
            i0, i1, i2 = idx[k - 1], idx[k], idx[(k + 1) % len(idx)]
            a, b, c = loop2d[i0], loop2d[i1], loop2d[i2]
            ar = area2(a, b, c)
            if ar <= 1e-12:
                continue
            inside = False
            for j in idx:
                if j in (i0, i1, i2):
                    continue
                p = loop2d[j]
                if area2(a, b, p) >= 0 and area2(b, c, p) >= 0 and area2(c, a, p) >= 0:
                    inside = True
                    break
            if inside:
                continue
            # Prefer the ear whose smallest angle is largest, to avoid slivers
            def ang(p, q, r):
                u = (q[0] - p[0], q[1] - p[1])
                v = (r[0] - p[0], r[1] - p[1])
                lu = math.hypot(*u) or 1.0
                lv = math.hypot(*v) or 1.0
                return math.acos(max(-1.0, min(1.0, (u[0] * v[0] + u[1] * v[1]) / (lu * lv))))
            q = min(ang(a, b, c), ang(b, c, a), ang(c, a, b))
            if best is None or q > best[0]:
                best = (q, k)
        if best is None:
            return None
        k = best[1]
        out.append((idx[k - 1], idx[k], idx[(k + 1) % len(idx)]))
        del idx[k]
    out.append(tuple(idx))
    return out

def merge_coplanar(polys, max_angle, uv_tol):
    """Merge adjacent polygons that are within max_angle radians of the plane
    of the first, whose uvs follow one affine map and whose normals agree
    along the edges they share, and cut each merged region up again from its
    outline. A vertex is only dropped where every region around it either
    covers it or passes straight through it, so that no crack opens."""
    n = len(polys)
    pts = [[c[0] for c in p] for p in polys]
    info = [poly_normal(p) for p in pts]
    allp = [q for p in pts for q in p]
    diag = math.sqrt(sum((max(q[i] for q in allp) - min(q[i] for q in allp)) ** 2 for i in range(3)))
    dist_tol = diag * 1e-3
    cos_tol = math.cos(max_angle)

    edge_polys = {}
    for f, p in enumerate(polys):
        for k in range(len(p)):
            e = tuple(sorted((p[k][3], p[(k + 1) % len(p)][3])))
            edge_polys.setdefault(e, []).append(f)

    # Grow regions from the largest faces down
    region = [-1] * n
    regions = []
    for seed in sorted(range(n), key=lambda f: -info[f][1]):
        if region[seed] >= 0:
            continue
        nrm = info[seed][0]
        origin = pts[seed][0]
        e1, e2 = plane_basis(nrm)
        uvmap = fit_uv_map(pts[seed], [c[2] for c in polys[seed]], e1, e2)
        smooth = polys[seed][0][1] is not None
        r = len(regions)
        members = [seed]
        region[seed] = r
        stack = [seed]
        while stack and uvmap is not None:
            f = stack.pop()
            p = polys[f]
            for k in range(len(p)):
                a, b = p[k], p[(k + 1) % len(p)]
                for g in edge_polys[tuple(sorted((a[3], b[3])))]:
                    if region[g] >= 0 or g == f:
                        continue
                    q = polys[g]
                    if dot(info[g][0], nrm) < cos_tol or (q[0][1] is not None) != smooth:
                        continue
                    if any(abs(dot(sub(c[0], origin), nrm)) > dist_tol for c in q):
                        continue
                    ok = True
                    for c in q:
                        s, t = dot(c[0], e1), dot(c[0], e2)
                        u = uvmap[0][0] * s + uvmap[0][1] * t + uvmap[0][2]
                        v = uvmap[1][0] * s + uvmap[1][1] * t + uvmap[1][2]
                        if abs(u - c[2][0]) > uv_tol or abs(v - c[2][1]) > uv_tol:
                            ok = False
                            break
                    # Normals have to agree where the two meet
                    for c in q:
                        if not ok:
                            break
                        if c[3] in (a[3], b[3]):
                            mine = a if c[3] == a[3] else b
                            ok = same_normal(c[1], mine[1])
                    if ok:
                        region[g] = r
                        members.append(g)
                        stack.append(g)
        regions.append(dict(members=members, normal=nrm, e1=e1, e2=e2))

    # The outline of each region, as one loop of corners; a region with a
    # hole or that touches itself at a corner falls back to its faces
    loops = []
    for reg in regions:
        if len(reg['members']) == 1:
            loops.append([(polys[reg['members'][0]], None)])
            continue
        directed = set()
        corner_of = {}
        for f in reg['members']:
            p = polys[f]
            for k in range(len(p)):
                directed.add((p[k][3], p[(k + 1) % len(p)][3]))
                corner_of.setdefault(p[k][3], p[k])
        nxt = {}
        bad = False
        for (a, b) in directed:
            if (b, a) in directed:
                continue
            if a in nxt:
                bad = True
                break
            nxt[a] = b
        loop = []
        if not bad and nxt:
            start = next(iter(nxt))
            v = start
            while True:
                loop.append(v)
                v = nxt.get(v)
                if v is None or len(loop) > len(nxt):
                    bad = True
                    break
                if v == start:
                    break
            if len(loop) != len(nxt):
                bad = True
        if bad:
            loops.append([(polys[f], None) for f in reg['members']])
        else:
            loops.append([([corner_of[v] for v in loop], reg)])

    # Drop vertices that every loop holding them passes straight through
    sin_tol = math.sin(math.radians(0.5))
    straight = {}
    for group in loops:
        for corners, _ in group:
            m = len(corners)
            for k in range(m):
                a, b, c = corners[k - 1][0], corners[k][0], corners[(k + 1) % m][0]
                u = sub(b, a)
                w = sub(c, b)
                lu = math.sqrt(dot(u, u))
                lw = math.sqrt(dot(w, w))
                cr = cross(u, w)
                st = lu > 0 and lw > 0 and dot(u, w) > 0 and math.sqrt(dot(cr, cr)) <= sin_tol * lu * lw
                vid = corners[k][3]
                straight[vid] = straight.get(vid, True) and st

    tris = []
    for group in loops:
        for corners, reg in group:
            kept = [c for c in corners if not straight.get(c[3], False)]
            if reg is None and len(kept) == len(corners):
                tris += fan([corners])
                continue
            if len(kept) < 3:
                kept = corners
            nrm = reg['normal'] if reg else poly_normal([c[0] for c in corners])[0]
            e1, e2 = plane_basis(nrm)
            loop2d = [(dot(c[0], e1), dot(c[0], e2)) for c in kept]
            flipped = sum(loop2d[k - 1][0] * loop2d[k][1] - loop2d[k][0] * loop2d[k - 1][1]
                          for k in range(len(loop2d))) < 0
            if flipped:
                loop2d.reverse()
                kept = kept[::-1]
            cut = ear_clip(loop2d)
            if cut is not None and flipped:
                # Back to the winding the faces had
                cut = [(i, k, j) for i, j, k in cut]
            if cut is None:
                # Keep the faces as they were, vertices and all
                members = reg['members'] if reg else None
                tris += fan([polys[f] for f in members] if members else [corners])
                continue
            for i, j, k in cut:
                tris.append([kept[i][:4], kept[j][:4], kept[k][:4]])
    return tris, len(regions)

# --- Images ---

def png_decode(d):
    if d[:8] != b'\x89PNG\r\n\x1a\n':
        sys.exit('texture is not a PNG')
    p = 8
    idat = b''
    palette = None
    trns = None
    while p < len(d):
        n, t = struct.unpack_from('>I4s', d, p)
        body = d[p + 8:p + 8 + n]
        if t == b'IHDR':
            w, h, depth, ctype, _, _, interlace = struct.unpack('>IIBBBBB', body)
        elif t == b'PLTE':
            palette = body
        elif t == b'tRNS':
            trns = body
        elif t == b'IDAT':
            idat += body
        p += 12 + n
    if depth != 8 or interlace != 0:
        sys.exit('only 8 bit, non-interlaced PNG textures are supported')
    bpp = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ctype]
    raw = zlib.decompress(idat)
    stride = w * bpp
    rows = bytearray(h * stride)
    prev = bytearray(stride)
    p = 0
    for y in range(h):
        f = raw[p]
        line = bytearray(raw[p + 1:p + 1 + stride])
        p += 1 + stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                pa = abs(b - c)
                pb = abs(a - c)
                pc = abs(a + b - 2 * c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else (b if pb <= pc else c))) & 255
        rows[y * stride:(y + 1) * stride] = line
        prev = line

    rgba = bytearray(w * h * 4)
    for i in range(w * h):
        if ctype == 6:
            rgba[i * 4:i * 4 + 4] = rows[i * 4:i * 4 + 4]
        elif ctype == 2:
            rgba[i * 4:i * 4 + 3] = rows[i * 3:i * 3 + 3]
            rgba[i * 4 + 3] = 255
        elif ctype == 0:
            rgba[i * 4:i * 4 + 3] = bytes([rows[i]]) * 3
            rgba[i * 4 + 3] = 255
        elif ctype == 4:
            rgba[i * 4:i * 4 + 3] = bytes([rows[i * 2]]) * 3
            rgba[i * 4 + 3] = rows[i * 2 + 1]
        else:
            k = rows[i]
            rgba[i * 4:i * 4 + 3] = palette[k * 3:k * 3 + 3]
            rgba[i * 4 + 3] = trns[k] if trns and k < len(trns) else 255
    return w, h, rgba

def downscale(w, h, px, max_size, by_alpha=False):
    """Box filter by a whole factor until both sides fit; by_alpha weighs
    colours by their alpha, so clear texels do not darken what they border"""
    f = 1
    while -(-w // f) > max_size or -(-h // f) > max_size:
        f += 1
    if f == 1:
        return w, h, bytearray(px)
    if by_alpha:
        nw, nh = w // f, h // f
        out = bytearray(nw * nh * 4)
        for y in range(nh):
            for x in range(nw):
                acc = [0, 0, 0, 0]
                for sy in range(f):
                    row = (y * f + sy) * w
                    for sx in range(f):
                        i = (row + x * f + sx) * 4
                        a = px[i + 3]
                        for c in range(3):
                            acc[c] += px[i + c] * a
                        acc[3] += a
                o = (y * nw + x) * 4
                if acc[3]:
                    for c in range(3):
                        out[o + c] = (acc[c] + acc[3] // 2) // acc[3]
                out[o + 3] = (acc[3] + f * f // 2) // (f * f)
        return nw, nh, out
    nw, nh = w // f, h // f
    out = bytearray(nw * nh * 4)
    for y in range(nh):
        for x in range(nw):
            acc = [0, 0, 0, 0]
            for sy in range(f):
                row = (y * f + sy) * w
                for sx in range(f):
                    i = (row + x * f + sx) * 4
                    for c in range(4):
                        acc[c] += px[i + c]
            for c in range(4):
                out[(y * nw + x) * 4 + c] = (acc[c] + f * f // 2) // (f * f)
    return nw, nh, out

def pad_islands(w, h, px, passes=4):
    """An opaque copy, with the colours of texels that were not clear spread
    a few texels into the clear ones around them"""
    px = bytearray(px)
    filled = [px[i * 4 + 3] > 0 for i in range(w * h)]
    for _ in range(passes):
        grown = []
        for y in range(h):
            for x in range(w):
                if filled[y * w + x]:
                    continue
                acc = [0, 0, 0]
                n = 0
                for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                    if 0 <= nx < w and 0 <= ny < h and filled[ny * w + nx]:
                        j = (ny * w + nx) * 4
                        for c in range(3):
                            acc[c] += px[j + c]
                        n += 1
                if n:
                    grown.append((y * w + x, [(v + n // 2) // n for v in acc]))
        for i, rgb in grown:
            px[i * 4:i * 4 + 3] = bytes(rgb)
            filled[i] = True
    px[3::4] = b'\xff' * (w * h)
    return px

def png_encode(w, h, px):
    opaque = all(px[i] == 255 for i in range(3, len(px), 4))
    bpp = 3 if opaque else 4
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        for x in range(w):
            i = (y * w + x) * 4
            raw += px[i:i + bpp]

    def chunk(t, body):
        return struct.pack('>I', len(body)) + t + body + struct.pack('>I', zlib.crc32(t + body) & 0xffffffff)
    ihdr = struct.pack('>IIBBBBB', w, h, 8, 2 if opaque else 6, 0, 0, 0)
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', ihdr) + chunk(b'IDAT', zlib.compress(bytes(raw), 9))
            + chunk(b'IEND', b''))

# The example quantization tables of the JPEG standard (Annex K), in row order
JPEG_LUMA_Q = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55,
    14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51, 87, 80, 62,
    18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92,
    49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99]
JPEG_CHROMA_Q = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99,
    24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99, 99, 99, 99] + [99] * 32

def zigzag_order():
    """Row order indices of an 8x8 block, in zigzag order"""
    out = []
    for s in range(15):
        diag = [(y, s - y) for y in range(8) if 0 <= s - y < 8]
        if s % 2 == 0:
            diag.reverse()
        out += [y * 8 + x for y, x in diag]
    return out

def huffman_table(freq):
    """The BITS counts and HUFFVAL symbols of a Huffman table for these symbol
    counts, with no code longer than 16 bits and none all ones, as the JPEG
    standard builds them (Annex K.2)"""
    import heapq
    freq = dict(freq)
    freq[256] = 0   # Reserves the all-ones code; sorts last
    heap = [(f, s, [s]) for s, f in freq.items()]
    heapq.heapify(heap)
    size = {s: 0 for s in freq}
    if len(heap) == 1:
        size = {s: 1 for s in freq}
    while len(heap) > 1:
        f0, s0, g0 = heapq.heappop(heap)
        f1, s1, g1 = heapq.heappop(heap)
        for s in g0 + g1:
            size[s] += 1
        heapq.heappush(heap, (f0 + f1, min(s0, s1), g0 + g1))
    bits = [0] * 33
    for s, n in size.items():
        bits[n] += 1
    i = 32
    while i > 16:
        while bits[i] > 0:
            j = i - 2
            while bits[j] == 0:
                j -= 1
            bits[i] -= 2
            bits[i - 1] += 1
            bits[j + 1] += 2
            bits[j] -= 1
        i -= 1
    # Take the reserved code back out
    i = 16
    while bits[i] == 0:
        i -= 1
    bits[i] -= 1
    order = sorted(freq, key=lambda s: (size[s], s == 256, s))
    return bits[1:17], [s for s in order if s != 256]

def huffman_codes(bits, vals):
    codes = {}
    code = 0
    k = 0
    for n in range(1, 17):
        for _ in range(bits[n - 1]):
            codes[vals[k]] = (code, n)
            code += 1
            k += 1
        code <<= 1
    return codes

def jpeg_encode(w, h, px, quality):
    """A baseline JPEG of RGBA pixels: YCbCr with chroma at half resolution
    both ways, and Huffman tables made for this image"""
    scale = 5000 // quality if quality < 50 else 200 - 2 * quality
    qtabs = [[max(1, min(255, (q * scale + 50) // 100)) for q in t] for t in (JPEG_LUMA_Q, JPEG_CHROMA_Q)]
    zz = zigzag_order()

    # Planes padded with their edge to whole 16x16 units
    pw = (w + 15) // 16 * 16
    ph = (h + 15) // 16 * 16
    Y = [0.0] * (pw * ph)
    Cb = [0.0] * (pw * ph)
    Cr = [0.0] * (pw * ph)
    for y in range(ph):
        sy = min(y, h - 1)
        for x in range(pw):
            i = (sy * w + min(x, w - 1)) * 4
            r, g, b = px[i], px[i + 1], px[i + 2]
            j = y * pw + x
            Y[j] = 0.299 * r + 0.587 * g + 0.114 * b
            Cb[j] = -0.168736 * r - 0.331264 * g + 0.5 * b + 128.0
            Cr[j] = 0.5 * r - 0.418688 * g - 0.081312 * b + 128.0
    cw, ch = pw // 2, ph // 2

    def half(plane):
        return [(plane[(2 * y) * pw + 2 * x] + plane[(2 * y) * pw + 2 * x + 1]
                 + plane[(2 * y + 1) * pw + 2 * x] + plane[(2 * y + 1) * pw + 2 * x + 1]) * 0.25
                for y in range(ch) for x in range(cw)]
    Cb = half(Cb)
    Cr = half(Cr)

    cos_t = [[(math.sqrt(0.125) if u == 0 else 0.5) * math.cos((2 * x + 1) * u * math.pi / 16)
              for x in range(8)] for u in range(8)]

    def block(plane, stride, bx, by, qt):
        """Quantized DCT coefficients of one block, in zigzag order"""
        rows = [[plane[(by + y) * stride + bx + x] - 128.0 for x in range(8)] for y in range(8)]
        tmp = [[sum(cos_t[u][x] * row[x] for x in range(8)) for u in range(8)] for row in rows]
        coef = [0] * 64
        for v in range(8):
            cv = cos_t[v]
            for u in range(8):
                val = sum(cv[y] * tmp[y][u] for y in range(8))
                coef[v * 8 + u] = int(round(val / qt[v * 8 + u]))
        return [coef[k] for k in zz]

    # Blocks in scan order: four of Y, then Cb, then Cr, per 16x16 unit
    blocks = []
    for my in range(0, ph, 16):
        for mx in range(0, pw, 16):
            for dy in (0, 8):
                for dx in (0, 8):
                    blocks.append((0, block(Y, pw, mx + dx, my + dy, qtabs[0])))
            blocks.append((1, block(Cb, cw, mx // 2, my // 2, qtabs[1])))
            blocks.append((2, block(Cr, cw, mx // 2, my // 2, qtabs[1])))

    # Symbols and their extra bits, per block, counted for the tables
    def category(v):
        return abs(v).bit_length()

    def extra(v, n):
        return v if v >= 0 else v + (1 << n) - 1
    prev = [0, 0, 0]
    coded = []
    counts = [{}, {}, {}, {}]   # DC luma, AC luma, DC chroma, AC chroma
    for comp, z in blocks:
        t = 0 if comp == 0 else 2
        syms = []
        diff = z[0] - prev[comp]
        prev[comp] = z[0]
        n = category(diff)
        syms.append((t, n, extra(diff, n), n))
        run = 0
        last = max([k for k in range(1, 64) if z[k] != 0], default=0)
        for k in range(1, last + 1):
            if z[k] == 0:
                run += 1
                continue
            while run > 15:
                syms.append((t + 1, 0xF0, 0, 0))
                run -= 16
            n = category(z[k])
            syms.append((t + 1, (run << 4) | n, extra(z[k], n), n))
            run = 0
        if last < 63:
            syms.append((t + 1, 0x00, 0, 0))
        for tab, s, _, _ in syms:
            counts[tab][s] = counts[tab].get(s, 0) + 1
        coded.append(syms)

    tables = [huffman_table(c) for c in counts]
    codes = [huffman_codes(*t) for t in tables]

    # The entropy coded data, with a zero after every 0xFF byte
    out = bytearray()
    acc = 0
    nacc = 0
    for syms in coded:
        for tab, s, val, n in syms:
            code, length = codes[tab][s]
            acc = (acc << length) | code
            nacc += length
            if n:
                acc = (acc << n) | val
                nacc += n
            while nacc >= 8:
                nacc -= 8
                byte = (acc >> nacc) & 0xFF
                out.append(byte)
                if byte == 0xFF:
                    out.append(0)
            acc &= (1 << nacc) - 1
    if nacc:
        byte = ((acc << (8 - nacc)) | ((1 << (8 - nacc)) - 1)) & 0xFF
        out.append(byte)
        if byte == 0xFF:
            out.append(0)

    def segment(marker, body):
        return bytes([0xFF, marker]) + struct.pack('>H', len(body) + 2) + body
    head = b'\xff\xd8' + segment(0xE0, b'JFIF\0\x01\x01\0\0\x01\0\x01\0\0')
    for k, qt in enumerate(qtabs):
        head += segment(0xDB, bytes([k]) + bytes(qt[i] for i in zz))
    head += segment(0xC0, struct.pack('>BHHB', 8, h, w, 3) + bytes([1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]))
    for (bits, vals), cls_id in zip(tables, (0x00, 0x10, 0x01, 0x11)):
        head += segment(0xC4, bytes([cls_id]) + bytes(bits) + bytes(vals))
    head += segment(0xDA, bytes([3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]))
    return head + bytes(out) + b'\xff\xd9'

def jpeg_info(d):
    """Width, height and whether it is progressive"""
    p = 2
    while p + 4 <= len(d):
        if d[p] != 0xFF:
            p += 1
            continue
        marker = d[p + 1]
        length = struct.unpack_from('>H', d, p + 2)[0]
        if 0xC0 <= marker <= 0xCF and marker not in (0xC4, 0xC8, 0xCC):
            h, w = struct.unpack_from('>HH', d, p + 5)
            return w, h, marker != 0xC0 and marker != 0xC1
        p += 2 + length
    sys.exit('could not read the JPEG texture size')

def jpeg_decode(data):
    """Decode a JPEG with lib/std/jpeg.psh, run by this repository's plush"""
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    plush = None
    for build in ('release', 'debug'):
        p = os.path.join(root, 'target', build, 'plush')
        if os.path.isfile(p):
            plush = p
            break
    if plush is None:
        sys.exit('a JPEG texture has to be decoded to be scaled down, which needs plush: run cargo build --release')
    # Inside the repository, as plush only reads files under where it runs
    with tempfile.TemporaryDirectory(dir=root, prefix='.model_to_glb_') as tmp:
        src = os.path.join(tmp, 'in.jpg')
        dst = os.path.join(tmp, 'out.tga')
        script = os.path.join(tmp, 'decode.psh')
        open(src, 'wb').write(data)
        open(script, 'w').write(
            'from jpeg import load_jpeg;\nfrom image import save_tga;\n'
            'if (!save_tga(%s, load_jpeg(%s))) $exit(1);\n' % (json.dumps(dst), json.dumps(src)))
        res = subprocess.run([plush, script], cwd=root, capture_output=True, text=True)
        if res.returncode != 0 or not os.path.isfile(dst):
            sys.exit('could not decode the JPEG texture:\n' + res.stdout + res.stderr)
        tga = open(dst, 'rb').read()

    # Uncompressed, 32 bit BGRA, rows top-down, as save_tga() writes
    w, h = struct.unpack_from('<HH', tga, 12)
    px = bytearray(tga[18:18 + w * h * 4])
    for i in range(0, len(px), 4):
        px[i], px[i + 2] = px[i + 2], px[i]
        px[i + 3] = 255
    return w, h, px

def prepare_texture(data, max_size, fmt, quality, drop_alpha=False):
    """The image to embed, its mime type, and a description. A JPEG that fits
    is embedded as it is; anything else is written as fmt, except that an
    image with transparency stays PNG, as JPEG has none, unless drop_alpha."""
    if data[:2] == b'\xff\xd8':
        w, h, progressive = jpeg_info(data)
        if progressive:
            sys.exit('the JPEG texture is progressive, which lib/std/jpeg.psh does not decode; convert it to PNG')
        if max(w, h) <= max_size and fmt == 'jpeg':
            return data, 'image/jpeg', '%dx%d JPEG, embedded as is' % (w, h)
        w, h, px = jpeg_decode(data)
        kind = 'JPEG'
    else:
        w, h, px = png_decode(data)
        kind = 'PNG'
    if drop_alpha:
        nw, nh, px = downscale(w, h, px, max_size, by_alpha=True)
        px = pad_islands(nw, nh, px)
    else:
        nw, nh, px = downscale(w, h, px, max_size)
    opaque = all(px[i] == 255 for i in range(3, len(px), 4))
    if fmt == 'jpeg' and not opaque:
        print('note: the texture has transparency, so it stays PNG; --drop-alpha makes it opaque')
    if fmt == 'jpeg' and opaque:
        out = jpeg_encode(nw, nh, px, quality)
        return out, 'image/jpeg', '%dx%d %s -> %dx%d JPEG, quality %d, %d bytes' % (w, h, kind, nw, nh, quality, len(out))
    out = png_encode(nw, nh, px)
    return out, 'image/png', '%dx%d %s -> %dx%d PNG, %d bytes' % (w, h, kind, nw, nh, len(out))

# --- Orienting ---

def normalize(v):
    n = math.sqrt(sum(x * x for x in v)) or 1.0
    return [x / n for x in v]

def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]

def axis_vec(name):
    m = re.fullmatch(r'([+-]?)([xyz])', name)
    if not m:
        sys.exit('bad axis %r, expected one of +x -x +y -y +z -z' % name)
    v = [0.0, 0.0, 0.0]
    v['xyz'.index(m.group(2))] = -1.0 if m.group(1) == '-' else 1.0
    return v

def orientation(up, front, rotations):
    """A 3x3 matrix taking source axes to output axes"""
    U = axis_vec(up) if up else None
    F = axis_vec(front) if front else None
    if U is None and F is None:
        R = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    else:
        # The source axes that are to become +Y and +Z, filling in one if not given
        if U is None:
            U = [0.0, 1.0, 0.0] if abs(F[1]) < 0.5 else [0.0, 0.0, -F[1]]
        if F is None:
            F = [0.0, 0.0, 1.0] if abs(U[2]) < 0.5 else [0.0, -U[2], 0.0]
        if abs(sum(U[i] * F[i] for i in range(3))) > 0.5:
            sys.exit('--up and --front must be different axes')
        X = cross(U, F)
        R = [X, U, F]   # Rows: what each output axis is in the source
    for spec in rotations:
        m = re.fullmatch(r'([xyz]):(-?[0-9.]+)', spec)
        if not m:
            sys.exit('bad rotation %r, expected e.g. z:90' % spec)
        a = math.radians(float(m.group(2)))
        c, s = math.cos(a), math.sin(a)
        i = 'xyz'.index(m.group(1))
        j, k = (i + 1) % 3, (i + 2) % 3
        rot = [[1.0 if r == q else 0.0 for q in range(3)] for r in range(3)]
        rot[j][j] = c
        rot[j][k] = -s
        rot[k][j] = s
        rot[k][k] = c
        R = [[sum(rot[r][q] * R[q][t] for q in range(3)) for t in range(3)] for r in range(3)]
    return R

# --- Output ---

def write_glb(path, tris, image, mime, name, skin=None):
    pos, nor, uv, idx, vids = [], [], [], [], []
    keys = {}
    for tri in tris:
        p = [c[0] for c in tri]
        fn = normalize(cross([p[1][i] - p[0][i] for i in range(3)], [p[2][i] - p[0][i] for i in range(3)]))
        for c in tri:
            n = c[1] if c[1] is not None else fn
            key = (tuple(round(x, 6) for x in c[0]), tuple(round(x, 4) for x in n), tuple(round(x, 6) for x in c[2]),
                   tuple(skin['weights'].get(c[3], ())) if skin else None)
            if key not in keys:
                keys[key] = len(pos)
                pos.append(c[0])
                nor.append(n)
                uv.append(c[2])
                vids.append(c[3])
            idx.append(keys[key])

    views = []
    accessors = []
    chunks = []
    size = [0]

    def add(data, comp, count, kind, **extra):
        size[0] += -size[0] % 4
        views.append({'buffer': 0, 'byteOffset': size[0], 'byteLength': len(data)})
        chunks.append((size[0], data))
        size[0] += len(data)
        if comp is None:
            return len(views) - 1
        accessors.append(dict({'bufferView': len(views) - 1, 'componentType': comp, 'count': count, 'type': kind}, **extra))
        return len(accessors) - 1

    mn = [min(v[i] for v in pos) for i in range(3)]
    mx = [max(v[i] for v in pos) for i in range(3)]
    big = len(pos) > 65535
    attrs = {'POSITION': add(b''.join(struct.pack('<3f', *v) for v in pos), 5126, len(pos), 'VEC3', min=mn, max=mx),
             'NORMAL': add(b''.join(struct.pack('<3f', *v) for v in nor), 5126, len(nor), 'VEC3'),
             'TEXCOORD_0': add(b''.join(struct.pack('<2f', *v) for v in uv), 5126, len(uv), 'VEC2')}
    indices = add(struct.pack('<%d%s' % (len(idx), 'I' if big else 'H'), *idx), 5125 if big else 5123, len(idx), 'SCALAR')

    pbr = {'metallicFactor': 0.0, 'roughnessFactor': 1.0}
    js = {
        'asset': {'version': '2.0', 'generator': 'scripts/model_to_glb.py'},
        'scene': 0,
        'scenes': [{'nodes': [0]}],
        'nodes': [{'mesh': 0, 'name': name}],
        'meshes': [{'name': name, 'primitives': [{'attributes': attrs, 'indices': indices, 'material': 0}]}],
        'materials': [{'name': name, 'pbrMetallicRoughness': pbr}],
    }
    if image:
        pbr['baseColorTexture'] = {'index': 0}
        js['textures'] = [{'source': 0, 'sampler': 0}]
        js['samplers'] = [{'magFilter': 9728, 'minFilter': 9728}]
        js['images'] = [{'bufferView': add(image, None, 0, None), 'mimeType': mime}]
    if skin:
        write_skin(js, skin, vids, attrs, add)
    js['accessors'] = accessors
    js['bufferViews'] = views

    bin_chunk = bytearray(size[0] + (-size[0] % 4))
    for off, data in chunks:
        bin_chunk[off:off + len(data)] = data
    js['buffers'] = [{'byteLength': len(bin_chunk)}]
    text = json.dumps(js, separators=(',', ':')).encode()
    text += b' ' * (-len(text) % 4)
    total = 12 + 8 + len(text) + 8 + len(bin_chunk)
    with open(path, 'wb') as f:
        f.write(struct.pack('<III', 0x46546C67, 2, total))
        f.write(struct.pack('<II', len(text), 0x4E4F534A) + text)
        f.write(struct.pack('<II', len(bin_chunk), 0x004E4942) + bin_chunk)
    return len(pos), len(idx) // 3, mn, mx, total

def write_skin(js, skin, vids, attrs, add):
    """Adds the joints, the weights of the vertices and the animations"""
    joints = skin['joints']
    root = skin['root']
    base = len(js['nodes'])

    # A node that places the skeleton, then a node for each joint
    flat = lambda m: [m[r][c] for c in range(4) for r in range(4)]
    js['nodes'].append({'name': 'Armature', 'matrix': flat(root), 'children': []})
    for n, j in enumerate(joints):
        node = {'name': j['name'], 'translation': j['t'], 'rotation': j['q'][1:] + j['q'][:1]}
        if any(abs(x - 1.0) > 1e-6 for x in j['s']):
            node['scale'] = j['s']
        js['nodes'].append(node)
        parent = base + 1 + j['parent'] if j['parent'] >= 0 else base
        js['nodes'][parent].setdefault('children', []).append(base + 1 + n)
    js['scenes'][0]['nodes'].append(base)

    jdata, wdata = b'', b''
    for v in vids:
        w = skin['weights'].get(v) or [(0, 1.0)]
        w = (w + [(0, 0.0)] * 4)[:4]
        jdata += struct.pack('<4H', *(j for j, _ in w))
        wdata += struct.pack('<4f', *(x for _, x in w))
    attrs['JOINTS_0'] = add(jdata, 5123, len(vids), 'VEC4')
    attrs['WEIGHTS_0'] = add(wdata, 5126, len(vids), 'VEC4')
    ibm = b''.join(struct.pack('<16f', *flat(m4_inv(m4_mul(root, j['rest'])))) for j in joints)
    js['skins'] = [{'joints': [base + 1 + n for n in range(len(joints))], 'skeleton': base,
                    'inverseBindMatrices': add(ibm, 5126, len(joints), 'MAT4')}]
    js['nodes'][0]['skin'] = 0

    # Each animation, at every frame, leaving out what stays as it rests
    anims = []
    for a in skin['anims']:
        times = a['times']
        tin = add(struct.pack('<%df' % len(times), *times), 5126, len(times), 'SCALAR', min=[times[0]], max=[times[-1]])
        samplers, channels = [], []
        for n, track in enumerate(a['tracks']):
            j = joints[n]
            qs = []
            for _, q, _ in track:
                q = q[1:] + q[:1]
                if qs and sum(x * y for x, y in zip(q, qs[-1])) < 0:
                    q = [-x for x in q]
                qs.append(q)
            parts = (('translation', [t for t, _, _ in track], j['t'], 'VEC3'),
                     ('rotation', qs, j['q'][1:] + j['q'][:1], 'VEC4'),
                     ('scale', [s for _, _, s in track], [1.0, 1.0, 1.0], 'VEC3'))
            for path, vals, rest, kind in parts:
                if path == 'rotation':
                    still = all(abs(abs(sum(x * y for x, y in zip(v, rest))) - 1.0) < 1e-9 for v in vals)
                else:
                    still = all(abs(x - y) < 1e-6 for v in vals for x, y in zip(v, rest))
                if still:
                    continue
                out = add(b''.join(struct.pack('<%df' % len(v), *v) for v in vals), 5126, len(vals), kind)
                samplers.append({'input': tin, 'output': out, 'interpolation': 'LINEAR'})
                channels.append({'sampler': len(samplers) - 1, 'target': {'node': base + 1 + n, 'path': path}})
        if not channels:
            # One that stays put, for the length of the animation
            q = joints[0]['q'][1:] + joints[0]['q'][:1]
            out = add(b''.join(struct.pack('<4f', *q) for _ in times), 5126, len(times), 'VEC4')
            samplers.append({'input': tin, 'output': out, 'interpolation': 'LINEAR'})
            channels.append({'sampler': 0, 'target': {'node': base + 1, 'path': 'rotation'}})
        anims.append({'name': a['name'], 'samplers': samplers, 'channels': channels})
    if anims:
        js['animations'] = anims

def main():
    parser = argparse.ArgumentParser(description="Convert an .obj or .blend model to a GLB file")
    parser.add_argument("input", help="input .obj or .blend file")
    parser.add_argument("output", help="output .glb file")
    parser.add_argument("--texture", help="texture file, instead of the one the model names or packs")
    parser.add_argument("--max-tex-size", type=int, default=256,
                        help="largest texture side in pixels; larger textures are scaled down (default 256)")
    parser.add_argument("--tex-format", choices=["jpeg", "png"], default="jpeg",
                        help="how to store the texture (default jpeg); one with transparency stays png")
    parser.add_argument("--drop-alpha", action="store_true",
                        help="make the texture opaque, as the model loaders ignore its alpha, so it can be a JPEG")
    parser.add_argument("--jpeg-quality", type=int, default=85, metavar="1-100",
                        help="JPEG quality, as most encoders take it (default 85)")
    parser.add_argument("--up", help="source axis that becomes up (+Y), e.g. +z or -y")
    parser.add_argument("--front", help="source axis that becomes the front (+Z)")
    parser.add_argument("--rotate", action="append", default=[], metavar="AXIS:DEGREES",
                        help="then rotate about an output axis, e.g. z:90; may be repeated")
    parser.add_argument("--scale", type=float, default=1.0, help="multiply every position by this")
    parser.add_argument("--center", action="store_true", help="centre the bounding box on the origin")
    parser.add_argument("--ignore-object-rotation", action="store_true",
                        help=".blend only: use the mesh as modelled, keeping only the object's scale")
    parser.add_argument("--object", action="append", default=[], metavar="NAME",
                        help=".blend only: convert this mesh object, rather than all; may be repeated")
    parser.add_argument("--image", metavar="NAME", help=".blend only: use the image of this name as the texture")
    parser.add_argument("--static", action="store_true",
                        help=".blend only: leave out the armature and its actions, writing the mesh as it rests")
    parser.add_argument("--action", action="append", default=[], metavar="NAME",
                        help=".blend only: write this action as an animation, rather than all; may be repeated")
    parser.add_argument("--skin-from", metavar="OBJECT",
                        help=".blend only: give meshes with no weights of their own those of the closest "
                             "points of this mesh object, and animate them with its armature")
    parser.add_argument("--merge", type=float, nargs='?', const=1.0, metavar="DEGREES",
                        help="merge adjacent faces within this many degrees of coplanar (default 1) "
                             "whose texturing and shading carry across, and cut them up again")
    parser.add_argument("--target-tris", type=int, metavar="N",
                        help="simplify by collapsing edges until at most N triangles remain, "
                             "keeping uv seams, hard edges and open edges")
    parser.add_argument("--keep", action="append", default=[], metavar="AXIS<VALUE",
                        help='with --target-tris, leave the vertices in this range as they are, e.g. "y<2.8" or '
                             '"-3.4<y<2.8"; in the model\'s coordinates as loaded, Y up, before --up, --front, '
                             '--rotate, --center and --scale; may be repeated')
    parser.add_argument("--seam-texels", type=float, default=1.0, metavar="N",
                        help="with --target-tris, how far, in texels, a uv seam may be straightened (default 1); "
                             "more lets it go further, at the risk of faces showing the texture around their island")
    parser.add_argument("--hard-angle", type=float, metavar="DEGREES",
                        help="work out normals again, with hard edges where faces meet at more than this "
                             "angle; with --target-tris, the model's own hard edges no longer hold vertices")
    args = parser.parse_args()

    ext = os.path.splitext(args.input)[1].lower()
    skin = None
    if ext == '.obj':
        polys, tex, tex_name = load_obj(args.input)
    elif ext == '.blend':
        polys, tex, tex_name, skin = load_blend(args.input, args.ignore_object_rotation, args.object, args.image,
                                                not args.static, args.action, args.skin_from)
    else:
        sys.exit('unsupported input format: ' + ext)
    if not polys:
        sys.exit(args.input + ': no faces')

    if args.target_tris is not None:
        before = sum(len(p) - 2 for p in polys)
        keep = None
        if args.keep:
            # Each spec is a range on one axis, open at one end or not
            num = r'(-?[0-9.]+)'
            tests = []
            for spec in args.keep:
                s = spec.replace(' ', '')
                if m := re.fullmatch(num + r'<([xyz])<' + num, s):
                    axis, lo, hi = m.group(2), float(m.group(1)), float(m.group(3))
                elif m := re.fullmatch(r'([xyz])<' + num, s):
                    axis, lo, hi = m.group(1), -math.inf, float(m.group(2))
                elif m := re.fullmatch(r'([xyz])>' + num, s):
                    axis, lo, hi = m.group(1), float(m.group(2)), math.inf
                else:
                    sys.exit('bad --keep %r, expected e.g. "y<2.8", "y>1" or "-3.4<y<2.8"' % spec)
                tests.append(('xyz'.index(axis), lo, hi))
            keep = lambda p: any(lo < p[i] < hi for i, lo, hi in tests)
        polys = decimate(polys, args.target_tris, args.hard_angle is not None, args.max_tex_size, args.seam_texels, keep)
        print('decimated %d triangles to %d' % (before, len(polys)))
        if len(polys) > args.target_tris:
            print('warning: stopped above the target, where seams and folds allow no more')

    if args.hard_angle is not None:
        # Normals again, for smooth faces, hard where faces meet steeply
        pos = {c[3]: c[0] for p in polys for c in p}
        faces = [([c[3] for c in p], None, p[0][1] is not None) for p in polys]
        corner = split_normals(pos, faces, set(), (math.radians(args.hard_angle), False))
        polys = [[(c[0], corner[f][k], c[2], c[3]) for k, c in enumerate(p)] for f, p in enumerate(polys)]

    if args.merge is not None:
        before = sum(len(p) - 2 for p in polys)
        tris, num_regions = merge_coplanar(polys, math.radians(args.merge), 0.5 / args.max_tex_size)
        print('merged %d faces (%d triangles) into %d regions: %d triangles'
              % (len(polys), before, num_regions, len(tris)))
    else:
        tris = fan(polys)

    if args.texture:
        tex, tex_name = open(args.texture, 'rb').read(), args.texture

    R = orientation(args.up, args.front, args.rotate)
    turn = lambda v: [sum(R[i][j] * v[j] for j in range(3)) for i in range(3)]
    tris = [[(turn(c[0]), normalize(turn(c[1])) if c[1] is not None else None, c[2], c[3]) for c in tri]
            for tri in tris]
    mid = [0.0, 0.0, 0.0]
    if args.center:
        pts = [c[0] for tri in tris for c in tri]
        mid = [(min(p[i] for p in pts) + max(p[i] for p in pts)) * 0.5 for i in range(3)]
        tris = [[([c[0][i] - mid[i] for i in range(3)], c[1], c[2], c[3]) for c in tri] for tri in tris]
    if args.scale != 1.0:
        tris = [[([x * args.scale for x in c[0]], c[1], c[2], c[3]) for c in tri] for tri in tris]
    if skin:
        # The skeleton is placed as the mesh was: from Blender's axes to
        # glTF's, then turned, centred and scaled
        sc = args.scale
        place = [[R[i][j] * sc for j in range(3)] + [-mid[i] * sc] for i in range(3)] + [[0.0, 0.0, 0.0, 1.0]]
        to_gltf = [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
        skin['root'] = m4_mul(place, m4_mul(to_gltf, skin['obmat']))

    image = mime = None
    if tex:
        image, mime, desc = prepare_texture(tex, args.max_tex_size, args.tex_format, args.jpeg_quality,
                                            args.drop_alpha)
        print('texture: %s (%s)' % (desc, tex_name))
    else:
        print('warning: no texture found; use --texture')

    name = os.path.splitext(os.path.basename(args.output))[0]
    num_verts, num_tris, mn, mx, size = write_glb(args.output, tris, image, mime, name, skin)
    print('%d triangles, %d vertices' % (num_tris, num_verts))
    print('bounds: min (%.4g, %.4g, %.4g) max (%.4g, %.4g, %.4g)' % tuple(mn + mx))
    print('size: %.4g x %.4g x %.4g' % tuple(mx[i] - mn[i] for i in range(3)))
    print('wrote %s, %d bytes' % (args.output, size))

if __name__ == '__main__':
    main()
