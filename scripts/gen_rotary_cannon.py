# Build an original six-barrel rotary cannon as a skinned, quantized .glb for
# Tremor: the body on one joint, and the barrel cluster on a second joint with
# a looping "spin" animation of one full turn. The mesh is made from simple
# parts, each surface laid out in its own chart of a texture atlas, and the
# texture is painted procedurally from where each texel lands on the model:
# a material, ambient occlusion from ray casts, worn highlights along hard
# edges, and details such as bolts, fins and vents. Standard library only.
#
# Usage: gen_rotary.py texture out.png          bake the texture atlas
#        gen_rotary.py glb texture.jpg out.glb  write the model with it
# The texture is baked as a PNG and turned into a JPEG in between, e.g. with
# sips -s format jpeg -s formatOptions 80 out.png --out texture.jpg
import sys, math, struct, json, zlib, random

ATLAS = 512
PAD = 3

# --- Small vector helpers ---

def add(a, b): return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
def sub(a, b): return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
def mul(a, s): return [a[0] * s, a[1] * s, a[2] * s]
def dot(a, b): return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
def cross(a, b): return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
def norm(a):
    L = math.sqrt(dot(a, a)) or 1.0
    return [a[0] / L, a[1] / L, a[2] / L]

def basis(axis):
    # Two unit vectors at right angles to an axis and to each other
    a = norm(axis)
    up = [0.0, 1.0, 0.0] if abs(a[1]) < 0.9 else [1.0, 0.0, 0.0]
    u = norm(cross(up, a))
    v = cross(a, u)
    return u, v

# --- The mesh ---

class Mesh:
    def __init__(self):
        self.verts = []    # dicts: p, n, chart, cu, cv, joint
        self.tris = []     # (a, b, c, part)
        self.charts = {}   # name -> dict(w, h, mat, owner_part)
        self.parts = []    # name, mat

    def part(self, name, mat):
        self.parts.append((name, mat))
        return len(self.parts) - 1

    def chart(self, name, w, h, mat, part):
        # A chart shared by several parts is laid out once; the first part to
        # claim it is the one the texture is baked from
        if name not in self.charts:
            self.charts[name] = dict(w=max(w, 0.004), h=max(h, 0.004), mat=mat, part=part)
        return name

    def vert(self, p, n, chart, cu, cv, joint):
        self.verts.append(dict(p=list(p), n=norm(n), chart=chart, cu=cu, cv=cv, joint=joint))
        return len(self.verts) - 1

    def tri(self, a, b, c, part, outward=None):
        # Wind each triangle so that its front faces outward, as told by the
        # vertex normals unless a direction is given
        pa, pb, pc = self.verts[a]['p'], self.verts[b]['p'], self.verts[c]['p']
        fn = cross(sub(pb, pa), sub(pc, pa))
        want = outward if outward is not None else add(add(self.verts[a]['n'], self.verts[b]['n']), self.verts[c]['n'])
        if dot(fn, want) < 0: b, c = c, b
        self.tris.append((a, b, c, part))

    def quad(self, a, b, c, d, part, outward=None):
        self.tri(a, b, c, part, outward)
        self.tri(a, c, d, part, outward)

# A tube from p0 to p1 of radius r (or r0 at p0 and r1 at p1), smooth around
# its sides, with flat caps where asked. `inward` turns it inside out, for a
# bore. Each surface gets a chart; `key` lets several tubes share them.
def tube(m, part, joint, mat, key, p0, p1, r0, r1, sides, cap0=False, cap1=False, inward=False, a0=0.0):
    axis = sub(p1, p0)
    length = math.sqrt(dot(axis, axis))
    u, v = basis(axis)
    rmax = max(r0, r1)
    side = m.chart(key + ':side', 2 * math.pi * rmax, length, mat, part)
    sgn = -1.0 if inward else 1.0
    # The slope of a cone tips its normals along the axis
    slope = (r0 - r1) / length
    ax = norm(axis)
    ring0 = []; ring1 = []
    for i in range(sides + 1):
        a = a0 + 2 * math.pi * i / sides
        d = add(mul(u, math.cos(a)), mul(v, math.sin(a)))
        n = mul(norm(add(d, mul(ax, slope))), sgn)
        cu = (i / sides) * 2 * math.pi * rmax
        ring0.append(m.vert(add(p0, mul(d, r0)), n, side, cu, 0.0, joint))
        ring1.append(m.vert(add(p1, mul(d, r1)), n, side, cu, length, joint))
    for i in range(sides):
        m.quad(ring0[i], ring0[i + 1], ring1[i + 1], ring1[i], part)
    for which, (p, r, want) in enumerate(((p0, r0, cap0), (p1, r1, cap1))):
        if not want: continue
        out = mul(ax, -1.0 if which == 0 else 1.0)
        if inward: out = mul(out, -1.0)
        cap = m.chart(key + ':cap%d' % which, 2 * r, 2 * r, mat, part)
        c = m.vert(p, out, cap, r, r, joint)
        ring = []
        for i in range(sides):
            a = a0 + 2 * math.pi * i / sides
            ca, sa = math.cos(a), math.sin(a)
            ring.append(m.vert(add(p, add(mul(u, ca * r), mul(v, sa * r))), out, cap, r + ca * r, r + sa * r, joint))
        for i in range(sides):
            m.tri(c, ring[i], ring[(i + 1) % sides], part, out)

