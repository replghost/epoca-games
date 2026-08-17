/*
 * doomgeneric_polkavm.c — Platform layer for doomgeneric running on PolkaVM.
 *
 * Implements Doomgeneric's platform callbacks using the version-1 host ABI.
 */

#include "doomgeneric.h"
#include "doomkeys.h"

#include <stdint.h>
#include <string.h>

/* ── Host function imports (linked via Rust #[no_mangle] wrappers) ── */
extern uint32_t host_present_frame_wrapper(uint32_t ptr, uint32_t width, uint32_t height, uint32_t stride);
extern uint32_t host_poll_input_wrapper(uint32_t buf_ptr, uint32_t buf_len);
extern uint64_t host_time_ms_wrapper(void);
extern void host_sleep_ms_wrapper(uint32_t duration_ms);
extern void host_log_wrapper(uint32_t ptr, uint32_t len);

/* ── ABI v1 input decoding ──────────────────────────────────────── */

#define INPUT_EVENT_BYTES 8
#define INPUT_KIND_KEY_DOWN 1
#define INPUT_KIND_KEY_UP 2
#define INPUT_KIND_BUTTON_DOWN 3
#define INPUT_KIND_BUTTON_UP 4
#define INPUT_KIND_POINTER_MOVE 5
#define MAX_INPUT_EVENTS 32
static const char INPUT_LOG[] = "doom-input";

static unsigned char translate_key(uint8_t usage) {
    /* Modern FPS controls. Arrow keys remain available for turning and menus. */
    switch (usage) {
        case 0x04: return KEY_STRAFE_L;  /* A */
        case 0x07: return KEY_STRAFE_R;  /* D */
        case 0x16: return KEY_DOWNARROW; /* S */
        case 0x08: return KEY_USE;       /* E */
        case 0x1a: return KEY_UPARROW;   /* W */
    }

    if (usage >= 0x04 && usage <= 0x1d) {
        return (unsigned char)('a' + usage - 0x04);
    }
    if (usage >= 0x1e && usage <= 0x26) {
        return (unsigned char)('1' + usage - 0x1e);
    }

    switch (usage) {
        case 0x27: return '0';
        case 0x28: return KEY_ENTER;
        case 0x29: return KEY_ESCAPE;
        case 0x2a: return KEY_BACKSPACE;
        case 0x2b: return KEY_TAB;
        case 0x2c: return KEY_FIRE;      /* Space; Doom has no jump action. */
        case 0x2d: return KEY_MINUS;
        case 0x2e: return KEY_EQUALS;
        case 0x2f: return '[';
        case 0x30: return ']';
        case 0x31: return '\\';
        case 0x33: return ';';
        case 0x34: return '\'';
        case 0x35: return '`';
        case 0x36: return ',';
        case 0x37: return '.';
        case 0x38: return '/';
        case 0x39: return KEY_CAPSLOCK;
        case 0x3a: return KEY_F1;
        case 0x3b: return KEY_F2;
        case 0x3c: return KEY_F3;
        case 0x3d: return KEY_F4;
        case 0x3e: return KEY_F5;
        case 0x3f: return KEY_F6;
        case 0x40: return KEY_F7;
        case 0x41: return KEY_F8;
        case 0x42: return KEY_F9;
        case 0x43: return KEY_F10;
        case 0x44: return KEY_F11;
        case 0x45: return KEY_F12;
        case 0x46: return KEY_PRTSCR;
        case 0x47: return KEY_SCRLCK;
        case 0x48: return KEY_PAUSE;
        case 0x49: return KEY_INS;
        case 0x4a: return KEY_HOME;
        case 0x4b: return KEY_PGUP;
        case 0x4c: return KEY_DEL;
        case 0x4d: return KEY_END;
        case 0x4e: return KEY_PGDN;
        case 0x4f: return KEY_RIGHTARROW;
        case 0x50: return KEY_LEFTARROW;
        case 0x51: return KEY_DOWNARROW;
        case 0x52: return KEY_UPARROW;
        case 0x54: return KEYP_DIVIDE;
        case 0x55: return KEYP_MULTIPLY;
        case 0x56: return KEYP_MINUS;
        case 0x57: return KEYP_PLUS;
        case 0x58: return KEYP_ENTER;
        case 0x59: return KEYP_1;
        case 0x5a: return KEYP_2;
        case 0x5b: return KEYP_3;
        case 0x5c: return KEYP_4;
        case 0x5d: return KEYP_5;
        case 0x5e: return KEYP_6;
        case 0x5f: return KEYP_7;
        case 0x60: return KEYP_8;
        case 0x61: return KEYP_9;
        case 0x62: return KEYP_0;
        case 0x63: return KEYP_PERIOD;
        case 0xe0:
        case 0xe4: return KEY_FIRE;
        case 0xe1:
        case 0xe5: return KEY_RSHIFT;
        case 0xe2:
        case 0xe6: return KEY_RALT;
        default: return 0;
    }
}

typedef struct {
    int pressed;
    unsigned char key;
} key_event_t;

typedef struct {
    int buttons;
    int xrel;
    int yrel;
} mouse_event_t;

static key_event_t key_ring[MAX_INPUT_EVENTS];
static mouse_event_t mouse_ring[MAX_INPUT_EVENTS];
static int key_head;
static int key_count;
static int mouse_head;
static int mouse_count;
static int mouse_buttons;
static uint16_t pointer_x;
static uint16_t pointer_y;
static int pointer_initialized;

