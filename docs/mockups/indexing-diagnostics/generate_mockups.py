#!/usr/bin/env python3
"""Generate GNOME-style mockup images for indexing diagnostics exploration."""

from PIL import Image, ImageDraw, ImageFont
import os

OUTPUT_DIR = os.path.dirname(os.path.abspath(__file__))

# ── Fonts ──────────────────────────────────────────────────────────────
FONT_REGULAR = "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf"
FONT_MONO = "/usr/share/fonts/adwaita-mono-fonts/AdwaitaMono-Regular.ttf" if os.path.exists("/usr/share/fonts/adwaita-mono-fonts/AdwaitaMono-Regular.ttf") else "/usr/share/fonts/google-noto-vf/NotoSansMono[wght].ttf"

def font(size, bold=False):
    try:
        return ImageFont.truetype(FONT_REGULAR, size)
    except Exception:
        return ImageFont.load_default()

def mono_font(size):
    try:
        return ImageFont.truetype(FONT_MONO, size)
    except Exception:
        return ImageFont.load_default()

# ── GNOME Adwaita Dark Color Palette ───────────────────────────────────
BG = (36, 36, 36)              # Window background
CARD_BG = (48, 48, 48)         # Card/group background
CARD_BORDER = (62, 62, 62)     # Card border
HEADER_BG = (43, 43, 43)       # Header bar
TEXT = (255, 255, 255)          # Primary text
TEXT_DIM = (154, 154, 154)     # Secondary/dim text
TEXT_VERY_DIM = (110, 110, 110)
ACCENT = (98, 160, 234)        # Blue accent
SUCCESS = (38, 162, 105)       # Green
WARNING = (230, 97, 0)         # Orange
ERROR = (224, 27, 36)          # Red
SEPARATOR = (58, 58, 58)       # Row separators
PILL_SUCCESS_BG = (38, 162, 105, 40)
PILL_WARNING_BG = (230, 97, 0, 40)
PILL_ERROR_BG = (224, 27, 36, 40)
BANNER_WARNING_BG = (70, 55, 30)  # Warning banner bg
BANNER_ERROR_BG = (65, 30, 30)
PROGRESS_BG = (62, 62, 62)
PROGRESS_FG = ACCENT
TAB_ACTIVE = ACCENT
TAB_INACTIVE = TEXT_DIM
BUTTON_BG = (62, 62, 62)
BUTTON_ACCENT_BG = (53, 132, 228)
BOTTOM_PANEL_BG = (30, 30, 30)
HEALTH_GOOD = SUCCESS
HEALTH_WARN = WARNING
HEALTH_BAD = ERROR
HEALTH_NONE = (80, 80, 80)


# ── Drawing Helpers ────────────────────────────────────────────────────

def draw_rounded_rect(draw, xy, radius, fill=None, outline=None, width=1):
    x0, y0, x1, y1 = xy
    r = radius
    if fill:
        draw.rectangle([x0+r, y0, x1-r, y1], fill=fill)
        draw.rectangle([x0, y0+r, x1, y1-r], fill=fill)
        draw.pieslice([x0, y0, x0+2*r, y0+2*r], 180, 270, fill=fill)
        draw.pieslice([x1-2*r, y0, x1, y0+2*r], 270, 360, fill=fill)
        draw.pieslice([x0, y1-2*r, x0+2*r, y1], 90, 180, fill=fill)
        draw.pieslice([x1-2*r, y1-2*r, x1, y1], 0, 90, fill=fill)
    if outline:
        draw.arc([x0, y0, x0+2*r, y0+2*r], 180, 270, fill=outline, width=width)
        draw.arc([x1-2*r, y0, x1, y0+2*r], 270, 360, fill=outline, width=width)
        draw.arc([x0, y1-2*r, x0+2*r, y1], 90, 180, fill=outline, width=width)
        draw.arc([x1-2*r, y1-2*r, x1, y1], 0, 90, fill=outline, width=width)
        draw.line([x0+r, y0, x1-r, y0], fill=outline, width=width)
        draw.line([x0+r, y1, x1-r, y1], fill=outline, width=width)
        draw.line([x0, y0+r, x0, y1-r], fill=outline, width=width)
        draw.line([x1, y0+r, x1, y1-r], fill=outline, width=width)

