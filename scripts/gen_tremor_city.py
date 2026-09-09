#!/usr/bin/env python3
"""Generate a fixed, large city map for examples/tremor.psh."""

from __future__ import annotations

import argparse
import math
from dataclasses import dataclass
from pathlib import Path


LAND_W = 1000.0
LAND_D = 1000.0
SPACING = 100.0
OFFSET = 18.0
AVENUE_EVERY = 2
LANE_W = 3.5
SIDEWALK_W = 2.5
DROP_PERCENT = 20

H_MIN = 40.0
H_ALPHA = 1.14
H_MAX = 620.0

DEFAULT_SEED = 0xC17
DEFAULT_OUTPUT = Path(__file__).with_name("data") / "city_torture_test.map"
BUILDING_TEXTURES = ("skyscraper_01", "skyscraper_03", "skyscraper_04")


@dataclass(frozen=True)
class Rect:
    x0: float
    z0: float
    x1: float
    z1: float


@dataclass(frozen=True)
class Street:
    bounds: Rect
    lanes: int
    axis: str


class Lcg:
    """The random generator used by city_layout.psh."""

    def __init__(self, seed: int) -> None:
        self.seed = seed & 0x7FFF_FFFF

    def next(self) -> int:
        self.seed = (self.seed * 1073716837 + 12345) & 0x7FFF_FFFF
        return self.seed

    def integer(self, minimum: int, maximum: int) -> int:
        return minimum + ((self.next() >> 8) % (maximum - minimum))

    def uniform(self, minimum: float, maximum: float) -> float:
        t = self.next() / 2147483648.0
        return minimum + t * (maximum - minimum)

    def pareto(self, minimum: float, alpha: float, maximum: float) -> float:
        u = self.uniform(0.0001, 1.0)
        return min(minimum * u ** (-1.0 / alpha), maximum)