static void push_key(int pressed, unsigned char key) {
    int index;

    if (key_count >= MAX_INPUT_EVENTS) {
        return;
    }
    index = (key_head + key_count) % MAX_INPUT_EVENTS;
    key_ring[index].pressed = pressed;
    key_ring[index].key = key;
    ++key_count;
    host_log_wrapper(
        (uint32_t) (uintptr_t) INPUT_LOG,
        (uint32_t) (sizeof(INPUT_LOG) - 1)
    );
}

static void push_mouse(int xrel, int yrel) {
    int index;

    if (mouse_count >= MAX_INPUT_EVENTS) {
        return;
    }
    index = (mouse_head + mouse_count) % MAX_INPUT_EVENTS;
    mouse_ring[index].buttons = mouse_buttons;
    mouse_ring[index].xrel = xrel;
    mouse_ring[index].yrel = yrel;
    ++mouse_count;
}

static void poll_host_input(void) {
    uint8_t buffer[MAX_INPUT_EVENTS * INPUT_EVENT_BYTES];
    uint32_t bytes;
    uint32_t count;
    uint32_t i;

    bytes = host_poll_input_wrapper((uint32_t)(uintptr_t)buffer, sizeof(buffer));
    if (bytes > sizeof(buffer)) {
        bytes = sizeof(buffer);
    }
    count = bytes / INPUT_EVENT_BYTES;

    for (i = 0; i < count; ++i) {
        const uint8_t *event = &buffer[i * INPUT_EVENT_BYTES];
        uint8_t kind = event[0];
        uint8_t code = event[1];
        uint16_t x = (uint16_t)event[2] | ((uint16_t)event[3] << 8);
        uint16_t y = (uint16_t)event[4] | ((uint16_t)event[5] << 8);

        if (kind == INPUT_KIND_KEY_DOWN || kind == INPUT_KIND_KEY_UP) {
            unsigned char key = translate_key(code);
            if (key != 0) {
                push_key(kind == INPUT_KIND_KEY_DOWN, key);
            }
        } else if ((kind == INPUT_KIND_BUTTON_DOWN
                 || kind == INPUT_KIND_BUTTON_UP)
                && code >= 1 && code <= 3) {
            int button_mask = code == 1 ? 1 : (code == 2 ? 4 : 2);
            if (kind == INPUT_KIND_BUTTON_DOWN) {
                mouse_buttons |= button_mask;
            } else {
                mouse_buttons &= ~button_mask;
            }
            pointer_x = x;
            pointer_y = y;
            pointer_initialized = 1;
            push_mouse(0, 0);
            host_log_wrapper(
                (uint32_t) (uintptr_t) INPUT_LOG,
                (uint32_t) (sizeof(INPUT_LOG) - 1)
            );
        } else if (kind == INPUT_KIND_POINTER_MOVE) {
            int xrel = 0;
            int yrel = 0;
            if (pointer_initialized) {
                xrel = (int)x - (int)pointer_x;
                yrel = (int)y - (int)pointer_y;
            }
            pointer_x = x;
            pointer_y = y;
            pointer_initialized = 1;
            push_mouse(xrel, yrel);
        }
    }
}

/* ══════════════════════════════════════════════════════════════════
 * DG_* callbacks — required by doomgeneric
 * ══════════════════════════════════════════════════════════════════ */

void DG_Init(void) {
    /* Nothing to initialize — host is ready. */
}

void DG_DrawFrame(void) {
    /* DG_ScreenBuffer is XRGB (32-bit, X=unused, same as ARGB with A=0).
     * Our host expects ARGB with A=0xFF. Set alpha on each pixel. */
    uint32_t *pixels = (uint32_t *)DG_ScreenBuffer;
    int count = DOOMGENERIC_RESX * DOOMGENERIC_RESY;
    for (int i = 0; i < count; i++) {
        pixels[i] |= 0xFF000000;
    }
    host_present_frame_wrapper(
        (uint32_t)(uintptr_t)DG_ScreenBuffer,
        DOOMGENERIC_RESX,
        DOOMGENERIC_RESY,
        DOOMGENERIC_RESX * 4
    );
}

void DG_SleepMs(uint32_t ms) {
    host_sleep_ms_wrapper(ms);
}

uint32_t DG_GetTicksMs(void) {
    return (uint32_t)host_time_ms_wrapper();
}

int DG_GetKey(int *pressed, unsigned char *doom_key) {
    if (key_count == 0 && mouse_count == 0) {
        poll_host_input();
    }
    if (key_count == 0) {
        return 0;
    }

    *pressed = key_ring[key_head].pressed;
    *doom_key = key_ring[key_head].key;
    key_head = (key_head + 1) % MAX_INPUT_EVENTS;
    --key_count;
    return 1;
}

int DG_GetMouse(int *buttons, int *xrel, int *yrel) {
    if (key_count == 0 && mouse_count == 0) {
        poll_host_input();
    }
    if (mouse_count == 0) {
        return 0;
    }

    *buttons = mouse_ring[mouse_head].buttons;
    *xrel = mouse_ring[mouse_head].xrel;
    /* Classic Doom treats vertical mouse motion as forward/back movement.
     * Suppress it so pointer movement cannot leave the player walking. */
    *yrel = 0;
    mouse_head = (mouse_head + 1) % MAX_INPUT_EVENTS;
    --mouse_count;
    return 1;
}

void DG_SetWindowTitle(const char *title) {
    (void)title; /* Host manages the tab title. */
}

void DG_Close(void) {
    /* Nothing to clean up. */
}
