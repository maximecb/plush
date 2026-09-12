#!/usr/bin/env python3
"""Generate a walkable glass skyscraper map for examples/tremor.psh."""

from __future__ import annotations

import argparse
from pathlib import Path


SITE_HALF = 50.0
GROUND_Y = 0.2
BUILDING_X0 = -24.0
BUILDING_X1 = 24.0
BUILDING_Z0 = -22.0
BUILDING_Z1 = 30.0

LOBBY_HEIGHT = 5.0
FLOOR_HEIGHT = 4.0
SLAB_THICKNESS = 0.3
DEFAULT_UPPER_FLOORS = 26

STAIR_RUNS = ((10.0, 14.0), (15.0, 19.0))
STAIR_Z0 = -1.0
STAIR_Z1 = 7.0
STEP_RISE = 0.2
STREET_LIGHT_XS = (-36.0, -12.0, 12.0, 36.0)
STREET_LIGHT_Z = -34.12
STREET_LIGHT_HEIGHT = 6.5

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = REPO_ROOT / "examples" / "data" / "skyscraper.map"


def number(value: float) -> str:
    if abs(value) < 0.000_000_5:
        value = 0.0
    return f"{value:.3f}".rstrip("0").rstrip(".")


class MapWriter:
    def __init__(self) -> None:
        self.lines: list[str] = []

    def line(self, text: str = "") -> None:
        self.lines.append(text)

    def section(self, title: str) -> None:
        self.lines.extend(("", f"// {title}"))

    def box(
        self,
        x0: float,
        y0: float,
        z0: float,
        x1: float,
        y1: float,
        z1: float,
        options: str,
    ) -> None:
        coords = (
            f"x0={number(x0)} y0={number(y0)} z0={number(z0)} "
            f"x1={number(x1)} y1={number(y1)} z1={number(z1)}"
        )
        self.lines.append(f"box {coords} {options}")

    def light(
        self,
        x: float,
        y: float,
        z: float,
        radius: float,
        brightness: float,
    ) -> None:
        self.lines.append(
            f"light x={number(x)} y={number(y)} z={number(z)} "
            f"radius={number(radius)} brightness={number(brightness)}"
        )

    def prism(
        self,
        cx: float,
        cz: float,
        radius: float,
        y0: float,
        y1: float,
        sides: int,
        options: str,
    ) -> None:
        self.lines.append(
            f"prism cx={number(cx)} cz={number(cz)} radius={number(radius)} "
            f"y0={number(y0)} y1={number(y1)} sides={sides} rot=0 {options}"
        )

    def render(self) -> str:
        return "\n".join(self.lines) + "\n"


def add_site(out: MapWriter) -> None:
    out.section("A 100 m by 100 m site, with the tower entrance facing south")
    out.box(
        -SITE_HALF,
        -0.6,
        -SITE_HALF,
        SITE_HALF,
        -0.4,
        SITE_HALF,
        "tex=concrete_01 scale=16",
    )
    out.box(
        -SITE_HALF,
        -0.4,
        -SITE_HALF,
        SITE_HALF,
        0.0,
        -38.0,
        "tex=asphalt_02 scale=10",
    )
    out.box(
        -SITE_HALF,
        -0.4,
        -38.0,
        SITE_HALF,
        GROUND_Y,
        -34.0,
        "tex=concrete_white scale=16",
    )
    plaza_options = "tex=concrete_01 scale=16"
    out.box(-SITE_HALF, -0.4, -34.0, SITE_HALF, GROUND_Y, BUILDING_Z0, plaza_options)
    out.box(-SITE_HALF, -0.4, BUILDING_Z1, SITE_HALF, GROUND_Y, SITE_HALF, plaza_options)
    out.box(
        -SITE_HALF,
        -0.4,
        BUILDING_Z0,
        BUILDING_X0,
        GROUND_Y,
        BUILDING_Z1,
        plaza_options,
    )
    out.box(
        BUILDING_X1,
        -0.4,
        BUILDING_Z0,
        SITE_HALF,
        GROUND_Y,
        BUILDING_Z1,
        plaza_options,
    )

    for x0 in range(-47, 48, 10):
        out.box(
            float(x0),
            0.0,
            -44.08,
            float(x0 + 5),
            0.02,
            -43.92,
            "tex=concrete_white scale=40",
        )

    # Planters give the otherwise open plaza a little depth.
    for x0, x1 in ((-45.0, -31.0), (31.0, 45.0)):
        out.box(
            x0,
            GROUND_Y,
            -29.0,
            x1,
            GROUND_Y + 0.6,
            -26.0,
            "tex=stone_large top=floor_dirt scale=20",
        )

    out.line("// Street lights along the building-side pavement")
    pole = "tex=metal_dark scale=50 light=7"
    lamp = "tex=metal_dark bottom=light_square light=15 fit"
    for x in STREET_LIGHT_XS:
        out.box(
            x - 0.12,
            GROUND_Y,
            STREET_LIGHT_Z - 0.12,
            x + 0.12,
            GROUND_Y + STREET_LIGHT_HEIGHT,
            STREET_LIGHT_Z + 0.12,
            pole,
        )
        out.box(
            x - 0.12,
            GROUND_Y + STREET_LIGHT_HEIGHT - 0.2,
            -37.25,
            x + 0.12,
            GROUND_Y + STREET_LIGHT_HEIGHT,
            STREET_LIGHT_Z,
            pole,
        )
        out.box(
            x - 0.45,
            GROUND_Y + STREET_LIGHT_HEIGHT - 0.4,
            -37.55,
            x + 0.45,
            GROUND_Y + STREET_LIGHT_HEIGHT - 0.2,
            -36.75,
            lamp,
        )