# A flat ring between two radii, facing along `out`, at p
def annulus(m, part, joint, mat, key, p, axis, r_in, r_out, sides, a0=0.0):
    u, v = basis(axis)
    out = norm(axis)
    ch = m.chart(key + ':ring', 2 * r_out, 2 * r_out, mat, part)
    inner = []; outer = []
    for i in range(sides):
        a = a0 + 2 * math.pi * i / sides
        ca, sa = math.cos(a), math.sin(a)
        d = add(mul(u, ca), mul(v, sa))
        inner.append(m.vert(add(p, mul(d, r_in)), out, ch, r_out + ca * r_in, r_out + sa * r_in, joint))
        outer.append(m.vert(add(p, mul(d, r_out)), out, ch, r_out + ca * r_out, r_out + sa * r_out, joint))
    for i in range(sides):
        j = (i + 1) % sides
        m.quad(inner[i], outer[i], outer[j], inner[j], part, out)

# A prism along x from x0 to x1 whose cross-section is a convex polygon in
# (y, z), flat on every side. With poly1, the section changes from poly at x0
# to poly1 at x1, corner for corner. Caps are optional.
def prism(m, part, joint, mat, key, x0, x1, poly, cap0=True, cap1=True, poly1=None):
    n = len(poly)
    poly1 = poly1 or poly
    length = x1 - x0
    cy = sum(p[0] for p in poly) / n; cz = sum(p[1] for p in poly) / n
    for i in range(n):
        a = poly[i]; b = poly[(i + 1) % n]
        a1 = poly1[i]; b1 = poly1[(i + 1) % n]
        e0 = math.hypot(b[0] - a[0], b[1] - a[1]); e1 = math.hypot(b1[0] - a1[0], b1[1] - a1[1])
        mid = ((a[0] + b[0]) / 2 - cy, (a[1] + b[1]) / 2 - cz)
        p0, p1, p3 = [x0, a[0], a[1]], [x1, a1[0], a1[1]], [x0, b[0], b[1]]
        nn = norm(cross(sub(p1, p0), sub(p3, p0)))
        if nn[1] * mid[0] + nn[2] * mid[1] < 0: nn = mul(nn, -1.0)
        # Along the side, measured on the slant; across, each end keeps its width
        slant = math.sqrt(dot(sub(p1, p0), sub(p1, p0)))
        ch = m.chart('%s:side%d' % (key, i), slant, max(e0, e1), mat, part)
        v0 = m.vert(p0, nn, ch, 0.0, 0.0, joint)
        v1 = m.vert(p1, nn, ch, slant, 0.0, joint)
        v2 = m.vert([x1, b1[0], b1[1]], nn, ch, slant, e1, joint)
        v3 = m.vert(p3, nn, ch, 0.0, e0, joint)
        m.quad(v0, v1, v2, v3, part, nn)
    for which, (x, pl, want) in enumerate(((x0, poly, cap0), (x1, poly1, cap1))):
        if not want: continue
        ys = [p[0] for p in pl]; zs = [p[1] for p in pl]
        out = [-1.0 if which == 0 else 1.0, 0.0, 0.0]
        ch = m.chart('%s:cap%d' % (key, which), max(ys) - min(ys), max(zs) - min(zs), mat, part)
        vs = [m.vert([x, p[0], p[1]], out, ch, p[0] - min(ys), p[1] - min(zs), joint) for p in pl]
        for i in range(1, n - 1):
            m.tri(vs[0], vs[i], vs[i + 1], part, out)

# A box given by its middle, half extents and a turn about z (to tilt a grip),
# flat on every face, leaving out the faces named in `skip`
def box(m, part, joint, mat, key, c, h, tilt=0.0, skip=()):
    ct, st = math.cos(tilt), math.sin(tilt)
    def place(x, y, z):
        return [c[0] + x * ct - y * st, c[1] + x * st + y * ct, c[2] + z]
    def turn(n):
        return [n[0] * ct - n[1] * st, n[0] * st + n[1] * ct, n[2]]
    faces = {
        'px': ([1, 0, 0], [(1, -1, -1), (1, 1, -1), (1, 1, 1), (1, -1, 1)], (1, 2)),
        'nx': ([-1, 0, 0], [(-1, -1, -1), (-1, -1, 1), (-1, 1, 1), (-1, 1, -1)], (2, 1)),
        'py': ([0, 1, 0], [(-1, 1, -1), (-1, 1, 1), (1, 1, 1), (1, 1, -1)], (2, 0)),
        'ny': ([0, -1, 0], [(-1, -1, -1), (1, -1, -1), (1, -1, 1), (-1, -1, 1)], (0, 2)),
        'pz': ([0, 0, 1], [(-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1)], (0, 1)),
        'nz': ([0, 0, -1], [(-1, -1, -1), (-1, 1, -1), (1, 1, -1), (1, -1, -1)], (1, 0)),
    }
    for name, (n, corners, (ia, ib)) in faces.items():
        if name in skip: continue
        nn = turn(n)
        ch = m.chart('%s:%s' % (key, name), 2 * h[ia], 2 * h[ib], mat, part)
        vs = []
        for s in corners:
            p = place(s[0] * h[0], s[1] * h[1], s[2] * h[2])
            vs.append(m.vert(p, nn, ch, (s[ia] + 1) * h[ia], (s[ib] + 1) * h[ib], joint))
        m.quad(vs[0], vs[1], vs[2], vs[3], part, nn)

