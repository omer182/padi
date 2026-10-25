"""Generates hardware/wiring.svg (the PADI wiring diagram). Run: python hardware/make_wiring.py"""
from pathlib import Path

W, H = 1200, 780
C = dict(bg="#ffffff", ink="#1d232b", muted="#7a8494", line="#c9d0d9", pico="#2f8f57", pad="#1f9d61",
         pwr="#d23f3f", gnd="#3d4450", sda="#2f6fdb", scl="#c7770a", pot="#8a3fd2")
o = []
a = o.append


def gnd(x, y, col=C["gnd"]):
    a(f'<g stroke="{col}" stroke-width="2"><line x1="{x}" y1="{y}" x2="{x}" y2="{y+9}"/>'
      f'<line x1="{x-9}" y1="{y+9}" x2="{x+9}" y2="{y+9}"/><line x1="{x-6}" y1="{y+13}" x2="{x+6}" y2="{y+13}"/>'
      f'<line x1="{x-3}" y1="{y+17}" x2="{x+3}" y2="{y+17}"/></g>')


def pwr(x, y):
    a(f'<g stroke="{C["pwr"]}" stroke-width="2"><line x1="{x}" y1="{y}" x2="{x}" y2="{y-12}"/>'
      f'<line x1="{x-9}" y1="{y-12}" x2="{x+9}" y2="{y-12}"/></g>'
      f'<text x="{x}" y="{y-17}" text-anchor="middle" font-size="12" font-weight="700" fill="{C["pwr"]}">3V3</text>')


def wire(pts, col, w=2.5):
    p = " ".join(f"{x},{y}" for x, y in pts)
    a(f'<polyline points="{p}" fill="none" stroke="{col}" stroke-width="{w}" stroke-linejoin="round" stroke-linecap="round"/>')


def text(x, y, s, size=12, col=None, weight=None, anchor=None):
    extra = (f' font-weight="{weight}"' if weight else "") + (f' text-anchor="{anchor}"' if anchor else "")
    a(f'<text x="{x}" y="{y}" font-size="{size}" fill="{col or C["ink"]}"{extra}>{s}</text>')


a(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" '
  f'font-family="Segoe UI, Helvetica, Arial, sans-serif">')
a(f'<rect width="{W}" height="{H}" rx="18" fill="{C["bg"]}"/>')
text(40, 52, "PADI wiring", 26, weight=700)
text(40, 78, 'Raspberry Pi Pico · 16 keys · 0.96" SSD1306 OLED (I2C) · 10k potentiometer. Same-name labels are connected.',
     14, C["muted"])

# ---- Pico ----
PX0, PX1, PY0, PY1 = 540, 770, 100, 700
mid = (PX0 + PX1) / 2
a(f'<rect x="{PX0}" y="{PY0}" width="{PX1-PX0}" height="{PY1-PY0}" rx="10" fill="#eef7f1" stroke="{C["pico"]}" stroke-width="2.5"/>')
a(f'<rect x="{mid-26}" y="{PY0-16}" width="52" height="22" rx="4" fill="#dfe5ec" stroke="{C["muted"]}"/>')
text(mid, PY0 - 1, "USB", 10, C["muted"], anchor="middle")
text(mid, PY1 - 32, "Raspberry Pi Pico", 17, C["pico"], 700, "middle")
text(mid, PY1 - 14, "top view, USB up", 11, C["muted"], anchor="middle")

LEFT = ["GP0", "GP1", "GND", "GP2", "GP3", "GP4", "GP5", "GND", "GP6", "GP7", "GP8", "GP9", "GND",
        "GP10", "GP11", "GP12", "GP13", "GND", "GP14", "GP15"]
RIGHT = ["VBUS", "VSYS", "GND", "3V3_EN", "3V3(OUT)", "ADC_VREF", "GP28", "AGND", "GP27", "GP26", "RUN",
         "GP22", "GND", "GP21", "GP20", "GP19", "GP18", "GND", "GP17", "GP16"]
USED = {"3V3(OUT)": C["pwr"], "AGND": C["gnd"], "GP26": C["pot"], "GP17": C["scl"], "GP16": C["sda"]}