def add_lighting(out: MapWriter, upper_floors: int) -> None:
    out.section("Baked lighting for the site, lobby, and each upper floor")
    out.light(-65.0, 145.0, -85.0, 280.0, 1.0)
    out.light(-7.0, GROUND_Y + 3.2, 2.0, 48.0, 1.1)
    for floor in range(upper_floors):
        floor_y = GROUND_Y + LOBBY_HEIGHT + floor * FLOOR_HEIGHT
        out.light(-6.0, floor_y + 2.8, 4.0, 46.0, 0.95)
    for x in STREET_LIGHT_XS:
        out.light(
            x,
            GROUND_Y + STREET_LIGHT_HEIGHT - 0.5,
            -37.15,
            24.0,
            0.375,
        )


def add_floor_plate(
    out: MapWriter,
    y: float,
    stair_openings: tuple[tuple[float, float], ...] = (),
    top_texture: str = "carpet_blue",
) -> None:
    options = (
        f"tex=concrete_white top={top_texture} bottom=ceiling_tile "
        "scale=100"
    )
    y0 = y - SLAB_THICKNESS
    if not stair_openings:
        out.box(BUILDING_X0, y0, BUILDING_Z0, BUILDING_X1, y, BUILDING_Z1, options)
        return

    out.box(BUILDING_X0, y0, BUILDING_Z0, BUILDING_X1, y, STAIR_Z0, options)
    out.box(BUILDING_X0, y0, STAIR_Z1, BUILDING_X1, y, BUILDING_Z1, options)
    cursor = BUILDING_X0
    for opening_x0, opening_x1 in stair_openings:
        out.box(cursor, y0, STAIR_Z0, opening_x0, y, STAIR_Z1, options)
        cursor = opening_x1
    out.box(cursor, y0, STAIR_Z0, BUILDING_X1, y, STAIR_Z1, options)