# --- The gun ---
#
# Model space: the barrels run along +x with their axis on the x axis, y is up
# and z to the gun's left. One unit is about a meter.

BODY, SPIN = 0, 1
NB = 6                 # Barrels
RB = 0.052             # Radius of the circle the barrels sit on
RBAR = 0.0165          # Barrel radius
RBORE = 0.0095         # Bore radius
XB0, XM = -0.05, 0.64  # Where the barrels start (inside the hub) and end
RX0, RX1 = -0.50, -0.075  # The receiver, back to front
RW0, RW1 = 0.130, 0.130   # Its half width at the back and at the front
MX0 = -0.62               # The back of the motor
MOTOR_R = 0.098

# The barrel clamps: name, chart key, middle, length, radius. A heavy ring at
# the muzzle, one halfway along what shows of the barrels, and two bands
# evenly between them, which share a chart, being alike.
XFRONT, XMID = 0.60, 0.335
CLAMPS = [
    ('clamp_front', 'clamp_front', XFRONT, 0.03, 0.087),
    ('band0', 'band', XMID + (XFRONT - XMID) * 2 / 3, 0.018, 0.080),
    ('band1', 'band', XMID + (XFRONT - XMID) / 3, 0.018, 0.080),
    ('clamp_mid', 'clamp_mid', XMID, 0.03, 0.082),
]

def receiver_w(x):
    t = (x - RX0) / (RX1 - RX0)
    return RW0 + (RW1 - RW0) * t

def build():
    m = Mesh()

    # The barrels, the first of which owns the charts the others share
    for k in range(NB):
        a = math.radians(30 + 60 * k)
        cy, cz = RB * math.cos(a), RB * math.sin(a)
        part = m.part('barrel%d' % k, 'barrel')
        tube(m, part, SPIN, 'barrel', 'barrel', [XB0, cy, cz], [XM, cy, cz], RBAR, RBAR, 8, a0=a)
        annulus(m, part, SPIN, 'muzzle', 'muzzle', [XM, cy, cz], [1, 0, 0], RBORE, RBAR, 8, a0=a)
        tube(m, part, SPIN, 'bore', 'bore', [XM, cy, cz], [XM - 0.05, cy, cz], RBORE, RBORE, 8, cap1=True, inward=True, a0=a)

    # The axle the barrels turn about, seen between them
    part = m.part('axle', 'dark')
    tube(m, part, SPIN, 'dark', 'axle', [-0.04, 0, 0], [XM - 0.02, 0, 0], 0.024, 0.024, 8, cap1=True)

    # Clamps that hold the barrels together: a heavy ring at the muzzle, a run
    # of bands behind it, and one part way back
    for name, key, xc, length, r in CLAMPS:
        part = m.part(name, 'clamp')
        tube(m, part, SPIN, 'clamp', key, [xc - length / 2, 0, 0], [xc + length / 2, 0, 0], r, r, 12, cap0=True, cap1=True, a0=math.radians(15))

    # The rotor the barrels are seated in, and the bearing ring behind it,
    # which turn with them
    part = m.part('rotor', 'clamp')
    tube(m, part, SPIN, 'clamp', 'rotor', [-0.035, 0, 0], [0.03, 0, 0], 0.092, 0.088, 12, cap1=True, a0=math.radians(15))
    part = m.part('bearing', 'steel')
    tube(m, part, SPIN, 'steel', 'bearing', [-0.075, 0, 0], [-0.035, 0, 0], 0.084, 0.084, 12, a0=math.radians(15))

    # The receiver, a wide box with its long edges bevelled
    def octagon(w, hgt, ch):
        return [(hgt, -w + ch), (hgt, w - ch), (hgt - ch, w), (-hgt + ch, w), (-hgt, w - ch), (-hgt, -w + ch), (-hgt + ch, -w), (hgt - ch, -w)]
    part = m.part('receiver', 'gunmetal')
    prism(m, part, BODY, 'gunmetal', 'receiver', RX0, RX1, octagon(RW0, 0.105, 0.012), poly1=octagon(RW1, 0.105, 0.012))

    # A raised cover along its top
    part = m.part('cover', 'cover')
    box(m, part, BODY, 'cover', 'cover', [-0.355, 0.111, 0.0], [0.115, 0.006, 0.055], skip=('ny',))

    # A carry handle across the top near the front: two posts near the edges
    # of the receiver and a bar running from side to side between them
    part = m.part('handle', 'dark')
    hw = 0.92 * (RW1 - 0.012)
    for name, z in (('post_l', hw - 0.012), ('post_r', -(hw - 0.012))):
        box(m, part, BODY, 'dark', name, [-0.165, 0.1385, z], [0.014, 0.0335, 0.012], skip=('ny',))
    tube(m, part, BODY, 'dark', 'bar', [-0.165, 0.184, -hw], [-0.165, 0.184, hw], 0.016, 0.016, 8, cap0=True, cap1=True)

    # The drive motor behind it, ribbed for cooling, and its end cap
    part = m.part('motor', 'motor')
    tube(m, part, BODY, 'motor', 'motor', [MX0, 0.0, 0.0], [RX0, 0.0, 0.0], MOTOR_R, MOTOR_R + 0.004, 12, cap0=True, a0=math.radians(15))

    # The pistol grip at the back, raked, and the trigger
    part = m.part('grip', 'grip')
    box(m, part, BODY, 'grip', 'grip', [-0.43, -0.185, 0.0], [0.032, 0.085, 0.026], tilt=math.radians(-14), skip=('py',))
    part = m.part('trigger', 'dark')
    box(m, part, BODY, 'dark', 'trigger', [-0.365, -0.125, 0.0], [0.007, 0.022, 0.006], tilt=math.radians(10), skip=('py',))
    part = m.part('guard', 'gunmetal')
    box(m, part, BODY, 'gunmetal', 'guard', [-0.355, -0.155, 0.0], [0.055, 0.005, 0.012])
    # Its front, rising to meet the receiver and close the loop
    box(m, part, BODY, 'gunmetal', 'guard_f', [-0.305, -0.1275, 0.0], [0.005, 0.0275, 0.012], skip=('py',))

    # The front grip under the receiver
    part = m.part('fgrip', 'grip')
    tube(m, part, BODY, 'grip', 'fgrip', [-0.15, -0.10, 0.0], [-0.15, -0.255, 0.0], 0.026, 0.023, 8, cap1=True)

    # The ammunition box on the left, and the chute that feeds the belt up
    # into the receiver, both set against its flared side
    part = m.part('ammo', 'ammo')
    box(m, part, BODY, 'ammo', 'ammo', [-0.30, -0.165, -(receiver_w(-0.30) + 0.048)], [0.10, 0.075, 0.05])
    part = m.part('chute', 'chute')
    box(m, part, BODY, 'chute', 'chute', [-0.29, -0.055, -(receiver_w(-0.29) + 0.015)], [0.045, 0.045, 0.016], skip=('ny',))
    return m