def draw_pill(draw, x, y, text, color, bg_color):
    f = font(13)
    bbox = draw.textbbox((0, 0), text, font=f)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    pw, ph = tw + 16, th + 8
    draw_rounded_rect(draw, (x, y, x+pw, y+ph), ph//2, fill=bg_color)
    draw.text((x + 8, y + 4), text, fill=color, font=f)
    return pw

def draw_status_icon(draw, x, y, status, size=14):
    """Draw a small status dot."""
    colors = {"ok": SUCCESS, "warn": WARNING, "error": ERROR, "none": HEALTH_NONE}
    c = colors.get(status, HEALTH_NONE)
    draw.ellipse([x, y, x+size, y+size], fill=c)

def draw_separator(draw, x0, x1, y):
    draw.line([(x0, y), (x1, y)], fill=SEPARATOR, width=1)

def draw_header_bar(draw, x, y, w, title, buttons_right=None, spinner=False):
    """Draw a GNOME-style header bar."""
    draw_rounded_rect(draw, (x, y, x+w, y+46), 12, fill=HEADER_BG)
    # Bottom corners are square (it connects to content)
    draw.rectangle([x, y+24, x+w, y+46], fill=HEADER_BG)

    # Title centered
    f = font(15)
    bbox = draw.textbbox((0, 0), title, font=f)
    tw = bbox[2] - bbox[0]
    draw.text((x + (w - tw) // 2, y + 14), title, fill=TEXT, font=f)

    # Close button
    draw.text((x + w - 36, y + 12), "x", fill=TEXT_DIM, font=font(16))

    if spinner:
        # Draw simple spinner (circle arc)
        draw.arc([x + w - 70, y + 13, x + w - 50, y + 33], 0, 270, fill=ACCENT, width=2)

    if buttons_right:
        bx = x + w - 80
        for btn_text, btn_style in reversed(buttons_right):
            bf = font(13)
            bbox = draw.textbbox((0, 0), btn_text, font=bf)
            bw = bbox[2] - bbox[0] + 20
            bg = BUTTON_ACCENT_BG if btn_style == "accent" else BUTTON_BG
            draw_rounded_rect(draw, (bx - bw, y + 9, bx, y + 37), 6, fill=bg)
            draw.text((bx - bw + 10, y + 14), btn_text, fill=TEXT, font=bf)
            bx -= bw + 8

    return y + 46

def draw_adw_action_row(draw, x, y, w, title, subtitle=None, suffix_text=None,
                         suffix_color=TEXT_DIM, prefix_color=None, status=None,
                         mono_subtitle=False, expandable=False, expanded=False):
    """Draw an AdwActionRow-style row."""
    h = 52 if subtitle else 44
    # Prefix status dot
    px = x + 16
    if prefix_color:
        draw.ellipse([px, y + h//2 - 6, px + 12, y + h//2 + 6], fill=prefix_color)
        px += 24
    if status:
        draw_status_icon(draw, px, y + h//2 - 7, status)
        px += 24

    # Title
    draw.text((px, y + (10 if subtitle else h//2 - 8)), title, fill=TEXT, font=font(14))

    # Subtitle
    if subtitle:
        sf = mono_font(12) if mono_subtitle else font(12)
        draw.text((px, y + 30), subtitle, fill=TEXT_DIM, font=sf)

    # Suffix
    if suffix_text:
        sf = font(14)
        bbox = draw.textbbox((0, 0), suffix_text, font=sf)
        sw = bbox[2] - bbox[0]
        draw.text((x + w - 16 - sw, y + h//2 - 8), suffix_text, fill=suffix_color, font=sf)

    # Expander arrow
    if expandable:
        arrow = "v" if expanded else ">"
        draw.text((x + w - 16, y + h//2 - 8), arrow, fill=TEXT_DIM, font=font(14))

    return y + h

def draw_preferences_group(draw, x, y, w, title, description=None, rows=None):
    """Draw an AdwPreferencesGroup with rows."""
    # Group title
    draw.text((x + 4, y), title, fill=TEXT, font=font(14))
    cy = y + 24
    if description:
        draw.text((x + 4, cy), description, fill=TEXT_DIM, font=font(12))
        cy += 20
    cy += 4

    if not rows:
        return cy

    # Card background
    total_h = sum(r.get('height', 52 if r.get('subtitle') else 44) for r in rows) + (len(rows) - 1)
    draw_rounded_rect(draw, (x, cy, x + w, cy + total_h), 12, fill=CARD_BG)

    for i, row in enumerate(rows):
        rh = row.get('height', 52 if row.get('subtitle') else 44)
        draw_adw_action_row(draw, x, cy, w,
                           row.get('title', ''),
                           row.get('subtitle'),
                           row.get('suffix'),
                           row.get('suffix_color', TEXT_DIM),
                           row.get('prefix_color'),
                           row.get('status'),
                           row.get('mono_subtitle', False),
                           row.get('expandable', False),
                           row.get('expanded', False))
        cy += rh
        if i < len(rows) - 1:
            draw_separator(draw, x + 16, x + w - 16, cy)
            cy += 1

    return cy + 16


# ════════════════════════════════════════════════════════════════════════
# PROPOSAL A: Preferences Extension (UI Designer)
# ════════════════════════════════════════════════════════════════════════

def generate_proposal_a():
    W, H = 520, 720
    img = Image.new('RGB', (W, H), BG)
    draw = ImageDraw.Draw(img)

    # Dialog header
    cy = 0
    draw_rounded_rect(draw, (0, 0, W, H), 12, fill=BG, outline=CARD_BORDER)
    cy = draw_header_bar(draw, 0, 0, W, "Preferences")

    # Scrollable content
    cy += 8
    mx = 24  # margin
    cw = W - 2 * mx  # content width

    # ── Session Sources group ──
    cy = draw_preferences_group(draw, mx, cy, cw, "Session Sources", rows=[
        {"title": "Claude Code", "subtitle": "~/.claude/projects", "mono_subtitle": True,
         "status": "ok", "suffix": "182 sessions", "suffix_color": TEXT_DIM},
        {"title": "OpenCode", "subtitle": "~/.local/share/opencode/storage", "mono_subtitle": True,
         "status": "warn", "suffix": "38 sessions", "suffix_color": WARNING},
        {"title": "Codex", "subtitle": "Directory not found",
         "status": "none", "suffix": "N/A", "suffix_color": TEXT_VERY_DIM},
        {"title": "Mistral Vibe", "subtitle": "~/.vibe/logs/session", "mono_subtitle": True,
         "status": "ok", "suffix": "6 sessions", "suffix_color": TEXT_DIM},
    ])

    cy += 8

    # ── Last Indexing Run group ──
    cy = draw_preferences_group(draw, mx, cy, cw, "Last Indexing Run", rows=[
        {"title": "Status", "subtitle": "Completed with 3 errors", "suffix_color": WARNING},
        {"title": "Sessions Indexed", "suffix": "226", "suffix_color": TEXT},
        {"title": "Duration", "suffix": "0.8s", "suffix_color": TEXT_DIM},
        {"title": "Last Run", "suffix": "2 minutes ago", "suffix_color": TEXT_DIM},
    ])

    cy += 8

    # ── Advanced group (existing) ──
    cy = draw_preferences_group(draw, mx, cy, cw, "Advanced", rows=[
        {"title": "Database Location", "subtitle": "~/.local/share/sessions-chronicle/sessions.db", "mono_subtitle": True},
        {"title": "Reset Session Index", "suffix": "Reset", "suffix_color": ERROR},
    ])

    # Label
    draw.text((mx, H - 30), "Proposal A: Preferences Extension (UI Designer)", fill=TEXT_DIM, font=font(11))

    img.save(os.path.join(OUTPUT_DIR, "proposal-a-preferences.png"))
    print("Generated proposal-a-preferences.png")


# ════════════════════════════════════════════════════════════════════════
# PROPOSAL A: Initial state variant
# ════════════════════════════════════════════════════════════════════════

def generate_proposal_a_initial():
    W, H = 520, 580
    img = Image.new('RGB', (W, H), BG)
    draw = ImageDraw.Draw(img)

    draw_rounded_rect(draw, (0, 0, W, H), 12, fill=BG, outline=CARD_BORDER)
    cy = draw_header_bar(draw, 0, 0, W, "Preferences")
    cy += 8
    mx = 24
    cw = W - 2 * mx

    # Sources - initial state (no indexing yet)
    cy = draw_preferences_group(draw, mx, cy, cw, "Session Sources", rows=[
        {"title": "Claude Code", "subtitle": "~/.claude/projects", "mono_subtitle": True},
        {"title": "OpenCode", "subtitle": "~/.local/share/opencode/storage", "mono_subtitle": True},
        {"title": "Codex", "subtitle": "~/.codex/sessions", "mono_subtitle": True},
        {"title": "Mistral Vibe", "subtitle": "~/.vibe/logs/session", "mono_subtitle": True},
    ])

    cy += 8

    cy = draw_preferences_group(draw, mx, cy, cw, "Last Indexing Run", rows=[
        {"title": "Status", "subtitle": "Not yet indexed"},
    ])

    draw.text((mx, H - 30), "Proposal A: Initial State (before first indexing)", fill=TEXT_DIM, font=font(11))

    img.save(os.path.join(OUTPUT_DIR, "proposal-a-initial.png"))
    print("Generated proposal-a-initial.png")


# ════════════════════════════════════════════════════════════════════════
# PROPOSAL B: Dedicated Dialog (UI Designer)
# ════════════════════════════════════════════════════════════════════════

def generate_proposal_b():
    W, H = 520, 780
    img = Image.new('RGB', (W, H), BG)
    draw = ImageDraw.Draw(img)

    draw_rounded_rect(draw, (0, 0, W, H), 12, fill=BG, outline=CARD_BORDER)
    cy = draw_header_bar(draw, 0, 0, W, "Indexing Status",
                         buttons_right=[("Re-index All", "accent")])

    cy += 16
    mx = 24
    cw = W - 2 * mx

    # ── Summary Status Page ──
    f_title = font(22)
    title = "226 sessions indexed"
    bbox = draw.textbbox((0, 0), title, font=f_title)
    tw = bbox[2] - bbox[0]
    draw.text(((W - tw) // 2, cy), title, fill=TEXT, font=f_title)
    cy += 32

    sub = "Completed with 3 errors -- 2 minutes ago"
    bbox = draw.textbbox((0, 0), sub, font=font(13))
    sw = bbox[2] - bbox[0]
    draw.text(((W - sw) // 2, cy), sub, fill=WARNING, font=font(13))
    cy += 32

    # ── Sources group with expander rows ──
    draw.text((mx + 4, cy), "Sources", fill=TEXT, font=font(14))
    cy += 28

    # Card
    card_top = cy
    card_h = 310
    draw_rounded_rect(draw, (mx, cy, mx + cw, cy + card_h), 12, fill=CARD_BG)

    # Claude Code (expanded)
    draw_status_icon(draw, mx + 16, cy + 18, "ok")
    draw.text((mx + 40, cy + 14), "Claude Code", fill=TEXT, font=font(14))
    draw_pill(draw, mx + cw - 80, cy + 12, "182", SUCCESS, (38, 80, 55))
    draw.text((mx + cw - 20, cy + 14), "v", fill=TEXT_DIM, font=font(14))
    cy += 44

    # Expanded content
    inner_x = mx + 32
    inner_w = cw - 48
    sub_rows = [
        ("Source Path", "~/.claude/projects", True),
        ("Sessions Indexed", "182", False),
        ("Skipped (unchanged)", "14", False),
        ("Parse Errors", "0", False),
    ]
    for label, val, is_path in sub_rows:
        draw_separator(draw, inner_x, mx + cw - 16, cy)
        cy += 1
        draw.text((inner_x + 8, cy + 10), label, fill=TEXT_DIM, font=font(12))
        if is_path:
            draw.text((inner_x + 8, cy + 26), val, fill=TEXT_DIM, font=mono_font(11))
            cy += 44
        else:
            vf = font(13)
            bbox = draw.textbbox((0, 0), val, font=vf)
            vw = bbox[2] - bbox[0]
            draw.text((mx + cw - 28 - vw, cy + 10), val, fill=TEXT, font=vf)
            cy += 36

    draw_separator(draw, mx + 16, mx + cw - 16, cy)
    cy += 1

    # OpenCode row
    draw_status_icon(draw, mx + 16, cy + 18, "warn")
    draw.text((mx + 40, cy + 14), "OpenCode", fill=TEXT, font=font(14))
    draw_pill(draw, mx + cw - 64, cy + 12, "38", WARNING, (70, 55, 30))
    draw.text((mx + cw - 20, cy + 14), ">", fill=TEXT_DIM, font=font(14))
    cy += 44

    draw_separator(draw, mx + 16, mx + cw - 16, cy)
    cy += 1

    # Codex row (not found)
    draw_status_icon(draw, mx + 16, cy + 15, "none")
    draw.text((mx + 40, cy + 8), "Codex", fill=TEXT, font=font(14))
    draw.text((mx + 40, cy + 28), "Directory not found", fill=TEXT_VERY_DIM, font=font(12))
    draw_pill(draw, mx + cw - 60, cy + 12, "N/A", TEXT_VERY_DIM, (50, 50, 50))
    cy += 52

    draw_separator(draw, mx + 16, mx + cw - 16, cy)
    cy += 1

    # Mistral Vibe row
    draw_status_icon(draw, mx + 16, cy + 18, "ok")
    draw.text((mx + 40, cy + 14), "Mistral Vibe", fill=TEXT, font=font(14))
    draw_pill(draw, mx + cw - 52, cy + 12, "6", SUCCESS, (38, 80, 55))
    draw.text((mx + cw - 20, cy + 14), ">", fill=TEXT_DIM, font=font(14))
    cy += 44

    cy += 16

    # ── Error Log ──
    draw.text((mx + 4, cy), "Recent Errors", fill=TEXT, font=font(14))
    cy += 4
    draw.text((mx + 4, cy + 18), "Most recent parse errors encountered", fill=TEXT_DIM, font=font(12))
    cy += 38

    err_card_top = cy
    errors = [
        ("opencode/session_abc.json", "Unexpected EOF at line 42"),
        ("opencode/session_def.json", "Invalid timestamp in message"),
        ("opencode/session_ghi.json", "Missing required field \"role\""),
    ]

    err_h = len(errors) * 52 + (len(errors) - 1)
    draw_rounded_rect(draw, (mx, cy, mx + cw, cy + err_h), 12, fill=CARD_BG)

    for i, (fname, msg) in enumerate(errors):
        # Warning icon
        draw.text((mx + 16, cy + 16), "!", fill=WARNING, font=font(16))
        draw.text((mx + 40, cy + 10), fname, fill=TEXT, font=font(13))
        draw.text((mx + 40, cy + 30), msg, fill=TEXT_DIM, font=font(12))
        cy += 52
        if i < len(errors) - 1:
            draw_separator(draw, mx + 16, mx + cw - 16, cy)
            cy += 1

    # Label
    draw.text((mx, H - 30), "Proposal B: Dedicated Dialog (UI Designer)", fill=TEXT_DIM, font=font(11))

    img.save(os.path.join(OUTPUT_DIR, "proposal-b-dialog.png"))
    print("Generated proposal-b-dialog.png")


# ════════════════════════════════════════════════════════════════════════
# PROPOSAL B: Indexing in progress variant
# ════════════════════════════════════════════════════════════════════════

def generate_proposal_b_progress():
    W, H = 520, 460
    img = Image.new('RGB', (W, H), BG)
    draw = ImageDraw.Draw(img)

    draw_rounded_rect(draw, (0, 0, W, H), 12, fill=BG, outline=CARD_BORDER)
    cy = draw_header_bar(draw, 0, 0, W, "Indexing Status", spinner=True)

    cy += 24
    mx = 24
    cw = W - 2 * mx

    # Status page with spinner text
    title = "Indexing in progress..."
    f_title = font(22)
    bbox = draw.textbbox((0, 0), title, font=f_title)
    tw = bbox[2] - bbox[0]
    draw.text(((W - tw) // 2, cy), title, fill=TEXT, font=f_title)
    cy += 32

    sub = "Processing session files"
    bbox = draw.textbbox((0, 0), sub, font=font(13))
    sw = bbox[2] - bbox[0]
    draw.text(((W - sw) // 2, cy), sub, fill=TEXT_DIM, font=font(13))
    cy += 28

    # Progress bar (indeterminate)
    bar_w = cw - 80
    bar_x = (W - bar_w) // 2
    draw_rounded_rect(draw, (bar_x, cy, bar_x + bar_w, cy + 6), 3, fill=PROGRESS_BG)
    # Indeterminate chunk
    chunk_w = bar_w // 3
    draw_rounded_rect(draw, (bar_x + bar_w // 4, cy, bar_x + bar_w // 4 + chunk_w, cy + 6), 3, fill=PROGRESS_FG)
    cy += 32

    # Sources (compact, no stats yet)
    draw.text((mx + 4, cy), "Sources", fill=TEXT, font=font(14))
    cy += 28

    sources = ["Claude Code", "OpenCode", "Codex", "Mistral Vibe"]
    card_h = len(sources) * 44 + (len(sources) - 1)
    draw_rounded_rect(draw, (mx, cy, mx + cw, cy + card_h), 12, fill=CARD_BG)

    for i, name in enumerate(sources):
        draw.text((mx + 16, cy + 14), name, fill=TEXT, font=font(14))
        cy += 44
        if i < len(sources) - 1:
            draw_separator(draw, mx + 16, mx + cw - 16, cy)
            cy += 1

    draw.text((mx, H - 30), "Proposal B: Indexing In Progress", fill=TEXT_DIM, font=font(11))

    img.save(os.path.join(OUTPUT_DIR, "proposal-b-progress.png"))
    print("Generated proposal-b-progress.png")


# ════════════════════════════════════════════════════════════════════════
# PROPOSAL C: GNOME HIG Pure (Banner + Inline Status)
# ════════════════════════════════════════════════════════════════════════

def generate_proposal_c():
    """Main window with AdwBanner for indexing warnings."""
    W, H = 800, 600
    img = Image.new('RGB', (W, H), BG)
    draw = ImageDraw.Draw(img)

    # Window outline
    draw_rounded_rect(draw, (0, 0, W, H), 12, fill=BG, outline=CARD_BORDER)

    # Header bar
    draw_rounded_rect(draw, (0, 0, W, 46), 12, fill=HEADER_BG)
    draw.rectangle([0, 24, W, 46], fill=HEADER_BG)

    # View switcher tabs
    tabs = [("Sessions", True), ("Analytics", False)]
    tx = W // 2 - 80
    for label, active in tabs:
        c = TAB_ACTIVE if active else TAB_INACTIVE
        draw.text((tx, 14), label, fill=c, font=font(14))
        if active:
            bbox = draw.textbbox((0, 0), label, font=font(14))
            tw = bbox[2] - bbox[0]
            draw.line([(tx, 42), (tx + tw, 42)], fill=ACCENT, width=3)
        tx += 100

    # Menu icon
    draw.text((W - 40, 14), "=", fill=TEXT_DIM, font=font(16))

    # Search icon
    draw.text((W - 80, 14), "Q", fill=TEXT_DIM, font=font(14))

    cy = 46

    # ── AdwBanner (warning) ──
    banner_h = 40
    draw.rectangle([0, cy, W, cy + banner_h], fill=BANNER_WARNING_BG)
    # Warning icon + text
    banner_text = "Indexing completed with 3 errors -- 1 source not found"
    draw.text((16, cy + 12), "!", fill=WARNING, font=font(16))
    draw.text((40, cy + 12), banner_text, fill=TEXT, font=font(13))
    # "Details" button
    btn_text = "Details"
    bf = font(13)
    bbox = draw.textbbox((0, 0), btn_text, font=bf)
    bw = bbox[2] - bbox[0] + 16
    draw_rounded_rect(draw, (W - bw - 16, cy + 8, W - 16, cy + 32), 6, fill=BUTTON_BG)
    draw.text((W - bw - 8, cy + 12), btn_text, fill=TEXT, font=bf)
    cy += banner_h

    # ── Split view: sidebar + session list ──
    sidebar_w = 220

    # Sidebar
    draw.rectangle([0, cy, sidebar_w, H], fill=CARD_BG)
    draw.line([(sidebar_w, cy), (sidebar_w, H)], fill=SEPARATOR)

    # Sidebar header "Filters"
    draw.text((16, cy + 12), "Filters", fill=TEXT, font=font(14))
    cy_sb = cy + 44

    # Sidebar: Assistants section
    draw.text((16, cy_sb), "ASSISTANTS", fill=TEXT_VERY_DIM, font=font(10))
    cy_sb += 22

    assistants = [
        ("Claude Code", "182", "ok"),
        ("OpenCode", "38", "warn"),
        ("Codex", "0", "none"),
        ("Mistral Vibe", "6", "ok"),
    ]
    for name, count, status in assistants:
        draw_status_icon(draw, 20, cy_sb + 4, status, 10)
        draw.text((38, cy_sb), name, fill=TEXT, font=font(13))
        draw.text((sidebar_w - 40, cy_sb), count, fill=TEXT_DIM, font=font(13))
        cy_sb += 30

    # Session list (right side)
    list_x = sidebar_w + 1
    list_cy = cy + 8

    sessions = [
        ("Fix login redirect bug", "Claude Code", "3 hours ago", "ok"),
        ("Refactor database queries", "OpenCode", "5 hours ago", "ok"),
        ("Add user settings page", "Claude Code", "Yesterday", "ok"),
        ("Debug API timeout", "Mistral Vibe", "Yesterday", "ok"),
        ("Setup CI pipeline", "Claude Code", "2 days ago", "ok"),
    ]

    for title, asst, time, status in sessions:
        # Session row
        draw.text((list_x + 16, list_cy + 8), title, fill=TEXT, font=font(14))
        draw.text((list_x + 16, list_cy + 28), f"{asst}  ·  {time}", fill=TEXT_DIM, font=font(12))
        list_cy += 52
        draw_separator(draw, list_x + 16, W - 16, list_cy)
        list_cy += 1

    # Label
    draw.text((16, H - 24), "Proposal C: GNOME HIG -- AdwBanner + Inline Sidebar Status", fill=TEXT_DIM, font=font(11))

    img.save(os.path.join(OUTPUT_DIR, "proposal-c-banner.png"))
    print("Generated proposal-c-banner.png")


def generate_proposal_c_empty():
    """Empty state with source diagnostic info."""
    W, H = 800, 500
    img = Image.new('RGB', (W, H), BG)
    draw = ImageDraw.Draw(img)

    draw_rounded_rect(draw, (0, 0, W, H), 12, fill=BG, outline=CARD_BORDER)

    # Header
    draw_rounded_rect(draw, (0, 0, W, 46), 12, fill=HEADER_BG)
    draw.rectangle([0, 24, W, 46], fill=HEADER_BG)
    draw.text((W//2 - 40, 14), "Sessions", fill=TAB_ACTIVE, font=font(14))

    cy = 46

    # Error banner
    banner_h = 40
    draw.rectangle([0, cy, W, cy + banner_h], fill=BANNER_ERROR_BG)
    draw.text((16, cy + 12), "!", fill=ERROR, font=font(16))
    draw.text((40, cy + 12), "No AI assistant sessions found on this system", fill=TEXT, font=font(13))
    cy += banner_h

    # Status page (centered)
    center_y = cy + 60

    # Icon placeholder
    icon_size = 64
    ix = (W - icon_size) // 2
    draw.ellipse([ix, center_y, ix + icon_size, center_y + icon_size], fill=CARD_BG)
    draw.text((ix + 20, center_y + 18), "?", fill=TEXT_DIM, font=font(28))
    center_y += icon_size + 20

    title = "No Sessions Yet"
    bbox = draw.textbbox((0, 0), title, font=font(24))
    tw = bbox[2] - bbox[0]
    draw.text(((W - tw) // 2, center_y), title, fill=TEXT, font=font(24))
    center_y += 36

    desc = "Install Claude Code, OpenCode, Codex, or Mistral Vibe to get started"
    bbox = draw.textbbox((0, 0), desc, font=font(13))
    dw = bbox[2] - bbox[0]
    draw.text(((W - dw) // 2, center_y), desc, fill=TEXT_DIM, font=font(13))
    center_y += 40

    # Source status summary (compact, inline in status page)
    sources_block_w = 320
    sbx = (W - sources_block_w) // 2

    source_info = [
        ("Claude Code", "~/.claude/projects", "not found"),
        ("OpenCode", "~/.local/share/opencode", "not found"),
        ("Codex", "~/.codex/sessions", "not found"),
        ("Mistral Vibe", "~/.vibe/logs/session", "not found"),
    ]

    for name, path, status in source_info:
        draw_status_icon(draw, sbx, center_y + 4, "none", 10)
        draw.text((sbx + 18, center_y), name, fill=TEXT_DIM, font=font(12))
        draw.text((sbx + sources_block_w - 60, center_y), status, fill=TEXT_VERY_DIM, font=font(11))
        center_y += 24

    draw.text((16, H - 24), "Proposal C: Empty State with Source Detection Info", fill=TEXT_DIM, font=font(11))

    img.save(os.path.join(OUTPUT_DIR, "proposal-c-empty.png"))
    print("Generated proposal-c-empty.png")


# ════════════════════════════════════════════════════════════════════════
# PROPOSAL D: Creative - Source Health Dashboard Tab
# ════════════════════════════════════════════════════════════════════════

def generate_proposal_d():
    W, H = 800, 620
    img = Image.new('RGB', (W, H), BG)
    draw = ImageDraw.Draw(img)

    draw_rounded_rect(draw, (0, 0, W, H), 12, fill=BG, outline=CARD_BORDER)

    # Header bar with 3 tabs
    draw_rounded_rect(draw, (0, 0, W, 46), 12, fill=HEADER_BG)
    draw.rectangle([0, 24, W, 46], fill=HEADER_BG)

    tabs = [("Sessions", False), ("Analytics", False), ("Sources", True)]
    tx = W // 2 - 130
    for label, active in tabs:
        c = TAB_ACTIVE if active else TAB_INACTIVE
        draw.text((tx, 14), label, fill=c, font=font(14))
        if active:
            bbox = draw.textbbox((0, 0), label, font=font(14))
            tw = bbox[2] - bbox[0]
            draw.line([(tx, 42), (tx + tw, 42)], fill=ACCENT, width=3)
        tx += 100

    cy = 46 + 16
    mx = 40

    # ── Summary bar ──
    # Big numbers across the top
    metrics = [
        ("226", "Sessions", TEXT),
        ("4", "Sources", TEXT),
        ("3", "Errors", WARNING),
        ("0.8s", "Duration", TEXT_DIM),
    ]

    metric_w = (W - 2 * mx) // len(metrics)
    for i, (val, label, color) in enumerate(metrics):
        bx = mx + i * metric_w
        draw.text((bx + metric_w // 2 - 15, cy), val, fill=color, font=font(28))
        bbox = draw.textbbox((0, 0), label, font=font(12))
        lw = bbox[2] - bbox[0]
        draw.text((bx + metric_w // 2 - lw // 2 + 5, cy + 36), label, fill=TEXT_DIM, font=font(12))

    cy += 70
    draw_separator(draw, mx, W - mx, cy)
    cy += 16

    # ── Source Cards (2x2 grid) ──
    card_w = (W - 2 * mx - 16) // 2
    card_h = 180

    sources = [
        ("Claude Code", "~/.claude/projects", 182, 14, 0, "ok", HEALTH_GOOD),
        ("OpenCode", "~/.local/share/opencode", 38, 5, 3, "warn", HEALTH_WARN),
        ("Codex", "~/.codex/sessions", 0, 0, 0, "none", HEALTH_NONE),
        ("Mistral Vibe", "~/.vibe/logs/session", 6, 2, 0, "ok", HEALTH_GOOD),
    ]

    for idx, (name, path, indexed, skipped, errors, status, health_color) in enumerate(sources):
        col = idx % 2
        row = idx // 2
        cx = mx + col * (card_w + 16)
        card_y = cy + row * (card_h + 12)

        draw_rounded_rect(draw, (cx, card_y, cx + card_w, card_y + card_h), 12, fill=CARD_BG)

        # Health indicator bar at top of card
        draw_rounded_rect(draw, (cx, card_y, cx + card_w, card_y + 4), 2, fill=health_color)

        # Source name + status dot
        draw_status_icon(draw, cx + 16, card_y + 20, status, 12)
        draw.text((cx + 36, card_y + 16), name, fill=TEXT, font=font(15))

        # Path
        draw.text((cx + 16, card_y + 42), path, fill=TEXT_DIM, font=mono_font(10))

        if status == "none":
            # Not found state
            draw.text((cx + 16, card_y + 80), "Directory not found", fill=TEXT_VERY_DIM, font=font(14))
            draw.text((cx + 16, card_y + 105), "This assistant is not", fill=TEXT_VERY_DIM, font=font(12))
            draw.text((cx + 16, card_y + 122), "installed on this system", fill=TEXT_VERY_DIM, font=font(12))
        else:
            # Stats
            draw_separator(draw, cx + 16, cx + card_w - 16, card_y + 62)

            stats = [
                ("Indexed", str(indexed)),
                ("Skipped", str(skipped)),
                ("Errors", str(errors)),
            ]
            stat_y = card_y + 74
            for slabel, sval in stats:
                draw.text((cx + 16, stat_y), slabel, fill=TEXT_DIM, font=font(12))
                vf = font(14)
                bbox = draw.textbbox((0, 0), sval, font=vf)
                vw = bbox[2] - bbox[0]
                vc = ERROR if slabel == "Errors" and int(sval) > 0 else TEXT
                draw.text((cx + card_w - 20 - vw, stat_y), sval, fill=vc, font=vf)
                stat_y += 28

            # Mini progress bar
            bar_y = card_y + card_h - 20
            total = indexed + skipped + errors if (indexed + skipped + errors) > 0 else 1
            bar_full = card_w - 32
            draw_rounded_rect(draw, (cx + 16, bar_y, cx + 16 + bar_full, bar_y + 4), 2, fill=PROGRESS_BG)
            if indexed > 0:
                ok_w = int(bar_full * indexed / total)
                draw_rounded_rect(draw, (cx + 16, bar_y, cx + 16 + ok_w, bar_y + 4), 2, fill=SUCCESS)
            if errors > 0:
                err_start = cx + 16 + int(bar_full * (indexed + skipped) / total)
                err_w = int(bar_full * errors / total)
                draw_rounded_rect(draw, (err_start, bar_y, err_start + max(err_w, 4), bar_y + 4), 2, fill=ERROR)

    # Label
    draw.text((16, H - 24), "Proposal D: Source Health Dashboard Tab (Creative)", fill=TEXT_DIM, font=font(11))

    # Last indexed
    draw.text((W - 200, H - 24), "Last indexed: 2 minutes ago", fill=TEXT_VERY_DIM, font=font(11))

    img.save(os.path.join(OUTPUT_DIR, "proposal-d-dashboard.png"))
    print("Generated proposal-d-dashboard.png")


# ════════════════════════════════════════════════════════════════════════
# PROPOSAL E: Creative - Live Log Bottom Panel
# ════════════════════════════════════════════════════════════════════════

def generate_proposal_e():
    W, H = 800, 650
    img = Image.new('RGB', (W, H), BG)
    draw = ImageDraw.Draw(img)

    draw_rounded_rect(draw, (0, 0, W, H), 12, fill=BG, outline=CARD_BORDER)

    # Header bar
    draw_rounded_rect(draw, (0, 0, W, 46), 12, fill=HEADER_BG)
    draw.rectangle([0, 24, W, 46], fill=HEADER_BG)

    tabs = [("Sessions", True), ("Analytics", False)]
    tx = W // 2 - 80
    for label, active in tabs:
        c = TAB_ACTIVE if active else TAB_INACTIVE
        draw.text((tx, 14), label, fill=c, font=font(14))
        if active:
            bbox = draw.textbbox((0, 0), label, font=font(14))
            tw = bbox[2] - bbox[0]
            draw.line([(tx, 42), (tx + tw, 42)], fill=ACCENT, width=3)
        tx += 100

    cy = 46

    # ── Main content area (session list, abbreviated) ──
    panel_split = H - 220  # Bottom panel starts here

    # Session list rows
    list_cy = cy + 8
    sessions = [
        ("Fix login redirect bug", "Claude Code  ·  3 hours ago"),
        ("Refactor database queries", "OpenCode  ·  5 hours ago"),
        ("Add user settings page", "Claude Code  ·  Yesterday"),
        ("Debug API timeout", "Mistral Vibe  ·  Yesterday"),
    ]
    for title, sub in sessions:
        draw.text((24, list_cy + 8), title, fill=TEXT, font=font(14))
        draw.text((24, list_cy + 28), sub, fill=TEXT_DIM, font=font(12))
        list_cy += 52
        draw_separator(draw, 24, W - 24, list_cy)
        list_cy += 1

    # ── Bottom Panel (slide-up) ──
    # Resize handle
    draw.rectangle([0, panel_split - 2, W, panel_split], fill=SEPARATOR)
    draw.rectangle([(W//2 - 20, panel_split - 6), (W//2 + 20, panel_split - 2)], fill=TEXT_VERY_DIM)

    # Panel header
    panel_header_y = panel_split
    draw.rectangle([0, panel_header_y, W, panel_header_y + 36], fill=BOTTOM_PANEL_BG)

    # Tab bar for panel
    panel_tabs = [("Indexing Log", True), ("Errors (3)", False)]
    ptx = 16
    for label, active in panel_tabs:
        c = TAB_ACTIVE if active else TAB_INACTIVE
        draw.text((ptx, panel_header_y + 10), label, fill=c, font=font(13))
        if active:
            bbox = draw.textbbox((0, 0), label, font=font(13))
            tw = bbox[2] - bbox[0]
            draw.line([(ptx, panel_header_y + 33), (ptx + tw, panel_header_y + 33)], fill=ACCENT, width=2)
        ptx += 120

    # Close button for panel
    draw.text((W - 32, panel_header_y + 10), "x", fill=TEXT_DIM, font=font(14))
    # Minimize button
    draw.text((W - 60, panel_header_y + 10), "_", fill=TEXT_DIM, font=font(14))

    draw_separator(draw, 0, W, panel_header_y + 36)

    # Log content (monospace, terminal-like)
    log_y = panel_header_y + 40
    draw.rectangle([0, log_y, W, H], fill=BOTTOM_PANEL_BG)

    log_entries = [
        ("[14:32:01]", "INFO ", "Starting incremental indexing...", TEXT_DIM, ACCENT, TEXT),
        ("[14:32:01]", "INFO ", "Claude Code: scanning ~/.claude/projects", TEXT_DIM, ACCENT, TEXT),
        ("[14:32:02]", "INFO ", "Claude Code: indexed 12 new, skipped 170 unchanged", TEXT_DIM, ACCENT, SUCCESS),
        ("[14:32:02]", "INFO ", "OpenCode: scanning ~/.local/share/opencode/storage", TEXT_DIM, ACCENT, TEXT),
        ("[14:32:02]", "WARN ", "OpenCode: session_abc.json -- Unexpected EOF at line 42", TEXT_DIM, WARNING, WARNING),
        ("[14:32:03]", "WARN ", "OpenCode: session_def.json -- Invalid timestamp", TEXT_DIM, WARNING, WARNING),
        ("[14:32:03]", "INFO ", "OpenCode: indexed 3 new, 3 errors, skipped 32 unchanged", TEXT_DIM, ACCENT, TEXT),
        ("[14:32:03]", "ERROR", "Codex: directory ~/.codex/sessions not found", TEXT_DIM, ERROR, ERROR),
        ("[14:32:03]", "INFO ", "Mistral Vibe: indexed 1 new, skipped 5 unchanged", TEXT_DIM, ACCENT, SUCCESS),
        ("[14:32:03]", "INFO ", "Indexing complete: 226 sessions total (0.8s)", TEXT_DIM, ACCENT, TEXT),
    ]

    mf = mono_font(11)
    for i, (ts, level, msg, ts_color, level_color, msg_color) in enumerate(log_entries):
        ly = log_y + 4 + i * 16
        if ly > H - 20:
            break
        draw.text((12, ly), ts, fill=ts_color, font=mf)
        draw.text((100, ly), level, fill=level_color, font=mf)
        draw.text((140, ly), msg, fill=msg_color, font=mf)

    # Label
    draw.text((16, H - 18), "Proposal E: Live Indexing Log Panel (Creative)", fill=TEXT_VERY_DIM, font=font(11))

    img.save(os.path.join(OUTPUT_DIR, "proposal-e-log-panel.png"))
    print("Generated proposal-e-log-panel.png")


# ════════════════════════════════════════════════════════════════════════
# Main
# ════════════════════════════════════════════════════════════════════════

if __name__ == "__main__":
    generate_proposal_a()
    generate_proposal_a_initial()
    generate_proposal_b()
    generate_proposal_b_progress()
    generate_proposal_c()
    generate_proposal_c_empty()
    generate_proposal_d()
    generate_proposal_e()
    print("\nAll mockups generated!")
