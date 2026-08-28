/*
 * PolkaVM sound backend. Doom's DMX sound effects are mixed directly from
 * cached WAD lumps into the fixed-size buffer consumed by the host ABI.
 */

#include "doomtype.h"
#include "i_sound.h"
#include "w_wad.h"
#include "z_zone.h"

#include <stddef.h>
#include <stdint.h>

#define OUTPUT_RATE 48000U
#define OUTPUT_FRAMES 800U
#define OUTPUT_CHANNELS 2U
#define OUTPUT_SAMPLES (OUTPUT_FRAMES * OUTPUT_CHANNELS)
#define MAX_MIX_CHANNELS 32
#define DMX_HEADER_BYTES 8U
#define DMX_PADDING_BYTES 32U

typedef struct
{
    const uint8_t *samples;
    uint32_t sample_count;
    uint32_t sample_rate;
    uint32_t position;
    uint32_t phase;
    int left_volume;
    int right_volume;
    boolean active;
} mixer_channel_t;

extern uint32_t host_audio_submit_wrapper(uint32_t ptr, uint32_t sample_count);

static mixer_channel_t channels[MAX_MIX_CHANNELS];
static int16_t output_buffer[OUTPUT_SAMPLES];
static boolean use_sfx_prefix;

int snd_samplerate = OUTPUT_RATE;
int snd_cachesize = 64 * 1024 * 1024;
int snd_maxslicetime_ms = 17;
char *snd_musiccmd = "";
int snd_musicdevice = SNDDEVICE_NONE;
int snd_sfxdevice = SNDDEVICE_SB;

static int Clamp(int value, int minimum, int maximum)
{
    if (value < minimum)
    {
        return minimum;
    }
    if (value > maximum)
    {
        return maximum;
    }
    return value;
}

static int16_t ClampSample(int32_t sample)
{
    if (sample > 32767)
    {
        return 32767;
    }
    if (sample < -32768)
    {
        return -32768;
    }
    return (int16_t) sample;
}

static void SetChannelParams(mixer_channel_t *channel, int volume, int separation)
{
    int left;
    int right;

    volume = Clamp(volume, 0, 127);
    separation = Clamp(separation, 0, 254);

    /* Doom separation is 0=left, 128=center, 254=right.  Preserve full
     * center gain and clamp the louder edge produced by the DMX formula. */
    left = volume * (254 - separation) / 127;
    right = volume * separation / 127;
    channel->left_volume = Clamp(left, 0, 127);
    channel->right_volume = Clamp(right, 0, 127);
}

static uint32_t ReadU32LE(const uint8_t *data)
{
    return (uint32_t) data[0]
         | ((uint32_t) data[1] << 8)
         | ((uint32_t) data[2] << 16)
         | ((uint32_t) data[3] << 24);
}

static boolean LoadChannelSound(mixer_channel_t *channel, int lumpnum)
{
    const uint8_t *lump;
    int lump_length;
    uint32_t declared_length;
    uint32_t sample_rate;

    if (lumpnum < 0)
    {
        return false;
    }

    lump_length = W_LumpLength((unsigned int) lumpnum);
    if (lump_length < (int) DMX_HEADER_BYTES)
    {
        return false;
    }

    lump = W_CacheLumpNum(lumpnum, PU_STATIC);
    if (lump[0] != 0x03 || lump[1] != 0x00)
    {
        return false;
    }

    sample_rate = (uint32_t) lump[2] | ((uint32_t) lump[3] << 8);
    declared_length = ReadU32LE(lump + 4);
    if (sample_rate == 0
     || declared_length <= 48
     || declared_length > (uint32_t) lump_length - DMX_HEADER_BYTES)
    {
        return false;
    }

    /* DMX ignores sixteen bytes at each end of its declared sample data. */
    channel->samples = lump + DMX_HEADER_BYTES + 16;
    channel->sample_count = declared_length - DMX_PADDING_BYTES;
    channel->sample_rate = sample_rate;
    channel->position = 0;
    channel->phase = 0;
    return true;
}

void I_InitSound(boolean prefix)
{
    int i;

    use_sfx_prefix = prefix;
    for (i = 0; i < MAX_MIX_CHANNELS; ++i)
    {
        channels[i].active = false;
    }
}