# --- Laying out the charts ---

def pack(m):
    # Shelf packing at the largest texel density that fits. Charts are laid
    # on their side when taller than wide, and go in shortest side first.
    def dims(n):
        c = m.charts[n]
        return (c['h'], c['w'], True) if c['h'] > c['w'] else (c['w'], c['h'], False)
    names = sorted(m.charts, key=lambda n: -dims(n)[1] * 1000 - dims(n)[0])
    lo, hi = 50.0, 4000.0
    best = None
    for _ in range(30):
        dens = (lo + hi) / 2
        x = y = shelf = 0
        place = {}
        ok = True
        for n in names:
            cw, chh, rot = dims(n)
            w = int(math.ceil(cw * dens)) + 2 * PAD
            h = int(math.ceil(chh * dens)) + 2 * PAD
            if w > ATLAS: ok = False; break
            if x + w > ATLAS: x = 0; y += shelf; shelf = 0
            if y + h > ATLAS: ok = False; break
            place[n] = (x + PAD, y + PAD, rot)
            x += w; shelf = max(shelf, h)
        if ok: best = (dens, place); lo = dens
        else: hi = dens
    return best

# Where a point of a chart lands in the atlas, in texels
def atlas_xy(place, chart, cu, cv, dens):
    ox, oy, rot = place[chart]
    if rot: cu, cv = cv, cu
    return (ox + cu * dens, oy + cv * dens)

# --- Ray casts for occlusion ---

class Grid:
    def __init__(self, P, T, n=24):
        self.P = P; self.T = T; self.n = n
        self.mn = [min(p[j] for p in P) - 1e-4 for j in range(3)]
        mx = [max(p[j] for p in P) + 1e-4 for j in range(3)]
        self.cs = [(mx[j] - self.mn[j]) / n for j in range(3)]
        self.cells = {}
        for i, t in enumerate(T):
            a, b, c = P[t[0]], P[t[1]], P[t[2]]
            lo = [self.cell(min(a[j], b[j], c[j]), j) for j in range(3)]
            hi = [self.cell(max(a[j], b[j], c[j]), j) for j in range(3)]
            for x in range(lo[0], hi[0] + 1):
                for y in range(lo[1], hi[1] + 1):
                    for z in range(lo[2], hi[2] + 1):
                        self.cells.setdefault((x, y, z), []).append(i)
        self.pre = []
        for t in T:
            a = P[t[0]]
            self.pre.append((a, sub(P[t[1]], a), sub(P[t[2]], a)))

    def cell(self, v, j):
        return max(0, min(self.n - 1, int((v - self.mn[j]) / self.cs[j])))

    def hit(self, o, d, tmax):
        # Any triangle within tmax, walking the cells the ray passes through
        seen = set()
        steps = int(tmax / min(self.cs) * 2) + 2
        for s in range(steps + 1):
            t = tmax * s / steps
            p = add(o, mul(d, t))
            key = tuple(self.cell(p[j], j) for j in range(3))
            if key in seen: continue
            seen.add(key)
            for i in self.cells.get(key, ()):
                a, e1, e2 = self.pre[i]
                pv = cross(d, e2); det = dot(e1, pv)
                if abs(det) < 1e-12: continue
                inv = 1.0 / det; sv = sub(o, a)
                u = dot(sv, pv) * inv
                if u < 0 or u > 1: continue
                q = cross(sv, e1); v = dot(d, q) * inv
                if v < 0 or u + v > 1: continue
                tt = dot(e2, q) * inv
                if 1e-4 < tt < tmax: return True
        return False