def py(i):
    return 128 + i * 26


for i, n in enumerate(LEFT):
    y = py(i)
    col = C["gnd"] if n == "GND" else C["pad"]
    a(f'<circle cx="{PX0}" cy="{y}" r="4" fill="{col}"/>')
    text(PX0 + 10, y + 4, n)
    text(PX0 + 84, y + 4, i + 1, 9, C["muted"], anchor="end")
    if n == "GND":
        wire([(PX0, y), (PX0 - 22, y)], C["gnd"], 2)
        gnd(PX0 - 22, y)
    else:
        pad = int(n[2:]) + 1
        wire([(PX0, y), (PX0 - 34, y)], C["pad"], 2)
        a(f'<rect x="{PX0-96}" y="{y-10}" width="62" height="20" rx="10" fill="{C["pad"]}"/>')
        text(PX0 - 65, y + 4, f"Pad {pad}", 11.5, "#fff", 700, "middle")

for i, n in enumerate(RIGHT):
    y = py(i)
    used = n in USED or (n == "GND" and i == 2)
    col = USED.get(n, C["gnd"] if used else "#b9c1cc")
    a(f'<circle cx="{PX1}" cy="{y}" r="4" fill="{col}"/>')
    text(PX1 - 10, y + 4, n, 12, C["ink"] if used else C["muted"], anchor="end")
    text(PX1 - 92, y + 4, 40 - i, 9, C["muted"])


def ry(n):
    return py(RIGHT.index(n))


y = py(2)
wire([(PX1, y), (PX1 + 24, y)], C["gnd"], 2)
gnd(PX1 + 24, y)
y = ry("3V3(OUT)")
wire([(PX1, y), (PX1 + 26, y)], C["pwr"], 2)
pwr(PX1 + 26, y)
y = ry("AGND")
wire([(PX1, y), (PX1 + 24, y)], C["gnd"], 2)
gnd(PX1 + 24, y)

# ---- Potentiometer ----
X, T, B = 930, 226, 330
pwr(X, T)
wire([(X, T), (X, 250)], C["pwr"], 2)
zz = [(X, 250)] + [(X + (10 if k % 2 == 0 else -10), 258 + k * 12) for k in range(6)] + [(X, 320)]
wire(zz, C["ink"], 2)
wire([(X, 320), (X, B)], C["gnd"], 2)
gnd(X, B)
yw = ry("GP26")
wire([(PX1, yw), (860, yw), (860, 285), (906, 285)], C["pot"])
a(f'<polygon points="{X-14},285 {X-26},279 {X-26},291" fill="{C["pot"]}"/>')
text(X + 26, 270, "Knob", 15, weight=700)
text(X + 26, 289, "10k linear pot (B10K)", 12, C["muted"])
text(X + 26, 306, "middle leg → GP26 (ADC0)", 12, C["muted"])
text(X + 26, 323, "outer legs → 3V3 and AGND", 12, C["muted"])

# ---- OLED ----
OX, OY = 890, 468
a(f'<rect x="{OX}" y="{OY}" width="270" height="158" rx="10" fill="#f2f5f9" stroke="{C["ink"]}" stroke-width="2"/>')
a(f'<rect x="{OX+86}" y="{OY+18}" width="166" height="96" rx="4" fill="#0d1117"/>')
a(f'<text x="{OX+169}" y="{OY+62}" text-anchor="middle" font-size="22" font-weight="800" fill="#7cf5b4" letter-spacing="3">PADI</text>')
a(f'<text x="{OX+169}" y="{OY+86}" text-anchor="middle" font-size="10" fill="#7cf5b4" opacity=".7">128×64 SSD1306</text>')
text(OX + 169, OY + 140, '0.96" I2C OLED (address 0x3C)', 12, C["muted"], anchor="middle")
pins = [("GND", OY + 26), ("VCC", OY + 56), ("SCL", OY + 86), ("SDA", OY + 116)]
for n, yy in pins:
    a(f'<circle cx="{OX}" cy="{yy}" r="4" fill="{C["ink"]}"/>')
    text(OX + 10, yy + 4, n, 12, weight=600)
