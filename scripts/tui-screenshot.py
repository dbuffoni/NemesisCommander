#!/usr/bin/env python3
"""Takes a picture of the terminal interface, for the README and the manual.

Runs `pnxr` in a pseudo terminal, types the keys, reads the screen with pyte
and draws it with Pillow, so the picture shows what the program really draws.

    scripts/tui-screenshot.py OUT.png [--keys "F9,Down"] [--size 124x32] [--theme dark|light]
                              [--pnxr target/debug/pnxr] -- LEFT RIGHT

Needs `pip install pyte pillow`. Keys are comma separated names (Enter, Tab,
Esc, Up, Down, Left, Right, F1..F10, Ctrl+X) or plain text prefixed with `text:`.
Set HOME, PNXR_CONFIG_DIR and PNXR_DATA_DIR yourself to get the same demo every time.
"""
import argparse, fcntl, os, pty, select, struct, sys, termios, time

import pyte
from PIL import Image, ImageDraw, ImageFont

FONT = "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf"
FONT_BOLD = "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Bold.ttf"
PALETTES = {
    "dark": {
        "bg": "1e2127", "fg": "d7dae0",
        "colors": {"black": "1e2127", "red": "e06c75", "green": "98c379", "brown": "e5c07b",
                   "blue": "61afef", "magenta": "c678dd", "cyan": "56b6c2", "white": "d7dae0",
                   "brightblack": "5c6370", "brightred": "e06c75", "brightgreen": "98c379",
                   "brightbrown": "e5c07b", "brightblue": "61afef", "brightmagenta": "c678dd",
                   "brightcyan": "56b6c2", "brightwhite": "ffffff"},
    },
    "light": {
        "bg": "fafafa", "fg": "383a42",
        "colors": {"black": "383a42", "red": "ca1243", "green": "50a14f", "brown": "c18401",
                   "blue": "4078f2", "magenta": "a626a4", "cyan": "0184bc", "white": "a0a1a7",
                   "brightblack": "696c77", "brightred": "e45649", "brightgreen": "50a14f",
                   "brightbrown": "c18401", "brightblue": "4078f2", "brightmagenta": "a626a4",
                   "brightcyan": "0184bc", "brightwhite": "383a42"},
    },
}
KEYS = {"Enter": "\r", "Tab": "\t", "Esc": "\x1b", "Up": "\x1b[A", "Down": "\x1b[B",
        "Right": "\x1b[C", "Left": "\x1b[D", "Space": " ", "Backspace": "\x7f",
        "F1": "\x1bOP", "F2": "\x1bOQ", "F3": "\x1bOR", "F4": "\x1bOS", "F5": "\x1b[15~",
        "F6": "\x1b[17~", "F7": "\x1b[18~", "F8": "\x1b[19~", "F9": "\x1b[20~", "F10": "\x1b[21~"}


def key_bytes(name):
    if name.startswith("text:"):
        return name[5:]
    if name.startswith("Ctrl+") and len(name) == 6:
        return chr(ord(name[5].upper()) - 64)
    return KEYS[name]


def run(args):
    cols, rows = map(int, args.size.split("x"))
    pid, fd = pty.fork()
    if pid == 0:
        os.environ.update(TERM="xterm-256color", COLORTERM="truecolor", LANG="C.UTF-8")
        os.execv(args.pnxr, [args.pnxr, *args.locations])
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    screen = pyte.Screen(cols, rows)
    stream = pyte.ByteStream(screen)

    def pump(seconds):
        end = time.time() + seconds
        while time.time() < end:
            ready, _, _ = select.select([fd], [], [], 0.1)
            if ready:
                try:
                    data = os.read(fd, 65536)
                except OSError:
                    return
                if not data:
                    return
                stream.feed(data)

    pump(2.0)
    for name in filter(None, args.keys.split(",")):
        os.write(fd, key_bytes(name).encode())
        pump(0.6)
    pump(0.5)
    os.kill(pid, 9)
    return screen, cols, rows


# Box drawing characters are drawn as lines so they join without gaps:
# (up, down, left, right) strengths, heavy ones count as light.
BOX = {"│": (1, 1, 0, 0), "─": (0, 0, 1, 1), "┌": (0, 1, 0, 1), "┐": (0, 1, 1, 0),
       "└": (1, 0, 0, 1), "┘": (1, 0, 1, 0), "├": (1, 1, 0, 1), "┤": (1, 1, 1, 0),
       "┬": (0, 1, 1, 1), "┴": (1, 0, 1, 1), "┼": (1, 1, 1, 1), "╭": (0, 1, 0, 1),
       "╮": (0, 1, 1, 0), "╰": (1, 0, 0, 1), "╯": (1, 0, 1, 0), "┃": (1, 1, 0, 0),
       "━": (0, 0, 1, 1), "┏": (0, 1, 0, 1), "┓": (0, 1, 1, 0), "┗": (1, 0, 0, 1),
       "┛": (1, 0, 1, 0)}


def box_char(d, char, x, y, w, h, fill):
    up, down, left, right = BOX[char]
    cx, cy = x + w // 2, y + h // 2
    if up:
        d.line([cx, y, cx, cy], fill=fill)
    if down:
        d.line([cx, cy, cx, y + h], fill=fill)
    if left:
        d.line([x, cy, cx, cy], fill=fill)
    if right:
        d.line([cx, cy, x + w, cy], fill=fill)


def draw(screen, cols, rows, theme, out):
    pal = PALETTES[theme]
    size = 18
    font, bold = ImageFont.truetype(FONT, size), ImageFont.truetype(FONT_BOLD, size)
    fallback = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", size - 2)
    cw, ch = round(font.getlength("M")), size + 6
    pad = 14
    img = Image.new("RGB", (cols * cw + 2 * pad, rows * ch + 2 * pad), "#" + pal["bg"])
    d = ImageDraw.Draw(img)

    def color(name, default):
        if name == "default":
            return default
        if name in pal["colors"]:
            return pal["colors"][name]
        return name if len(name) == 6 else default

    for y in range(rows):
        for x in range(cols):
            c = screen.buffer[y][x]
            fg, bg = color(c.fg, pal["fg"]), color(c.bg, pal["bg"])
            if c.reverse:
                fg, bg = bg, fg
            if bg != pal["bg"]:
                d.rectangle([pad + x * cw, pad + y * ch, pad + (x + 1) * cw, pad + (y + 1) * ch], fill="#" + bg)
            if c.data in BOX:
                box_char(d, c.data, pad + x * cw, pad + y * ch, cw, ch, "#" + fg)
            elif c.data.strip():
                face = fallback if 0x25A0 <= ord(c.data[0]) <= 0x26FF else (bold if c.bold else font)
                d.text((pad + x * cw, pad + y * ch), c.data, font=face, fill="#" + fg)
    img.save(out)


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("out")
    p.add_argument("--keys", default="")
    p.add_argument("--size", default="124x32")
    p.add_argument("--theme", default="dark", choices=PALETTES)
    p.add_argument("--pnxr", default="target/debug/pnxr")
    p.add_argument("locations", nargs="*")
    a = p.parse_args()
    s, c, r = run(a)
    draw(s, c, r, a.theme, a.out)
    print("wrote", a.out)