def add_lobby(out: MapWriter) -> None:
    out.section("Ground-floor lobby")
    add_floor_plate(out, GROUND_Y)

    wall = "tex=concrete_white scale=80"
    glass = "tex=glass_blue light=9 alpha=0.23 fit"
    lobby_top = GROUND_Y + LOBBY_HEIGHT
    entrance_top = GROUND_Y + 3.6
    out.box(
        BUILDING_X0,
        GROUND_Y,
        BUILDING_Z1 - 0.35,
        BUILDING_X1,
        lobby_top,
        BUILDING_Z1,
        wall,
    )
    out.box(
        BUILDING_X0,
        GROUND_Y,
        BUILDING_Z0,
        BUILDING_X0 + 0.35,
        lobby_top,
        BUILDING_Z1,
        wall,
    )
    out.box(
        BUILDING_X1 - 0.35,
        GROUND_Y,
        BUILDING_Z0,
        BUILDING_X1,
        lobby_top,
        BUILDING_Z1,
        wall,
    )

    out.box(
        BUILDING_X0,
        GROUND_Y,
        BUILDING_Z0,
        -2.2,
        lobby_top - SLAB_THICKNESS,
        BUILDING_Z0 + 0.08,
        glass,
    )
    out.box(
        2.2,
        GROUND_Y,
        BUILDING_Z0,
        BUILDING_X1,
        lobby_top - SLAB_THICKNESS,
        BUILDING_Z0 + 0.08,
        glass,
    )
    out.box(
        -2.2,
        entrance_top,
        BUILDING_Z0,
        2.2,
        lobby_top - SLAB_THICKNESS,
        BUILDING_Z0 + 0.08,
        glass,
    )

    for x in (-18.0, -12.0, -6.0, 6.0, 12.0, 18.0):
        out.box(
            x - 0.09,
            GROUND_Y,
            BUILDING_Z0 - 0.02,
            x + 0.09,
            lobby_top,
            BUILDING_Z0 + 0.18,
            "tex=metal_plate scale=50",
        )

    out.line("// Reception desk")
    reception = "tex=wood top=marble_white scale=90"
    out.box(-4.0, GROUND_Y, -11.0, 4.0, GROUND_Y + 1.05, -10.0, reception)
    out.box(3.0, GROUND_Y, -10.0, 4.0, GROUND_Y + 1.05, -7.0, reception)


def add_perimeter_band(out: MapWriter, y: float) -> None:
    options = "tex=concrete_white scale=70"
    thickness = 0.35
    y0 = y - SLAB_THICKNESS
    y1 = y + 0.65
    out.box(
        BUILDING_X0,
        y0,
        BUILDING_Z0,
        BUILDING_X1,
        y1,
        BUILDING_Z0 + thickness,
        options,
    )
    out.box(
        BUILDING_X0,
        y0,
        BUILDING_Z1 - thickness,
        BUILDING_X1,
        y1,
        BUILDING_Z1,
        options,
    )
    out.box(
        BUILDING_X0,
        y0,
        BUILDING_Z0 + thickness,
        BUILDING_X0 + thickness,
        y1,
        BUILDING_Z1 - thickness,
        options,
    )
    out.box(
        BUILDING_X1 - thickness,
        y0,
        BUILDING_Z0 + thickness,
        BUILDING_X1,
        y1,
        BUILDING_Z1 - thickness,
        options,
    )


def add_glass_storey(out: MapWriter, y0: float, y1: float) -> None:
    options = "tex=glass_grey light=9 alpha=0.21 fit"
    pane_bottom = y0 + 0.65
    pane_top = y1 - 0.2
    inset = 0.12
    out.box(
        BUILDING_X0 + inset,
        pane_bottom,
        BUILDING_Z0,
        BUILDING_X1 - inset,
        pane_top,
        BUILDING_Z0 + 0.08,
        options,
    )
    out.box(
        BUILDING_X0 + inset,
        pane_bottom,
        BUILDING_Z1 - 0.08,
        BUILDING_X1 - inset,
        pane_top,
        BUILDING_Z1,
        options,
    )
    out.box(
        BUILDING_X0,
        pane_bottom,
        BUILDING_Z0 + inset,
        BUILDING_X0 + 0.08,
        pane_top,
        BUILDING_Z1 - inset,
        options,
    )
    out.box(
        BUILDING_X1 - 0.08,
        pane_bottom,
        BUILDING_Z0 + inset,
        BUILDING_X1,
        pane_top,
        BUILDING_Z1 - inset,
        options,
    )


def add_desk(out: MapWriter, floor_y: float, center_x: float, z0: float) -> None:
    desk_y = floor_y + 0.86
    out.box(
        center_x - 1.8,
        floor_y,
        z0,
        center_x + 1.8,
        desk_y,
        z0 + 1.2,
        "tex=metal_01 top=wood scale=80 light=8",
    )
    out.box(
        center_x - 0.4,
        desk_y,
        z0 + 0.66,
        center_x + 0.4,
        desk_y + 0.62,
        z0 + 0.82,
        "tex=asphalt_02 nz=monitor light=13 fit",
    )


