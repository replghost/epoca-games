#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "duke3d.h"
#include "filesystem.h"
#include "pitch.h"
#include "d3d_audio.h"

#define OUTPUT_RATE 48000u
#define MIX_FRAMES 960u
#define MAX_VOICES 16

typedef struct {
    uint8_t *samples;
    uint32_t count;
    uint32_t rate;
} sound_t;

typedef struct {
    int active;
    int handle;
    int sound_num;
    int priority;
    int owned;
    int looping;
    uint8_t left;
    uint8_t right;
    uint64_t position;
    uint64_t step;
} voice_t;

static sound_t cache[NUM_SOUNDS];
static voice_t voices[MAX_VOICES];
static int16_t output[MIX_FRAMES * 2];
static int initialized;
static int master_volume = 220;
static int next_handle = 1;
static uint64_t next_audio_ms;

extern uint32_t host_audio_submit_wrapper(uint32_t ptr, uint32_t sample_count);
extern uint64_t pvm_time_ms_wrapper(void);
extern void pvm_yield_wrapper(void);
extern void testcallback(int32_t num);

static uint16_t rd16(const uint8_t *p)
{
    return (uint16_t)p[0] | ((uint16_t)p[1] << 8);
}

static int decode(int num)
{
    if (num < 0 || num >= NUM_SOUNDS || !sounds[num][0])
        return 0;
    if (cache[num].samples)
        return 1;

    short file = TCkopen4load(sounds[num], 0);
    if (file < 0)
        return 0;
    int32_t size = kfilelength(file);
    if (size < 0x1a) {
        kclose(file);
        return 0;
    }
    uint8_t *raw = (uint8_t *)malloc((uint32_t)size);
    if (!raw) {
        kclose(file);
        return 0;
    }
    int ok = kread(file, raw, size) == size;
    kclose(file);
    if (!ok || raw[0] != 'C' || raw[1] != 'r') {
        free(raw);
        return 0;
    }

    uint32_t offset = rd16(raw + 0x14);
    while (offset + 6u <= (uint32_t)size) {
        uint8_t type = raw[offset];
        uint32_t length = (uint32_t)raw[offset + 1]
                        | ((uint32_t)raw[offset + 2] << 8)
                        | ((uint32_t)raw[offset + 3] << 16);
        offset += 4;
        if (type == 0 || offset + length > (uint32_t)size)
            break;
        if (type == 1 && length >= 2 && raw[offset + 1] == 0) {
            cache[num].samples = raw + offset + 2;
            cache[num].count = length - 2;
            cache[num].rate = 1000000u / (256u - raw[offset]);
            soundsiz[num] = size;
            Sound[num].lock = 199;
            return 1;
        }
        offset += length;
    }
    free(raw);
    return 0;
}

static int clamp(int value)
{
    if (value < 0) return 0;
    if (value > 255) return 255;
    return value;
}

static void pan(int angle, int distance, int *left, int *right)
{
    if (distance < 0) {
        distance = -distance;
        angle += 1024;
    }
    int level = 255 - clamp(distance);
    int position = (angle >> 6) & 31;
    *left = level;
    *right = level;
    if (position > 0 && position < 16) {
        int amount = position < 16 - position ? position : 16 - position;
        *left -= (*left * amount) / 8;
    } else if (position > 16) {
        int amount = position - 16 < 32 - position ? position - 16 : 32 - position;
        *right -= (*right * amount) / 8;
    }
}

static voice_t *find_voice(int handle)
{
    for (int i = 0; i < MAX_VOICES; i++)
        if (voices[i].active && voices[i].handle == handle)
            return &voices[i];
    return NULL;
}

static void finish(voice_t *voice, int notify)
{
    int sound_num = voice->sound_num;
    int owned = voice->owned;
    voice->active = 0;
    if (notify && owned)
        testcallback(sound_num);
}

static voice_t *allocate(int priority)
{
    voice_t *candidate = NULL;
    for (int i = 0; i < MAX_VOICES; i++) {
        if (!voices[i].active)
            return &voices[i];
        if (voices[i].priority <= priority
            && (!candidate || voices[i].priority < candidate->priority
                || (candidate->looping && !voices[i].looping)))
            candidate = &voices[i];
    }
    if (candidate)
        finish(candidate, 1);
    return candidate;
}