# --- Noise and patterns ---

def hash2(ix, iy, seed):
    h = (ix * 374761393 + iy * 668265263 + seed * 2147483647) & 0xffffffff
    h = ((h ^ (h >> 13)) * 1274126177) & 0xffffffff
    return ((h ^ (h >> 16)) & 0xffff) / 65535.0

def vnoise(x, y, seed=0):
    ix, iy = math.floor(x), math.floor(y)
    fx, fy = x - ix, y - iy
    fx = fx * fx * (3 - 2 * fx); fy = fy * fy * (3 - 2 * fy)
    a = hash2(ix, iy, seed); b = hash2(ix + 1, iy, seed)
    c = hash2(ix, iy + 1, seed); d = hash2(ix + 1, iy + 1, seed)
    return a + (b - a) * fx + (c - a) * fy + (a - b - c + d) * fx * fy

def fbm(x, y, seed=0):
    return 0.5 * vnoise(x, y, seed) + 0.3 * vnoise(x * 2.1, y * 2.1, seed + 1) + 0.2 * vnoise(x * 4.3, y * 4.3, seed + 2)

# Steel throughout, in a few finishes from dark to polished
MATS = {
    'gunmetal': (128, 133, 140),
    'cover': (148, 152, 158),
    'barrel': (176, 180, 186),
    'clamp': (112, 116, 122),
    'dark': (72, 74, 78),
    'steel': (192, 195, 200),
    'muzzle': (206, 208, 211),
    'bore': (14, 14, 15),
    'motor': (102, 106, 112),
    'grip': (62, 63, 66),
    'ammo': (96, 104, 64),   # Painted olive, to tell it apart

    'chute': (108, 112, 118),
}

# How readily each finish rusts
RUST = {'gunmetal': 1.0, 'cover': 0.9, 'clamp': 1.1, 'dark': 0.8, 'steel': 0.5, 'motor': 1.2,
        'grip': 0.5, 'ammo': 1.2, 'chute': 1.0, 'barrel': 0.55, 'muzzle': 0.3}