class CityLayout:
    """A Python rendering of the graph geometry in city_layout.psh."""

    def __init__(self, rng: Lcg) -> None:
        self.rng = rng
        self.x_lanes = self._lane_counts(self._num_roads(LAND_W))
        self.z_lanes = self._lane_counts(self._num_roads(LAND_D))
        self.xs = self._centerlines(LAND_W, self.x_lanes)
        self.zs = self._centerlines(LAND_D, self.z_lanes)
        self.hx = [self._half_width(lanes) for lanes in self.x_lanes]
        self.hz = [self._half_width(lanes) for lanes in self.z_lanes]
        self._init_grid()
        self._drop_streets()
        self.lots = self._make_lots()
        self.streets = self._make_streets()

    @staticmethod
    def _road_width(lanes: int) -> float:
        return lanes * LANE_W + 2.0 * SIDEWALK_W

    @classmethod
    def _half_width(cls, lanes: int) -> float:
        return cls._road_width(lanes) * 0.5

    @staticmethod
    def _num_roads(span: float) -> int:
        return math.floor(span / SPACING + 0.5) + 1

    @staticmethod
    def _lane_counts(count: int) -> list[int]:
        return [4 if i % AVENUE_EVERY == 0 else 2 for i in range(count)]

    def _centerlines(self, span: float, lanes: list[int]) -> list[float]:
        last = len(lanes) - 1
        centers = []
        for i, lane_count in enumerate(lanes):
            if i == 0:
                centers.append(self._half_width(lane_count))
            elif i == last:
                centers.append(span - self._half_width(lane_count))
            else:
                ideal = span * i / last
                centers.append(ideal + self.rng.uniform(-OFFSET, OFFSET))
        return centers

    def _init_grid(self) -> None:
        nx = len(self.xs)
        nz = len(self.zs)
        self.ew_open = [[i < nx - 1 for j in range(nz)] for i in range(nx)]
        self.ns_open = [[j < nz - 1 for j in range(nz)] for i in range(nx)]
        self.legs = []
        for i in range(nx):
            column = []
            for j in range(nz):
                column.append((i > 0) + (i < nx - 1) + (j > 0) + (j < nz - 1))
            self.legs.append(column)
        self.lot_w = [[1 for _ in range(nz - 1)] for _ in range(nx - 1)]
        self.lot_d = [[1 for _ in range(nz - 1)] for _ in range(nx - 1)]
        self.absorbed = [[False for _ in range(nz - 1)] for _ in range(nx - 1)]

    def _free_cell(self, i: int, j: int) -> bool:
        return (
            0 <= i < len(self.absorbed)
            and 0 <= j < len(self.absorbed[0])
            and not self.absorbed[i][j]
            and self.lot_w[i][j] == 1
            and self.lot_d[i][j] == 1
        )

    def _drop_streets(self) -> None:
        nx = len(self.xs)
        nz = len(self.zs)
        for i in range(nx - 1):
            for j in range(nz):
                if self.rng.integer(0, 100) < DROP_PERCENT:
                    self._drop_ew(i, j)
        for i in range(nx):
            for j in range(nz - 1):
                if self.rng.integer(0, 100) < DROP_PERCENT:
                    self._drop_ns(i, j)

    def _drop_ew(self, i: int, j: int) -> None:
        if self.z_lanes[j] == 4:
            return
        if self.legs[i][j] != 4 or self.legs[i + 1][j] != 4:
            return
        if not self._free_cell(i, j - 1) or not self._free_cell(i, j):
            return
        self.ew_open[i][j] = False
        self.legs[i][j] = 3
        self.legs[i + 1][j] = 3
        self.lot_d[i][j - 1] = 2
        self.absorbed[i][j] = True

    def _drop_ns(self, i: int, j: int) -> None:
        if self.x_lanes[i] == 4:
            return
        if self.legs[i][j] != 4 or self.legs[i][j + 1] != 4:
            return
        if not self._free_cell(i - 1, j) or not self._free_cell(i, j):
            return
        self.ns_open[i][j] = False
        self.legs[i][j] = 3
        self.legs[i][j + 1] = 3
        self.lot_w[i - 1][j] = 2
        self.absorbed[i][j] = True

    def _make_lots(self) -> list[Rect]:
        lots = []
        for i in range(len(self.xs) - 1):
            for j in range(len(self.zs) - 1):
                if self.absorbed[i][j]:
                    continue
                w = self.lot_w[i][j]
                d = self.lot_d[i][j]
                lots.append(
                    Rect(
                        self.xs[i] + self.hx[i],
                        self.zs[j] + self.hz[j],
                        self.xs[i + w] - self.hx[i + w],
                        self.zs[j + d] - self.hz[j + d],
                    )
                )
        return lots

    def _make_streets(self) -> list[Street]:
        streets = []
        nx = len(self.xs)
        nz = len(self.zs)
        for i in range(nx):
            for j in range(nz):
                if i < nx - 1 and self.ew_open[i][j]:
                    streets.append(
                        Street(
                            Rect(
                                self.xs[i] + self.hx[i],
                                self.zs[j] - self.hz[j],
                                self.xs[i + 1] - self.hx[i + 1],
                                self.zs[j] + self.hz[j],
                            ),
                            self.z_lanes[j],
                            "x",
                        )
                    )
                if j < nz - 1 and self.ns_open[i][j]:
                    streets.append(
                        Street(
                            Rect(
                                self.xs[i] - self.hx[i],
                                self.zs[j] + self.hz[j],
                                self.xs[i] + self.hx[i],
                                self.zs[j + 1] - self.hz[j + 1],
                            ),
                            self.x_lanes[i],
                            "z",
                        )
                    )
        return streets


def _number(value: float) -> str:
    if abs(value) < 0.000_000_5:
        value = 0.0
    return f"{value:.3f}".rstrip("0").rstrip(".")


def _shift(rect: Rect) -> Rect:
    return Rect(
        rect.x0 - LAND_W * 0.5,
        rect.z0 - LAND_D * 0.5,
        rect.x1 - LAND_W * 0.5,
        rect.z1 - LAND_D * 0.5,
    )


def _box(rect: Rect, y0: float, y1: float, options: str) -> str:
    return (
        f"box x0={_number(rect.x0)} y0={_number(y0)} z0={_number(rect.z0)} "
        f"x1={_number(rect.x1)} y1={_number(y1)} z1={_number(rect.z1)} {options}"
    )


def _street_boxes(street: Street) -> list[str]:
    bounds = _shift(street.bounds)
    road_half = street.lanes * LANE_W * 0.5
    if street.axis == "x":
        center = (bounds.z0 + bounds.z1) * 0.5
        road = Rect(bounds.x0, center - road_half, bounds.x1, center + road_half)
        walks = [
            Rect(bounds.x0, bounds.z0, bounds.x1, road.z0),
            Rect(bounds.x0, road.z1, bounds.x1, bounds.z1),
        ]
    else:
        center = (bounds.x0 + bounds.x1) * 0.5
        road = Rect(center - road_half, bounds.z0, center + road_half, bounds.z1)
        walks = [
            Rect(bounds.x0, bounds.z0, road.x0, bounds.z1),
            Rect(road.x1, bounds.z0, bounds.x1, bounds.z1),
        ]
    return [
        _box(road, -0.4, 0.0, "tex=asphalt_02 scale=10"),
        *(_box(walk, -0.4, 0.2, "tex=concrete_01 scale=16") for walk in walks),
    ]


