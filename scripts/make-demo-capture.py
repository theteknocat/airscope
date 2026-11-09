#!/usr/bin/env python3
"""Writes an invented Bluetooth capture (btmon "monitor" btsnoop format) for the README screenshots.
Usage: make-demo-capture.py OUT.snoop
Two devices from the app's demo data (Studio Headphones, Desk Keyboard) over 2 hours, with two bursts of
connection-timeout drops, some slow sniff exits and a few normal disconnects. Nothing here is real."""
import random, struct, sys

random.seed(7)
EPOCH = 0x00DCDDB30F2F8000          # btsnoop timestamps count microseconds from year 0
START = 1794168000                  # a Tuesday evening, 2026-11-09 18:40 UTC (invented)
DEV = {"AC:80:0A:3D:62:C0": 0x0041, "E4:11:5B:72:08:4D": 0x0042}   # MAC -> first handle

def wire(mac):                      # the address as it appears in HCI events: least significant byte first
    return bytes(int(x, 16) for x in reversed(mac.split(":")))

out = bytearray(b"btsnoop\0" + struct.pack(">II", 1, 2001))
def rec(t, op, data):
    out.extend(struct.pack(">IIIIq", len(data), len(data), op, 0, int((START + t) * 1e6) + EPOCH) + data)

def event(t, code, p): rec(t, 3, bytes([code, len(p)]) + p)
def connect(t, mac, h): event(t, 0x03, bytes([0]) + struct.pack("<H", h) + wire(mac) + bytes([1, 0]))
def disconnect(t, h, reason): event(t, 0x05, bytes([0]) + struct.pack("<H", h) + bytes([reason]))
def acl(t, h, tx): rec(t, 4 if tx else 5, struct.pack("<HHH", h, 8, 4) + b"\0" * 8)
def sniff_exit(t, h, answered_after):
    rec(t, 2, struct.pack("<HBH", 0x0804, 2, h))
    if answered_after is not None:
        event(t + answered_after, 0x14, bytes([0]) + struct.pack("<H", h) + bytes([0]) + struct.pack("<H", 0))

handle = dict(DEV)
alive = {m: True for m in DEV}
for m, h in DEV.items():
    connect(0.2, m, h)

# drops: (time s, who, reason, quiet seconds before it)
drops = [(1630, "both", 0x08, 22), (1674, "both", 0x08, 19), (1732, "kb", 0x08, 27), (1790, "both", 0x08, 20),
         (3490, "hp", 0x13, 1), (4310, "both", 0x08, 21), (4336, "both", 0x08, 18), (4401, "both", 0x08, 24),
         (4466, "kb", 0x08, 17), (5120, "both", 0x08, 23), (6350, "hp", 0x15, 1), (6900, "kb", 0x08, 20)]
quiet = {}   # (mac, t0, t1): ACL stops between t0 and t1
who = {"hp": ["AC:80:0A:3D:62:C0"], "kb": ["E4:11:5B:72:08:4D"], "both": list(DEV)}
for t, w, reason, q in drops:
    for m in who[w]:
        quiet[(m, t - q, t)] = reason

# each drop gives the device a new connection handle, so traffic must use the handle that is current at that moment
drop_times = {m: sorted(t + i * 0.12 for (t, w, r, q) in drops for i, mm in enumerate(who[w]) if mm == m) for m in DEV}
def handle_at(m, t):
    return DEV[m] + 2 * sum(1 for d in drop_times[m] if d <= t)
def link_up(m, t):                  # not in the second between a drop and the reconnect
    return not any(d <= t < d + 1.1 for d in drop_times[m])

t = 1.0
while t < 7200:
    for m in DEV:
        if link_up(m, t) and not any(m == mm and a <= t < b for (mm, a, b) in quiet):
            if random.random() < 0.8:
                acl(t + random.random() * 0.3, handle_at(m, t), True)
            acl(t + 0.4 + random.random() * 0.3, handle_at(m, t), False)
    t += random.uniform(1.0, 2.2)

for (t, w, reason, q) in drops:
    for i, m in enumerate(who[w]):
        td = t + i * 0.12
        if reason == 0x08 and m.startswith("E4") and q >= 19:
            sniff_exit(td - q + 8, handle[m], random.choice([None, 4.5, 9.0]))   # exit that the controller answers late, or never
        disconnect(td, handle[m], reason)
        handle[m] += 2
        connect(td + 1.1, m, handle[m])

# one drop of a link that was already up when the capture began, and a normal idle disconnect
rec(0.1, 3, bytes([5, 4, 0, 0x30, 0x00, 0x08]))

# records must be in time order
body = out[16:]
recs, o = [], 0
while o < len(body):
    n = struct.unpack(">I", body[o + 4:o + 8])[0]
    recs.append((struct.unpack(">q", body[o + 16:o + 24])[0], bytes(body[o:o + 24 + n])))
    o += 24 + n
recs.sort(key=lambda r: r[0])
with open(sys.argv[1], "wb") as f:
    f.write(bytes(out[:16]))
    for _, r in recs:
        f.write(r)