void I_ShutdownSound(void)
{
    int i;

    for (i = 0; i < MAX_MIX_CHANNELS; ++i)
    {
        channels[i].active = false;
    }
}

int I_GetSfxLumpNum(sfxinfo_t *sfxinfo)
{
    char name[9];
    int destination = 0;
    int source = 0;

    if (sfxinfo->link != NULL)
    {
        sfxinfo = sfxinfo->link;
    }

    if (use_sfx_prefix)
    {
        name[destination++] = 'D';
        name[destination++] = 'S';
    }

    while (destination < 8 && sfxinfo->name[source] != '\0')
    {
        name[destination++] = sfxinfo->name[source++];
    }
    name[destination] = '\0';

    return W_CheckNumForName(name);
}

void I_UpdateSound(void)
{
    /* The exported 60 Hz update calls doom_audio_pump exactly once. */
}

void I_UpdateSoundParams(int channel, int volume, int separation)
{
    if (channel >= 0 && channel < MAX_MIX_CHANNELS && channels[channel].active)
    {
        SetChannelParams(&channels[channel], volume, separation);
    }
}

int I_StartSound(sfxinfo_t *sfxinfo, int channel, int volume, int separation)
{
    mixer_channel_t *mixer_channel;

    if (channel < 0 || channel >= MAX_MIX_CHANNELS)
    {
        return -1;
    }

    mixer_channel = &channels[channel];
    mixer_channel->active = false;
    if (!LoadChannelSound(mixer_channel, sfxinfo->lumpnum))
    {
        return -1;
    }

    SetChannelParams(mixer_channel, volume, separation);
    mixer_channel->active = true;
    return channel;
}

void I_StopSound(int channel)
{
    if (channel >= 0 && channel < MAX_MIX_CHANNELS)
    {
        channels[channel].active = false;
    }
}

boolean I_SoundIsPlaying(int channel)
{
    return channel >= 0
        && channel < MAX_MIX_CHANNELS
        && channels[channel].active;
}

void I_PrecacheSounds(sfxinfo_t *sounds, int num_sounds)
{
    (void) sounds;
    (void) num_sounds;
}

void doom_audio_pump(void)
{
    uint32_t frame;

    for (frame = 0; frame < OUTPUT_FRAMES; ++frame)
    {
        int32_t left = 0;
        int32_t right = 0;
        int channel_index;

        for (channel_index = 0; channel_index < MAX_MIX_CHANNELS; ++channel_index)
        {
            mixer_channel_t *channel = &channels[channel_index];
            int32_t sample;

            if (!channel->active)
            {
                continue;
            }
            if (channel->position >= channel->sample_count)
            {
                channel->active = false;
                continue;
            }

            sample = ((int32_t) channel->samples[channel->position] - 128) * 256;
            left += sample * channel->left_volume / 127;
            right += sample * channel->right_volume / 127;

            channel->phase += channel->sample_rate;
            while (channel->phase >= OUTPUT_RATE)
            {
                channel->phase -= OUTPUT_RATE;
                ++channel->position;
            }
            if (channel->position >= channel->sample_count)
            {
                channel->active = false;
            }
        }

        output_buffer[frame * 2] = ClampSample(left);
        output_buffer[frame * 2 + 1] = ClampSample(right);
    }

    (void) host_audio_submit_wrapper(
        (uint32_t) (uintptr_t) output_buffer,
        OUTPUT_SAMPLES
    );
}

/* Music is deliberately unsupported by this backend. */
void I_InitMusic(void) {}
void I_ShutdownMusic(void) {}
void I_SetMusicVolume(int volume) { (void) volume; }
void I_PauseSong(void) {}
void I_ResumeSong(void) {}
void *I_RegisterSong(void *data, int len)
{
    (void) data;
    (void) len;
    return NULL;
}
void I_UnRegisterSong(void *handle) { (void) handle; }
void I_PlaySong(void *handle, boolean looping)
{
    (void) handle;
    (void) looping;
}
void I_StopSong(void) {}
boolean I_MusicIsPlaying(void) { return false; }

void I_BindSoundVariables(void) {}