def _building(lot: Rect, rng: Lcg) -> str:
    width = lot.x1 - lot.x0
    depth = lot.z1 - lot.z0
    building_w = max(18.0, width * rng.uniform(0.58, 0.86))
    building_d = max(18.0, depth * rng.uniform(0.58, 0.86))
    x0 = rng.uniform(lot.x0 + 3.0, lot.x1 - 3.0 - building_w)
    z0 = rng.uniform(lot.z0 + 3.0, lot.z1 - 3.0 - building_d)
    bounds = _shift(Rect(x0, z0, x0 + building_w, z0 + building_d))
    height = rng.pareto(H_MIN, H_ALPHA, H_MAX)
    texture = BUILDING_TEXTURES[rng.integer(0, len(BUILDING_TEXTURES))]
    scale = rng.integer(8, 11)
    light = rng.integer(8, 14)
    return _box(
        bounds,
        0.2,
        0.2 + height,
        f"tex={texture} top=asphalt_02 scale={scale} light={light}",
    )


def _intersection_boxes(city: CityLayout, i: int, j: int) -> list[str]:
    x = city.xs[i] - LAND_W * 0.5
    z = city.zs[j] - LAND_D * 0.5
    bounds = Rect(x - city.hx[i], z - city.hz[j], x + city.hx[i], z + city.hz[j])
    road_x0 = x - city.x_lanes[i] * LANE_W * 0.5
    road_x1 = x + city.x_lanes[i] * LANE_W * 0.5
    road_z0 = z - city.z_lanes[j] * LANE_W * 0.5
    road_z1 = z + city.z_lanes[j] * LANE_W * 0.5

    asphalt = [
        Rect(bounds.x0, road_z0, bounds.x1, road_z1),
        Rect(road_x0, bounds.z0, road_x1, road_z0),
        Rect(road_x0, road_z1, road_x1, bounds.z1),
    ]
    sidewalks = [
        Rect(bounds.x0, bounds.z0, road_x0, road_z0),
        Rect(road_x1, bounds.z0, bounds.x1, road_z0),
        Rect(bounds.x0, road_z1, road_x0, bounds.z1),
        Rect(road_x1, road_z1, bounds.x1, bounds.z1),
    ]
    return [
        *(_box(rect, -0.4, 0.0, "tex=asphalt_02 scale=10") for rect in asphalt),
        *(_box(rect, -0.4, 0.2, "tex=concrete_01 scale=16") for rect in sidewalks),
    ]


def generate(seed: int) -> str:
    rng = Lcg(seed)
    city = CityLayout(rng)
    lines = [
        "// Generated by examples/gen_tremor_city.py.",
        "// The layout follows examples/city_layout.psh with a fixed seed.",
        f"// Seed: {seed}; lots: {len(city.lots)}; streets: {len(city.streets)}.",
        "",
    ]

    spawn_x = city.xs[len(city.xs) // 2] - LAND_W * 0.5
    spawn_z = city.zs[len(city.zs) // 2] - LAND_D * 0.5
    lines.append(f"spawn x={_number(spawn_x)} y=1.6 z={_number(spawn_z)} yaw=0")
    lines.append("light x=-180 y=900 z=-220 radius=1800 brightness=1")
    lines.extend(("", "// Roads and sidewalks"))

    for street in city.streets:
        lines.extend(_street_boxes(street))

    for i, x in enumerate(city.xs):
        for j, z in enumerate(city.zs):
            lines.extend(_intersection_boxes(city, i, j))

    lines.extend(("", "// Lots and pre-lit buildings"))
    for lot in city.lots:
        lines.append(_box(_shift(lot), -0.4, 0.2, "tex=concrete_01 scale=16"))
        lines.append(_building(lot, rng))

    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", nargs="?", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--seed", type=lambda value: int(value, 0), default=DEFAULT_SEED)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(generate(args.seed), encoding="utf-8")
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