yy = pins[0][1]
wire([(OX, yy), (OX - 26, yy)], C["gnd"], 2)
gnd(OX - 26, yy)
yy = pins[1][1]
wire([(OX, yy), (OX - 56, yy)], C["pwr"], 2)
pwr(OX - 56, yy)
wire([(PX1, ry("GP17")), (800, ry("GP17")), (800, pins[2][1]), (OX, pins[2][1])], C["scl"])
wire([(PX1, ry("GP16")), (822, ry("GP16")), (822, pins[3][1]), (OX, pins[3][1])], C["sda"])
text(850, pins[2][1] - 6, "SCL", 11, C["scl"], 700, "middle")
text(856, pins[3][1] - 6, "SDA", 11, C["sda"], 700, "middle")

# ---- Keys ----
KX, KY, KS, KG = 40, 172, 72, 10
text(KX, KY - 22, "16 keys (MX-style switches)", 15, weight=700)
for r, row in enumerate([[13, 14, 15, 16], [9, 10, 11, 12], [5, 6, 7, 8], [1, 2, 3, 4]]):
    for c, p in enumerate(row):
        x, y = KX + c * (KS + KG), KY + r * (KS + KG)
        a(f'<rect x="{x}" y="{y}" width="{KS}" height="{KS}" rx="10" fill="#f2f5f9" stroke="{C["line"]}" stroke-width="1.5"/>')
        text(x + KS / 2, y + 34, f"Pad {p}", 15, C["pad"], 700, "middle")
        text(x + KS / 2, y + 53, f"GP{p-1}", 11.5, C["muted"], anchor="middle")

IY = 530
text(KX, IY, "How each key connects", 13, weight=700)
y = IY + 34
a(f'<rect x="{KX}" y="{y-11}" width="62" height="22" rx="11" fill="{C["pad"]}"/>')
text(KX + 31, y + 4, "Pad N", 11.5, "#fff", 700, "middle")
wire([(KX + 62, y), (KX + 120, y)], C["pad"])
a(f'<circle cx="{KX+124}" cy="{y}" r="4" fill="none" stroke="{C["ink"]}" stroke-width="2"/>'
  f'<circle cx="{KX+176}" cy="{y}" r="4" fill="none" stroke="{C["ink"]}" stroke-width="2"/>'
  f'<line x1="{KX+127}" y1="{y-3}" x2="{KX+172}" y2="{y-20}" stroke="{C["ink"]}" stroke-width="2"/>')
wire([(KX + 180, y), (KX + 232, y)], C["gnd"], 2)
gnd(KX + 232, y)
text(KX + 150, y + 26, "switch", 11, C["muted"], anchor="middle")
text(KX, y + 52, "One leg to its GP pin, the other leg to GND.", 12, C["muted"])
text(KX, y + 69, "Chain all 16 GND legs together, then to any GND pin.", 12, C["muted"])
text(KX, y + 86, "No diodes or resistors needed (internal pull-ups).", 12, C["muted"])

# ---- legend + notes ----
for k, (n, col) in enumerate([("Keys", C["pad"]), ("3V3", C["pwr"]), ("GND", C["gnd"]),
                              ("Knob", C["pot"]), ("SCL", C["scl"]), ("SDA", C["sda"])]):
    x, y = 890 + (k % 3) * 95, 690 + (k // 3) * 24
    a(f'<line x1="{x}" y1="{y}" x2="{x+22}" y2="{y}" stroke="{col}" stroke-width="4" stroke-linecap="round"/>')
    text(x + 30, y + 4, n)
text(PX0, PY1 + 30, "Menu LED = the Pico's onboard LED (GP25). Nothing to wire.", 12, C["muted"])
text(PX0, PY1 + 48, "Power comes from USB. Small numbers are physical pin numbers.", 12, C["muted"])
a("</svg>")

out = Path(__file__).with_name("wiring.svg")
out.write_text("\n".join(o), encoding="utf-8")
print("wrote", out)