def smoothstep(a, b, x):
    t = max(0.0, min(1.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)

def rust(col, mat, p, n, ao):
    # A light, patchy sheen of rust that fades in and out, heavier in creases
    # and on surfaces facing down. Noise is taken from the position, so it
    # runs across seams.
    s = RUST.get(mat, 0.0)
    if s == 0.0: return col
    x = p[0] * 9 + p[2] * 4; y = p[1] * 9 - p[2] * 6
    blot = fbm(x, y, 31)
    amt = blot + 0.3 * (1.0 - ao) + 0.08 * max(0.0, -n[1]) - 0.5
    cover = 0.32 * smoothstep(0.0, 0.3, amt) * min(1.0, s)
    if cover <= 0.0: return col
    v = fbm(x * 4.0, y * 4.0, 35)
    rc = [150 + 40 * v, 92 + 26 * v, 58 + 14 * v]
    return [col[i] * (1 - cover) + rc[i] * cover for i in range(3)]

def shade_texel(mat, cu, cv, chart, p, n, edge, ao):
    # The base color of a material at a point of its chart, before light
    r, g, b = MATS[mat]
    k = 1.0
    if mat in ('gunmetal', 'cover', 'clamp', 'motor', 'chute', 'dark', 'steel', 'ammo'):
        k *= 0.9 + 0.2 * fbm(cu * 60, cv * 60, 3)
        # A fine grain, like a bead-blasted finish
        k *= 0.94 + 0.12 * hash2(int(cu * 900), int(cv * 900), 5)
    if mat == 'barrel':
        # Brushed along the length, with soot toward the muzzle
        k *= 0.85 + 0.3 * vnoise(cu * 300, cv * 8, 7)
        k *= 1.0 - 0.3 * max(0.0, min(1.0, (p[0] - 0.40) / 0.24))
    if mat == 'muzzle':
        k *= 0.9 + 0.2 * vnoise(cu * 200, cv * 200, 9)
    if mat == 'bore':
        # Darker the deeper in
        k *= max(0.25, 1.0 - (0.64 - p[0]) * 18)
    if mat == 'grip':
        k *= 0.9 + 0.15 * hash2(int(cu * 700), int(cv * 700), 11)
    if mat == 'grip' and chart.startswith('grip:') and not chart.endswith(('py', 'ny')):
        # Diamond knurling over the middle of the grip
        if 0.03 < cv < 0.15 or chart.endswith(('px', 'nx')):
            s = (math.sin((cu + cv) * 260) * math.sin((cu - cv) * 260))
            k *= 0.8 + 0.25 * (s > 0.2)
    if mat == 'grip' and chart.startswith('fgrip:side'):
        # Rings for fingers
        k *= 0.78 + 0.3 * (math.sin(cv * 160) > -0.2)
    if mat == 'motor' and chart.endswith('side'):
        # Cooling fins: bands around the motor, clear of its ends
        if 0.012 < cv < (RX0 - MX0) - 0.012:
            f = math.sin(cv * 2 * math.pi / 0.016)
            k *= 0.72 + 0.36 * (f > -0.1) + 0.1 * (f > 0.75)
    if mat == 'motor' and chart.endswith('cap0'):
        # Vent slots in the end cap, and a center boss
        x, y = cu - MOTOR_R, cv - MOTOR_R
        rr = math.hypot(x, y); ang = math.atan2(y, x)
        if 0.034 < rr < 0.082 and math.sin(ang * 10) > 0.35: k *= 0.35
        if rr < 0.024: k *= 1.25
    if mat == 'ammo':
        # Pressed ribs around the box, and a pale stencil band on the side
        if chart.endswith(('pz', 'nz')):
            if 0.05 < cv < 0.07 and 0.03 < cu < 0.17: r, g, b = 190, 186, 160
            if abs(cv - 0.11) < 0.004: k *= 0.7
        if chart.endswith('py') and (abs(cu - 0.04) < 0.008 or abs(cu - 0.16) < 0.008):
            r, g, b = 150, 153, 158   # Steel latches
    if mat == 'chute' and chart.endswith('nz'):
        # The belt seen through the chute's window: steel cases in links
        if 0.012 < cv < 0.078 and 0.008 < cu < 0.082:
            ph = (cu * 1000 / 9.0) % 1.0
            if ph < 0.68: r, g, b, k = 170, 173, 178, 0.8 + 0.35 * math.sin(ph / 0.68 * math.pi)
            else: r, g, b, k = 40, 40, 42, 1.0
    if mat == 'gunmetal' and chart.startswith('receiver:side'):
        # Panel lines and bolts on the flat sides
        if abs(cu - 0.10) < 0.002 or abs(cu - 0.33) < 0.002: k *= 0.55
        for bx in (0.03, 0.08, 0.36, 0.40):
            for by in (0.02, 0.166):
                if chart in ('receiver:side2', 'receiver:side6') and math.hypot(cu - bx, cv - by) < 0.006:
                    k *= 1.35
        if chart in ('receiver:side2', 'receiver:side6'):
            if 0.15 < cu < 0.29 and 0.05 < cv < 0.136:
                # A recessed plate
                k *= 0.82
                if abs(cu - 0.15) < 0.003 or abs(cu - 0.29) < 0.003 or abs(cv - 0.05) < 0.003 or abs(cv - 0.136) < 0.003: k *= 0.7
    if mat == 'clamp' and 'side' in chart and chart.startswith(('clamp_', 'rotor')):
        # Bolt heads around the clamps and the rotor; the thin bands are plain
        for i in range(6):
            bx = (i + 0.5) * (2 * math.pi * 0.083) / 6
            if math.hypot(cu - bx, cv - 0.015) < 0.006: k *= 1.45
    col = rust([r * k, g * k, b * k], mat, p, n, ao)
    if edge is not None and mat not in ('bore',):
        # Worn, bright metal along hard edges, sharp at the edge itself
        wear = max(0.0, 1.0 - edge / 0.0032) ** 1.5
        if mat in ('grip', 'ammo'): wear *= 0.45
        col = [col[0] + (220 - col[0]) * 0.6 * wear, col[1] + (223 - col[1]) * 0.6 * wear, col[2] + (227 - col[2]) * 0.6 * wear]
    return col

# --- Baking the texture ---

def bake(m, dens, place, rays=40):
    P = [v['p'] for v in m.verts]
    T = [(t[0], t[1], t[2]) for t in m.tris]
    grid = Grid(P, T)

    # Hard edges per part, for the worn highlights: mesh edges where the faces
    # either side turn by more than 35 degrees, or that have only one face
    def key(p): return tuple(round(x, 5) for x in p)
    edge_faces = {}
    fnorm = []
    for i, (a, b, c, part) in enumerate(m.tris):
        n = norm(cross(sub(P[b], P[a]), sub(P[c], P[a])))
        fnorm.append(n)
        for x, y in ((a, b), (b, c), (c, a)):
            k = tuple(sorted((key(P[x]), key(P[y]))))
            edge_faces.setdefault((part, k), []).append(i)
    hard = {}
    for (part, k), fs in edge_faces.items():
        is_hard = len(fs) == 1 or any(dot(fnorm[fs[0]], fnorm[f]) < math.cos(math.radians(35)) for f in fs[1:])
        if is_hard: hard.setdefault(part, []).append(k)

    def edge_dist(part, p):
        best = None
        for a, b in hard.get(part, ()):
            ab = sub(b, a); L = dot(ab, ab)
            t = 0.0 if L == 0 else max(0.0, min(1.0, dot(sub(p, a), ab) / L))
            d = math.sqrt(dot(sub(p, add(a, mul(ab, t))), sub(p, add(a, mul(ab, t)))))
            if best is None or d < best: best = d
        return best

    # Directions over a hemisphere, cosine weighted
    rnd = random.Random(1)
    dirs = []
    k = int(math.sqrt(rays))
    for i in range(k):
        for j in range(rays // k):
            u1 = (i + rnd.random()) / k; u2 = (j + rnd.random()) / (rays // k)
            r = math.sqrt(u1); ph = 2 * math.pi * u2
            dirs.append((r * math.cos(ph), r * math.sin(ph), math.sqrt(max(0.0, 1 - u1))))

    img = [None] * (ATLAS * ATLAS)
    ao_cache = {}
    owner = {name: c['part'] for name, c in m.charts.items()}
    done = 0
    for ti, (a, b, c, part) in enumerate(m.tris):
        va, vb, vc = m.verts[a], m.verts[b], m.verts[c]
        ch = va['chart']
        # Shared charts are painted from the part that owns them
        if owner[ch] != part: continue
        U = [atlas_xy(place, ch, v['cu'], v['cv'], dens) for v in (va, vb, vc)]
        det = (U[1][0] - U[0][0]) * (U[2][1] - U[0][1]) - (U[2][0] - U[0][0]) * (U[1][1] - U[0][1])
        if abs(det) < 1e-9: continue
        x0 = max(0, int(min(u[0] for u in U)) - 1); x1 = min(ATLAS - 1, int(max(u[0] for u in U)) + 1)
        y0 = max(0, int(min(u[1] for u in U)) - 1); y1 = min(ATLAS - 1, int(max(u[1] for u in U)) + 1)
        mat = m.charts[ch]['mat']
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                px, py = x + 0.5, y + 0.5
                b1 = ((px - U[0][0]) * (U[2][1] - U[0][1]) - (U[2][0] - U[0][0]) * (py - U[0][1])) / det
                b2 = ((U[1][0] - U[0][0]) * (py - U[0][1]) - (px - U[0][0]) * (U[1][1] - U[0][1])) / det
                b0 = 1 - b1 - b2
                if min(b0, b1, b2) < -0.03: continue
                p = [b0 * va['p'][j] + b1 * vb['p'][j] + b2 * vc['p'][j] for j in range(3)]
                n = norm([b0 * va['n'][j] + b1 * vb['n'][j] + b2 * vc['n'][j] for j in range(3)])
                cu = b0 * va['cu'] + b1 * vb['cu'] + b2 * vc['cu']
                cv = b0 * va['cv'] + b1 * vb['cv'] + b2 * vc['cv']

                # Occlusion, every other texel each way, which is plenty for
                # something this soft
                akey = (ti, x >> 1, y >> 1)
                if akey not in ao_cache:
                    t1, t2 = basis(n)
                    o = add(p, mul(n, 0.0015))
                    hits = 0
                    for dx, dy, dz in dirs:
                        d = add(add(mul(t1, dx), mul(t2, dy)), mul(n, dz))
                        if grid.hit(o, d, 0.12): hits += 1
                    ao_cache[akey] = 1.0 - hits / len(dirs)
                ao = ao_cache[akey]

                col = shade_texel(mat, cu, cv, ch, p, n, edge_dist(part, p), ao)
                light = 0.22 + 0.78 * ao ** 1.3
                img[y * ATLAS + x] = [c2 * light for c2 in col]
        done += 1
        if done % 100 == 0: print('baked', done, 'triangles', file=sys.stderr)

    # Spread the charts past their edges so filtering never reaches the gaps
    for _ in range(PAD + 1):
        add_px = {}
        for y in range(ATLAS):
            for x in range(ATLAS):
                if img[y * ATLAS + x] is not None: continue
                acc = [0.0, 0.0, 0.0]; cnt = 0
                for dy in (-1, 0, 1):
                    for dx in (-1, 0, 1):
                        xx, yy = x + dx, y + dy
                        if 0 <= xx < ATLAS and 0 <= yy < ATLAS and img[yy * ATLAS + xx] is not None:
                            q = img[yy * ATLAS + xx]; acc = [acc[j] + q[j] for j in range(3)]; cnt += 1
                if cnt: add_px[y * ATLAS + x] = [c2 / cnt for c2 in acc]
        for i, c2 in add_px.items(): img[i] = c2
    return img

def png_encode(w, h, rgb_rows):
    raw = b''.join(b'\x00' + bytes(r) for r in rgb_rows)
    def chunk(t, b): return struct.pack('>I', len(b)) + t + b + struct.pack('>I', zlib.crc32(t + b) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')

# --- Writing the .glb ---

def write_glb(m, dens, place, jpeg, out_path):
    P = [v['p'] for v in m.verts]
    N = [v['n'] for v in m.verts]
    UV = []
    for v in m.verts:
        x, y = atlas_xy(place, v['chart'], v['cu'], v['cv'], dens)
        UV.append((x / ATLAS, y / ATLAS))
    idx = [i for t in m.tris for i in t[:3]]

    # Quantized like the other models, but scaled only, not centred, so that
    # the barrel axis stays on the x axis the spin joint turns about
    half = max(abs(p[j]) for p in P for j in range(3))
    qp = [[round(p[j] / half * 32767) for j in range(3)] for p in P]
    qn = [[max(-127, min(127, round(n[j] * 127))) for j in range(3)] for n in N]
    qt = [[round(min(1.0, max(0.0, x)) * 65535) for x in t] for t in UV]
    qmn = [min(p[j] for p in qp) / 32767 for j in range(3)]
    qmx = [max(p[j] for p in qp) / 32767 for j in range(3)]
    joints = [[v['joint'], 0, 0, 0] for v in m.verts]
    weights = [[1.0, 0.0, 0.0, 0.0] for v in m.verts]

    # The spin: one full turn about x over a second, a key every 15 degrees
    keys = 24
    times = [i / keys for i in range(keys + 1)]
    quats = []
    for i in range(keys + 1):
        a = 2 * math.pi * i / keys
        quats.append([math.sin(a / 2), 0.0, 0.0, math.cos(a / 2)])

    def pack(fmt, rows): return b''.join(struct.pack('<' + fmt, *r) for r in rows)
    parts = [
        (pack('3hxx', qp), 8), (pack('3bx', qn), 4), (pack('2H', qt), 4),
        (struct.pack('<%dH' % len(idx), *idx), None),
        (pack('4B', joints), None), (pack('4f', weights), None),
        (struct.pack('<%df' % len(times), *times), None), (pack('4f', quats), None),
        (jpeg, None),
    ]
    views = []; binb = b''
    for data, st in parts:
        while len(binb) % 4: binb += b'\0'
        view = {'buffer': 0, 'byteOffset': len(binb), 'byteLength': len(data)}
        if st: view['byteStride'] = st
        views.append(view)
        binb += data
    while len(binb) % 4: binb += b'\0'

    gl = {
        'asset': {'version': '2.0', 'generator': 'gen_rotary.py'},
        'extensionsUsed': ['KHR_mesh_quantization'],
        'extensionsRequired': ['KHR_mesh_quantization'],
        'scene': 0, 'scenes': [{'nodes': [0, 1, 2]}],
        'nodes': [
            {'name': 'rotary_cannon', 'mesh': 0, 'skin': 0},
            {'name': 'body'},
            {'name': 'barrels'},
        ],
        'skins': [{'joints': [1, 2]}],
        'meshes': [{'name': 'rotary_cannon', 'primitives': [{
            'attributes': {'POSITION': 0, 'NORMAL': 1, 'TEXCOORD_0': 2, 'JOINTS_0': 4, 'WEIGHTS_0': 5},
            'indices': 3, 'material': 0}]}],
        'materials': [{'name': 'rotary_cannon', 'pbrMetallicRoughness': {'baseColorTexture': {'index': 0},
                                                                       'metallicFactor': 0.0, 'roughnessFactor': 1.0}}],
        'textures': [{'source': 0, 'sampler': 0}],
        'samplers': [{'magFilter': 9728, 'minFilter': 9728}],
        'images': [{'bufferView': 8, 'mimeType': 'image/jpeg'}],
        'animations': [{'name': 'spin', 'samplers': [{'input': 6, 'output': 7, 'interpolation': 'LINEAR'}],
                        'channels': [{'sampler': 0, 'target': {'node': 2, 'path': 'rotation'}}]}],
        'accessors': [
            {'bufferView': 0, 'componentType': 5122, 'normalized': True, 'count': len(P), 'type': 'VEC3', 'min': qmn, 'max': qmx},
            {'bufferView': 1, 'componentType': 5120, 'normalized': True, 'count': len(N), 'type': 'VEC3'},
            {'bufferView': 2, 'componentType': 5123, 'normalized': True, 'count': len(UV), 'type': 'VEC2'},
            {'bufferView': 3, 'componentType': 5123, 'count': len(idx), 'type': 'SCALAR'},
            {'bufferView': 4, 'componentType': 5121, 'count': len(P), 'type': 'VEC4'},
            {'bufferView': 5, 'componentType': 5126, 'count': len(P), 'type': 'VEC4'},
            {'bufferView': 6, 'componentType': 5126, 'count': len(times), 'type': 'SCALAR', 'min': [0.0], 'max': [1.0]},
            {'bufferView': 7, 'componentType': 5126, 'count': len(quats), 'type': 'VEC4'},
        ],
        'bufferViews': views,
        'buffers': [{'byteLength': len(binb)}],
    }
    js = json.dumps(gl, separators=(',', ':')).encode()
    while len(js) % 4: js += b' '
    total = 12 + 8 + len(js) + 8 + len(binb)
    out = struct.pack('<III', 0x46546C67, 2, total) + struct.pack('<II', len(js), 0x4E4F534A) + js \
        + struct.pack('<II', len(binb), 0x004E4942) + binb
    open(out_path, 'wb').write(out)
    print('wrote %s: %d triangles, %d vertices, %d bytes (texture %d)' % (out_path, len(m.tris), len(P), len(out), len(jpeg)))

if __name__ == '__main__':
    mode = sys.argv[1]
    m = build()
    dens, place = pack(m)
    print('%d triangles, %d charts, %.0f texels per unit' % (len(m.tris), len(m.charts), dens), file=sys.stderr)
    if mode == 'texture':
        img = bake(m, dens, place)
        rows = []
        for y in range(ATLAS):
            row = bytearray()
            for x in range(ATLAS):
                c = img[y * ATLAS + x] or [0, 0, 0]
                row += bytes(max(0, min(255, int(v + 0.5))) for v in c)
            rows.append(row)
        open(sys.argv[2], 'wb').write(png_encode(ATLAS, ATLAS, rows))
    else:
        write_glb(m, dens, place, open(sys.argv[2], 'rb').read(), sys.argv[3])
