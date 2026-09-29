# SPDX-License-Identifier: Apache-2.0
"""Generates icon.png and icon.ico for the desktop app (no third-party packages).

Run from this directory: python make_icons.py
"""

import struct
import zlib


def render(size):
    """RGBA rows for a rounded square with a white "L" and a dot (one endpoint)."""
    ss = 4  # supersampling
    n = size * ss
    radius = 0.22 * n
    top = (0x1F, 0x9D, 0x8B)  # teal
    bottom = (0x3B, 0x4B, 0xC8)  # indigo
    rows = []
    for y in range(size):
        row = []
        for x in range(size):
            acc = [0.0, 0.0, 0.0, 0.0]
            for sy in range(ss):
                for sx in range(ss):
                    px = x * ss + sx + 0.5
                    py = y * ss + sy + 0.5
                    # Rounded square coverage.
                    cx = min(max(px, radius), n - radius)
                    cy = min(max(py, radius), n - radius)
                    if (px - cx) ** 2 + (py - cy) ** 2 > radius**2:
                        continue
                    t = py / n
                    color = [top[i] + (bottom[i] - top[i]) * t for i in range(3)]
                    u, v = px / n, py / n
                    # "L": vertical bar and foot.
                    in_l = (0.28 <= u <= 0.42 and 0.22 <= v <= 0.78) or (
                        0.28 <= u <= 0.72 and 0.64 <= v <= 0.78
                    )
                    in_dot = (u - 0.70) ** 2 + (v - 0.32) ** 2 <= 0.085**2
                    if in_l or in_dot:
                        color = [255, 255, 255]
                    acc[0] += color[0]
                    acc[1] += color[1]
                    acc[2] += color[2]
                    acc[3] += 255
            k = ss * ss
            alpha = acc[3] / k
            if alpha == 0:
                row.append((0, 0, 0, 0))
            else:
                w = acc[3] / 255
                row.append(
                    (
                        round(acc[0] / w),
                        round(acc[1] / w),
                        round(acc[2] / w),
                        round(alpha),
                    )
                )
        rows.append(row)
    return rows


def png(rows):
    size = len(rows)
    raw = b"".join(b"\x00" + bytes(c for px in row for c in px) for row in rows)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def dib(rows):
    """32-bit BGRA DIB with an AND mask, as ICO entries expect."""
    size = len(rows)
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = b"".join(
        bytes((b, g, r, a)) for row in reversed(rows) for (r, g, b, a) in row
    )
    mask_row = ((size + 31) // 32) * 4
    mask = b"\x00" * (mask_row * size)
    return header + pixels + mask


def ico(images):
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    data = b""
    for size, blob in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return header + entries + data


if __name__ == "__main__":
    big = render(256)
    with open("icon.png", "wb") as f:
        f.write(png(big))
    images = [(s, dib(render(s))) for s in (16, 24, 32, 48, 64)]
    images.append((256, png(big)))
    with open("icon.ico", "wb") as f:
        f.write(ico(images))
    print("icon.png, icon.ico written")
