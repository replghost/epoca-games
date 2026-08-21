#include "of.h"
#include "d3d_audio.h"
#include <dirent.h>
#include <stdint.h>
#include <string.h>

extern void pvm_set_palette_wrapper(uint32_t ptr);
extern void pvm_display_wrapper(uint32_t width, uint32_t height, uint32_t ptr);
extern uint32_t pvm_fetch_epoca_inputs_wrapper(uint32_t ptr, uint32_t capacity);
extern uint64_t pvm_time_ms_wrapper(void);

#define WIDTH 320
#define HEIGHT 200
#define EVENT_BYTES 8
#define MAX_EVENTS 64

static uint8_t framebuffer[WIDTH * HEIGHT];
static uint8_t palette[256 * 3];
static of_video_mode_t video_mode = {
    WIDTH, HEIGHT, WIDTH, OF_VIDEO_MODE_8BIT, 0
};
static of_keyboard_state_t keyboard;
static of_mouse_state_t mouse;
static of_input_state_t controller;
static uint16_t pointer_x;
static uint16_t pointer_y;
static int pointer_initialized;

void of_video_init(void) {}
uint8_t *of_video_surface(void) { return framebuffer; }
void of_video_flip(void) {
    pvm_display_wrapper(WIDTH, HEIGHT, (uint32_t)(uintptr_t)framebuffer);
}
void of_video_wait_flip(void) {}
void of_video_clear(uint8_t color) { memset(framebuffer, color, sizeof(framebuffer)); }
void of_video_palette(uint8_t index, uint32_t rgb) {
    palette[index * 3] = (uint8_t)(rgb >> 16);
    palette[index * 3 + 1] = (uint8_t)(rgb >> 8);
    palette[index * 3 + 2] = (uint8_t)rgb;
    pvm_set_palette_wrapper((uint32_t)(uintptr_t)palette);
}
void of_video_palette_bulk(const uint32_t *colors, int count) {
    if (count > 256) count = 256;
    for (int index = 0; index < count; ++index) {
        uint32_t rgb = colors[index];
        palette[index * 3] = (uint8_t)(rgb >> 16);
        palette[index * 3 + 1] = (uint8_t)(rgb >> 8);
        palette[index * 3 + 2] = (uint8_t)rgb;
    }
    pvm_set_palette_wrapper((uint32_t)(uintptr_t)palette);
}
void of_video_flush(void) {}
void of_video_set_display_mode(int mode) { (void)mode; }
void of_video_set_color_mode(int mode) { (void)mode; }
int of_video_set_mode(const of_video_mode_t *mode) {
    if (!mode || mode->width != WIDTH || mode->height != HEIGHT ||
        mode->color_mode != OF_VIDEO_MODE_8BIT) return -1;
    video_mode = *mode;
    return 0;
}
void of_video_get_mode(of_video_mode_t *out) { if (out) *out = video_mode; }
int of_video_acquire_next(int previous, uint32_t fence) {
    (void)previous;
    (void)fence;
    return 0;
}
uint8_t *of_video_buffer_addr(int index) { (void)index; return framebuffer; }

static void set_keyboard_usage(uint8_t usage, int down) {
    if (usage >= 0xe0) {
        uint16_t bit = (uint16_t)1u << (usage - 0xe0);
        if (down) {
            keyboard.modifiers |= bit;
            keyboard.modifiers_pressed |= bit;
        } else {
            keyboard.modifiers &= (uint16_t)~bit;
            keyboard.modifiers_released |= bit;
        }
        return;
    }
    uint32_t bit = 1u << (usage & 31);
    uint32_t *held = &keyboard.keys[usage >> 5];
    uint32_t *changed = down ? &keyboard.keys_pressed[usage >> 5]
                             : &keyboard.keys_released[usage >> 5];
    if (down) *held |= bit; else *held &= ~bit;
    *changed |= bit;
}