static int play(int num, int priority, int left, int right, int pitch)
{
    if (!initialized || !decode(num))
        return -1;
    voice_t *voice = allocate(priority);
    if (!voice)
        return -1;

    memset(voice, 0, sizeof(*voice));
    voice->active = 1;
    voice->handle = next_handle++;
    if (next_handle <= 0) next_handle = 1;
    voice->sound_num = num;
    voice->priority = priority;
    voice->left = (uint8_t)clamp(left);
    voice->right = (uint8_t)clamp(right);
    uint64_t rate = ((uint64_t)cache[num].rate * PITCH_GetScale(pitch) + 0x8000u) >> 16;
    voice->step = (rate << 32) / OUTPUT_RATE;
    if (!voice->step) voice->step = 1;
    next_audio_ms = pvm_time_ms_wrapper();
    return voice->handle;
}

void d3d_audio_init(void)
{
    memset(cache, 0, sizeof(cache));
    memset(voices, 0, sizeof(voices));
    initialized = 1;
    next_audio_ms = pvm_time_ms_wrapper();
}

void d3d_sound_set_fx_volume(int volume)
{
    master_volume = clamp(volume);
}

int d3d_sound_precache(int num)
{
    return initialized && decode(num);
}

int d3d_sound_precache_all(void)
{
    pvm_yield_wrapper();
    return 0;
}

int d3d_sound_play(int num, int priority, int volume)
{
    return play(num, priority, volume, volume, 0);
}

int d3d_sound_play_pitch(int num, int priority, int volume, int pitch)
{
    return play(num, priority, volume, volume, pitch);
}

int d3d_sound_play_3d(int num, int priority, int angle, int distance)
{
    return d3d_sound_play_3d_pitch(num, priority, angle, distance, 0);
}

int d3d_sound_play_3d_pitch(int num, int priority, int angle, int distance, int pitch)
{
    int left, right;
    pan(angle, distance, &left, &right);
    return play(num, priority, left, right, pitch);
}

void d3d_sound_stop(int handle)
{
    voice_t *voice = find_voice(handle);
    if (voice)
        finish(voice, 0);
}

void d3d_sound_stop_all(void)
{
    for (int i = 0; i < MAX_VOICES; i++)
        if (voices[i].active)
            finish(&voices[i], 1);
}

void d3d_sound_set_volume(int handle, int volume)
{
    voice_t *voice = find_voice(handle);
    if (voice)
        voice->left = voice->right = (uint8_t)clamp(volume);
}

void d3d_sound_set_pan(int handle, int angle, int distance)
{
    voice_t *voice = find_voice(handle);
    if (!voice)
        return;
    int left, right;
    pan(angle, distance, &left, &right);
    voice->left = (uint8_t)left;
    voice->right = (uint8_t)right;
}

void d3d_sound_set_loop(int handle)
{
    voice_t *voice = find_voice(handle);
    if (voice) voice->looping = 1;
}

void d3d_sound_set_owned(int handle)
{
    voice_t *voice = find_voice(handle);
    if (voice) voice->owned = 1;
}

void d3d_audio_pump(void)
{
    if (!initialized)
        return;
    int active = 0;
    for (int i = 0; i < MAX_VOICES; i++) active |= voices[i].active;
    uint64_t now = pvm_time_ms_wrapper();
    if (!active) {
        next_audio_ms = now;
        return;
    }
    if (now < next_audio_ms)
        return;
    if (now > next_audio_ms + 100)
        next_audio_ms = now;
    next_audio_ms += 20;

    memset(output, 0, sizeof(output));
    for (uint32_t frame = 0; frame < MIX_FRAMES; frame++) {
        int32_t left = 0;
        int32_t right = 0;
        for (int i = 0; i < MAX_VOICES; i++) {
            voice_t *voice = &voices[i];
            if (!voice->active)
                continue;
            sound_t *sound = &cache[voice->sound_num];
            uint32_t index = (uint32_t)(voice->position >> 32);
            if (index >= sound->count) {
                if (!voice->looping) {
                    finish(voice, 1);
                    continue;
                }
                voice->position %= (uint64_t)sound->count << 32;
                index = (uint32_t)(voice->position >> 32);
            }
            int32_t sample = ((int32_t)sound->samples[index] - 128) * 256;
            left += sample * voice->left * master_volume / 65025;
            right += sample * voice->right * master_volume / 65025;
            voice->position += voice->step;
        }
        if (left < -32768) left = -32768;
        if (left > 32767) left = 32767;
        if (right < -32768) right = -32768;
        if (right > 32767) right = 32767;
        output[frame * 2] = (int16_t)left;
        output[frame * 2 + 1] = (int16_t)right;
    }
    host_audio_submit_wrapper((uint32_t)(uintptr_t)output, MIX_FRAMES * 2);
}

void d3d_audio_pump_loading(void)
{
    pvm_yield_wrapper();
}

void d3d_audio_shutdown(void)
{
    d3d_sound_stop_all();
    initialized = 0;
}
