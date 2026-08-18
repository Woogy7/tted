from dataclasses import dataclass


@dataclass(frozen=True)
class Point:
    x: float
    y: float

    def distance_squared(self) -> float:
        return self.x ** 2 + self.y ** 2


points = [Point(3, 4), Point(5, 12)]
for point in points:
    print(f"{point}: {point.distance_squared()=}")