def add_office_front(
    out: MapWriter,
    floor_y: float,
    ceiling_y: float,
    x0: float,
    x1: float,
) -> None:
    partition_z0 = 15.0
    partition_z1 = 15.24
    partition_top = ceiling_y - SLAB_THICKNESS
    door_x1 = x1 - 0.7
    door_x0 = door_x1 - 1.4
    window_x0 = x0 + 0.35
    window_x1 = door_x0 - 0.25
    wall = "tex=beigewall scale=80 light=8"
    frame = "tex=metal_plate scale=50 light=8"
    glass = "tex=glass_grey light=9 alpha=0.20 fit"

    out.box(x0, floor_y, partition_z0, window_x0, partition_top, partition_z1, wall)
    out.box(window_x0, floor_y, partition_z0, window_x1, floor_y + 0.75, partition_z1, wall)
    out.box(
        window_x0,
        floor_y + 0.75,
        partition_z0 + 0.08,
        window_x1,
        partition_top - 0.35,
        partition_z0 + 0.14,
        glass,
    )
    out.box(
        window_x0,
        partition_top - 0.35,
        partition_z0,
        window_x1,
        partition_top,
        partition_z1,
        wall,
    )
    out.box(window_x1, floor_y, partition_z0, door_x0, partition_top, partition_z1, frame)
    out.box(door_x1, floor_y, partition_z0, x1, partition_top, partition_z1, wall)
    out.box(
        door_x0,
        floor_y + 2.5,
        partition_z0,
        door_x1,
        partition_top,
        partition_z1,
        wall,
    )


def add_upper_floor_layout(out: MapWriter, floor_y: float, ceiling_y: float) -> None:
    office_edges = (-23.6, -12.0, -1.0, 9.0)
    partition_top = ceiling_y - SLAB_THICKNESS
    wall = "tex=beigewall scale=80 light=8"

    out.line("// Three enclosed offices along the rear curtain wall")
    for office in range(len(office_edges) - 1):
        x0 = office_edges[office]
        x1 = office_edges[office + 1]
        add_office_front(out, floor_y, ceiling_y, x0, x1)
        add_desk(out, floor_y, (x0 + x1) * 0.5, 24.4)

    for x in office_edges[1:]:
        out.box(
            x - 0.12,
            floor_y,
            15.24,
            x + 0.12,
            partition_top,
            BUILDING_Z1 - 0.12,
            wall,
        )

    out.line("// Open-plan work area, with the stair circulation left clear")
    for z0 in (-14.0, -10.0, -6.0, -2.0, 2.0):
        for center_x in (-17.0, -11.0, -5.0, 1.0):
            add_desk(out, floor_y, center_x, z0)


def add_stairs(
    out: MapWriter,
    y0: float,
    y1: float,
    flight: int,
) -> None:
    stair_x0, stair_x1 = STAIR_RUNS[flight % len(STAIR_RUNS)]
    ascending_positive_z = flight % 2 == 0
    step_count = round((y1 - y0) / STEP_RISE)
    step_depth = (STAIR_Z1 - STAIR_Z0) / step_count
    options = "tex=concrete_white top=floor_tread scale=80 light=8"
    for step in range(step_count):
        if ascending_positive_z:
            z0 = STAIR_Z0 + step * step_depth
            z1 = STAIR_Z0 + (step + 1) * step_depth
        else:
            z0 = STAIR_Z1 - (step + 1) * step_depth
            z1 = STAIR_Z1 - step * step_depth
        out.box(
            stair_x0,
            y0,
            z0,
            stair_x1,
            y0 + (step + 1) * STEP_RISE,
            z1,
            options,
        )


def add_mullions(out: MapWriter, upper_base: float, roof_y: float) -> None:
    out.section("Continuous mullions tie the separate window storeys together")
    options = "tex=metal_plate scale=50 light=8"
    for x in (-18.0, -12.0, -6.0, 0.0, 6.0, 12.0, 18.0):
        out.box(
            x - 0.08,
            upper_base,
            BUILDING_Z0 - 0.1,
            x + 0.08,
            roof_y,
            BUILDING_Z0 + 0.1,
            options,
        )
        out.box(
            x - 0.08,
            upper_base,
            BUILDING_Z1 - 0.1,
            x + 0.08,
            roof_y,
            BUILDING_Z1 + 0.1,
            options,
        )
    for z in (-16.0, -10.0, -4.0, 2.0, 8.0, 14.0, 20.0, 26.0):
        out.box(
            BUILDING_X0 - 0.1,
            upper_base,
            z - 0.08,
            BUILDING_X0 + 0.1,
            roof_y,
            z + 0.08,
            options,
        )
        out.box(
            BUILDING_X1 - 0.1,
            upper_base,
            z - 0.08,
            BUILDING_X1 + 0.1,
            roof_y,
            z + 0.08,
            options,
        )


