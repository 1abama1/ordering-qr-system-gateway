"""Reproducible code-drawn ICO, no external media or packages."""
from pathlib import Path
import struct
size = 64
pixels = bytearray()
for y in range(size - 1, -1, -1):
    for x in range(size):
        background = (20, 30, 24, 255)
        inside = (16 <= x <= 23 and 13 <= y <= 47) or (23 <= x <= 37 and (13 <= y <= 20 or 40 <= y <= 47)) or (37 <= x <= 44 and 20 <= y <= 40) or (16 <= x <= 44 and 52 <= y <= 55)
        r,g,b,a = (181,244,107,255) if inside else background
        pixels.extend((b,g,r,a))
bitmap = struct.pack('<IIIHHIIIIII',40,size,size*2,1,32,0,len(pixels),0,0,0,0)+pixels+bytes(size*size//8)
data = struct.pack('<HHH',0,1,1)+struct.pack('<BBBBHHII',size,size,0,0,1,32,len(bitmap),22)+bitmap
destination = Path(__file__).resolve().parents[1] / 'src-tauri/icons/icon.ico'
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_bytes(data)