void of_input_poll(void) {
    uint8_t events[MAX_EVENTS * EVENT_BYTES];
    memset(keyboard.keys_pressed, 0, sizeof(keyboard.keys_pressed));
    memset(keyboard.keys_released, 0, sizeof(keyboard.keys_released));
    keyboard.modifiers_pressed = keyboard.modifiers_released = 0;
    mouse.buttons_pressed = mouse.buttons_released = 0;
    mouse.dx = mouse.dy = 0;
    uint32_t count = pvm_fetch_epoca_inputs_wrapper(
        (uint32_t)(uintptr_t)events, MAX_EVENTS);
    if (count > MAX_EVENTS) count = MAX_EVENTS;
    for (uint32_t index = 0; index < count; ++index) {
        uint8_t *event = events + index * EVENT_BYTES;
        uint8_t kind = event[0];
        uint8_t code = event[1];
        uint16_t x = (uint16_t)event[2] | ((uint16_t)event[3] << 8);
        uint16_t y = (uint16_t)event[4] | ((uint16_t)event[5] << 8);
        if (kind == 1 || kind == 2) {
            set_keyboard_usage(code, kind == 1);
            keyboard.present = 1;
        } else if (kind == 3 || kind == 4) {
            uint16_t bit = code && code <= 16 ? (uint16_t)1u << (code - 1) : 0;
            if (kind == 3) {
                mouse.buttons |= bit;
                mouse.buttons_pressed |= bit;
            } else {
                mouse.buttons &= (uint16_t)~bit;
                mouse.buttons_released |= bit;
            }
            mouse.present = 1;
        } else if (kind == 5) {
            if (pointer_initialized) {
                mouse.dx += (int16_t)(x - pointer_x);
                mouse.dy += (int16_t)(y - pointer_y);
            }
            pointer_x = x;
            pointer_y = y;
            pointer_initialized = 1;
            mouse.present = 1;
        } else if (kind == 6) {
            mouse.dx += (int16_t)x;
            mouse.dy += (int16_t)y;
            mouse.present = 1;
        }
    }
}
void of_input_poll_p0(void) { of_input_poll(); }
int of_btn(uint32_t mask) { return (controller.buttons & mask) != 0; }
int of_btn_pressed(uint32_t mask) { return (controller.buttons_pressed & mask) != 0; }
int of_btn_released(uint32_t mask) { return (controller.buttons_released & mask) != 0; }
int of_btn_p2(uint32_t mask) { (void)mask; return 0; }
int of_btn_pressed_p2(uint32_t mask) { (void)mask; return 0; }
int of_btn_released_p2(uint32_t mask) { (void)mask; return 0; }
uint32_t of_input_state(int player, of_input_state_t *state) {
    (void)player;
    if (state) *state = controller;
    return controller.buttons;
}
void of_input_keyboard_state(of_keyboard_state_t *state) { if (state) *state = keyboard; }
void of_input_mouse_state(of_mouse_state_t *state) { if (state) *state = mouse; }
int of_input_is_docked(void) { return 1; }
void of_input_set_deadzone(int16_t deadzone) { (void)deadzone; }

unsigned int of_time_ms(void) { return (unsigned int)pvm_time_ms_wrapper(); }
unsigned int of_time_us(void) { return of_time_ms() * 1000u; }

static const struct of_capabilities capabilities;
const struct of_capabilities *of_get_caps(void) { return &capabilities; }

void of_file_slot_register(uint32_t slot, const char *name) { (void)slot; (void)name; }
void *of_file_dma_stage_alloc(uint32_t size, uint32_t align) { (void)size; (void)align; return 0; }
uint32_t of_file_async_max_read(void) { return 0; }
uint32_t of_file_dma_stage_size(void) { return 0; }
int of_file_read_async(int slot, uint32_t offset, void *dest, uint32_t length,
                       void (*callback)(int, int)) {
    (void)slot; (void)offset; (void)dest; (void)length; (void)callback; return -1;
}
int of_file_async_poll(void) { return 0; }
int of_file_async_busy(void) { return 0; }

int of_midi_init(void) { return -1; }
int of_midi_play(const uint8_t *data, uint32_t length, int loop) {
    (void)data; (void)length; (void)loop; return -1;
}
void of_midi_stop(void) {}
void of_midi_pause(void) {}
void of_midi_resume(void) {}
int of_midi_playing(void) { return 0; }
void of_midi_set_volume(int volume) { (void)volume; }
int SDL_SetRelativeMouseMode(int enabled) { (void)enabled; return 0; }
int of_has_feature(uint32_t feature) { (void)feature; return 0; }


void d3d_audio_init(void) {}
void d3d_sound_set_fx_volume(int volume) { (void)volume; }
int d3d_sound_precache(int sound_num) { (void)sound_num; return 0; }
int d3d_sound_precache_all(void) { return 0; }
int d3d_sound_play(int sound_num, int priority, int volume) {
    (void)sound_num; (void)priority; (void)volume; return -1;
}
int d3d_sound_play_3d(int sound_num, int priority, int angle, int distance) {
    (void)sound_num; (void)priority; (void)angle; (void)distance; return -1;
}
int d3d_sound_play_pitch(int sound_num, int priority, int volume, int pitch) {
    (void)sound_num; (void)priority; (void)volume; (void)pitch; return -1;
}
int d3d_sound_play_3d_pitch(int sound_num, int priority, int angle, int distance, int pitch) {
    (void)sound_num; (void)priority; (void)angle; (void)distance; (void)pitch; return -1;
}
void d3d_sound_stop(int handle) { (void)handle; }
void d3d_sound_stop_all(void) {}
void d3d_sound_set_volume(int handle, int volume) { (void)handle; (void)volume; }
void d3d_sound_set_pan(int handle, int angle, int distance) {
    (void)handle; (void)angle; (void)distance;
}
void d3d_sound_set_loop(int handle) { (void)handle; }
void d3d_sound_set_owned(int handle) { (void)handle; }
void d3d_audio_pump(void) {}
void d3d_audio_pump_loading(void) { pvm_yield_wrapper(); }
void d3d_audio_shutdown(void) {}

DIR *opendir(const char *path) { (void)path; return 0; }
struct dirent *readdir(DIR *directory) { (void)directory; return 0; }

int getchar(void) { return -1; }
int of_file_slot_find(const char *name, uint32_t *slot) {
    (void)name;
    (void)slot;
    return -1;
}
int closedir(DIR *directory) { (void)directory; return -1; }