def add_roof(out: MapWriter, roof_y: float, final_flight: int) -> None:
    out.section("Accessible roof reached by the final flight")
    add_floor_plate(
        out,
        roof_y,
        (STAIR_RUNS[final_flight % len(STAIR_RUNS)],),
        top_texture="asphalt_02",
    )
    options = "tex=concrete_white top=metal_01 scale=60"
    parapet_height = 1.2
    out.box(
        BUILDING_X0,
        roof_y,
        BUILDING_Z0,
        BUILDING_X1,
        roof_y + parapet_height,
        BUILDING_Z0 + 0.3,
        options,
    )
    out.box(
        BUILDING_X0,
        roof_y,
        BUILDING_Z1 - 0.3,
        BUILDING_X1,
        roof_y + parapet_height,
        BUILDING_Z1,
        options,
    )
    out.box(
        BUILDING_X0,
        roof_y,
        BUILDING_Z0 + 0.3,
        BUILDING_X0 + 0.3,
        roof_y + parapet_height,
        BUILDING_Z1 - 0.3,
        options,
    )
    out.box(
        BUILDING_X1 - 0.3,
        roof_y,
        BUILDING_Z0 + 0.3,
        BUILDING_X1,
        roof_y + parapet_height,
        BUILDING_Z1 - 0.3,
        options,
    )

    out.line("// Rooftop mechanical equipment")
    equipment = "tex=metal_bolted scale=60 light=8"
    out.box(-19.0, roof_y, 16.0, -9.0, roof_y + 4.2, 24.0, equipment)
    out.box(1.0, roof_y, 16.0, 11.0, roof_y + 4.2, 24.0, equipment)

    out.line("// Communications antenna")
    mast = "tex=metal_dark scale=50 light=8"
    out.box(-1.8, roof_y, 4.2, 1.8, roof_y + 0.8, 7.8, "tex=metal_bolted scale=50 light=8")
    out.prism(0.0, 6.0, 0.72, roof_y + 0.8, roof_y + 36.0, 16, mast)


def generate(upper_floors: int) -> str:
    if upper_floors < 1:
        raise ValueError("upper_floors must be positive")

    out = MapWriter()
    building_height = LOBBY_HEIGHT + upper_floors * FLOOR_HEIGHT
    roof_y = GROUND_Y + building_height
    out.line("// Generated by scripts/gen_tremor_skyscraper.py.")
    out.line(
        f"// One lobby and {upper_floors} upper floors; "
        f"building height {number(building_height)} m."
    )
    out.line("// The site occupies x -50..50 and z -50..50, exactly 100 m by 100 m.")
    out.line("")
    out.line(f"spawn x=0 y={number(GROUND_Y + 1.6)} z=-30 yaw=0")

    add_lighting(out, upper_floors)
    add_site(out)
    add_lobby(out)

    out.section("Open upper floors, glass curtain wall, and right-side stairs")
    lower_y = GROUND_Y
    upper_y = GROUND_Y + LOBBY_HEIGHT
    add_stairs(out, lower_y, upper_y, flight=0)
    for floor in range(upper_floors):
        floor_y = GROUND_Y + LOBBY_HEIGHT + floor * FLOOR_HEIGHT
        ceiling_y = floor_y + FLOOR_HEIGHT
        out.line("")
        out.line(f"// Upper floor {floor + 1}")
        add_floor_plate(out, floor_y, (STAIR_RUNS[floor % len(STAIR_RUNS)],))
        add_perimeter_band(out, floor_y)
        add_glass_storey(out, floor_y, ceiling_y)
        add_stairs(out, floor_y, ceiling_y, flight=floor + 1)
        add_upper_floor_layout(out, floor_y, ceiling_y)

    add_mullions(out, GROUND_Y + LOBBY_HEIGHT, roof_y)
    add_roof(out, roof_y, final_flight=upper_floors)
    return out.render()


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be at least 1")
    return parsed


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", nargs="?", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument(
        "--floors",
        type=positive_int,
        default=DEFAULT_UPPER_FLOORS,
        help="number of upper floors above the lobby (default: %(default)s)",
    )
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(generate(args.floors), encoding="utf-8")
    print(f"Wrote {args.output} with {args.floors} upper floors")


if __name__ == "__main__":
    main()
