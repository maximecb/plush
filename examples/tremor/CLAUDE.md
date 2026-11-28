# Tremor level design guidelines

Maps are in `maps/`, in the Valve 220 format that TrenchBroom writes. The
entities and texture conventions are described in `trenchbroom/README.md` and
`trenchbroom/Tremor.fgd`.

## Units and axes

- 40 units to the meter. In the file, +Z is up. The game's axes are
  x = qx/40, y = qz/40, z = qy/40, and yaw 0 faces the file's +Y.
- Size rooms in multiples of 1.6 m (64 units).
- The player is 1.7 m tall (68 units) with a 0.45 m radius, and their eye is
  at 1.6 m. A step can rise at most 0.7 m. Stairs use 0.4 m risers (16 units).

## Textures

- The standard density is 40 texels per meter: texture scale 1, one texel per
  unit. Most textures are 256x256, so one tile covers 6.4 m.
- Use scale 1 for walls, floors, ceilings, trim and props.
- Rough natural surfaces (`rock_craggy`, `dirt`) use 1.25, which is
  32 texels per meter.
- Don't use scale 2 on floors: at 20 texels per meter they look blocky up
  close.
- Pay attention to texture alignment. Some textures should line up with
  the edges of brushes.
  - Panel textures such as `floor_plate` should have their seams on the
    brush's edges.
  - Fixture textures such as `light_*` should show whole on their face. That
    can mean sizing the brush to the tile, or scaling the texture to fit:
    `light_square` is 128x128, so it fits a 64-unit face at scale 0.5.
  - The offset sets the alignment: u = p.U / scale + offset, in texels.
- Don't use flowing textures on walls. A flowing face ripples by moving its
  corners up and down, which tears a face that isn't horizontal. A glowing
  wall surface can use a `func_brush` with `light` 15 instead.

## Brushes

- Every brush must be convex. A face's three points satisfy
  (p1 - p2) x (p3 - p2) = outward normal.
- Faces textured `skip` are not drawn. Use it for faces that are always
  hidden.
- Seal every room with solid walls, floors and ceilings 64 units thick.
- Build rough rock, like the walls and roof of a cave, as a skin of facets
  over a plain shell that seals the space: each facet is the hull of four
  jittered corners on the rock face and the four behind them on the shell.
  Faces buried in other brushes are cut away, so the overlap costs nothing.
- Build curved shapes, like vaults and arches, from facets. Set the texture
  offsets so the pattern runs on from one facet to the next without a jump.

## Light and hazards

- Nothing is lit by hand: every surface takes its light from the `light`
  entities in the bake.
- Underground levels set no `moonlight`.
- A `flame` gives no light of its own. Place a `light` near it, warm orange
  such as `255 150 70` for torches.
- Keep the light level even between areas that join. In level2, that is wall
  torches at brightness 0.35 and radius 400, every 6.4 m.
- Wall lights don't reach the middle of a room much wider than 10 m. Light it
  with something on the floor, such as braziers.
- Flames burn at 20/s and lava at 40/s, so keep wall torches out of the
  player's reach, 3.5 m up.
- A door is not in the light bake. Make the space it rises into solid, and
  sink its sides a little into its doorway's walls, so that no gap shows.
  Doors that slide up use `door_iron`.
- Give bridges over a drop a parapet 0.8 m high, above the 0.7 m a monster can
  step up, so that monsters chasing the player don't walk off.

## Map file organization

- Start with a header comment naming the level.
- Give each area a section comment with its extent in meters, in the game's
  axes.
- Group the brushes of each area in a `func_group`, with a `_tb_name`.

## Checking a level

- Load the map and time it with
  `plush examples/tremor/main.psh path/to/map.map --bench`.
- To look at a map, copy `main.psh` to a temporary file and replace `TOUR`
  with viewpoints of the new area. Each entry is x, y, z, yaw, pitch.
- Then run that copy with `--bench --frames N --shots dir`, and read the
  PNGs. Delete the copy afterwards.
- Benchmark with interleaved runs, and take the median.
