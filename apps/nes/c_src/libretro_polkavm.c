#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <libretro.h>
#include <vfs/vfs_implementation.h>

extern uint32_t host_present_frame_wrapper(const uint32_t *ptr, uint32_t width,
                                           uint32_t height, uint32_t stride);
extern uint32_t host_audio_submit_wrapper(const int16_t *ptr,
                                          uint32_t sample_count);
extern void host_log_wrapper(const char *ptr, uint32_t len);

#define FRAME_WIDTH 256u
#define FRAME_HEIGHT 240u
#define AUDIO_CHANNELS 2u

static const char content_path[] = "game/cartridge.nes";
static const char content_dir[] = "game";
static const char content_name[] = "cartridge";
static const char content_ext[] = "nes";
static struct retro_game_info_ext game_info_ext;
static uint32_t frame[FRAME_WIDTH * FRAME_HEIGHT];
static uint16_t controllers[2];

char *strtok(char *string, const char *delimiters) {
  static char *next;
  char *start;
  if (string != NULL)
    next = string;
  if (next == NULL)
    return NULL;
  while (*next != '\0' && strchr(delimiters, *next) != NULL)
    next++;
  if (*next == '\0') {
    next = NULL;
    return NULL;
  }
  start = next;
  while (*next != '\0' && strchr(delimiters, *next) == NULL)
    next++;
  if (*next != '\0')
    *next++ = '\0';
  else
    next = NULL;
  return start;
}

void *memchr(const void *memory, int byte, unsigned long size) {
  const unsigned char *cursor = (const unsigned char *)memory;
  for (unsigned long index = 0; index < size; index++) {
    if (cursor[index] == (unsigned char)byte)
      return (void *)(cursor + index);
  }
  return NULL;
}

double strtod(const char *string, char **end) {
  const char *cursor = string;
  if (*cursor == '+' || *cursor == '-')
    cursor++;
  while (*cursor >= '0' && *cursor <= '9')
    cursor++;
  if (*cursor == '.') {
    cursor++;
    while (*cursor >= '0' && *cursor <= '9')
      cursor++;
  }
  if ((*cursor == 'e' || *cursor == 'E') &&
      ((cursor[1] >= '0' && cursor[1] <= '9') ||
       ((cursor[1] == '+' || cursor[1] == '-') && cursor[2] >= '0' &&
        cursor[2] <= '9'))) {
    cursor++;
    if (*cursor == '+' || *cursor == '-')
      cursor++;
    while (*cursor >= '0' && *cursor <= '9')
      cursor++;
  }
  if (end != NULL)
    *end = (char *)cursor;
  return atof(string);
}
int retro_vfs_stat_impl(const char *path, int32_t *size) {
  (void)path;
  if (size != NULL)
    *size = 0;
  return 0;
}

int retro_vfs_stat_64_impl(const char *path, int64_t *size) {
  (void)path;
  if (size != NULL)
    *size = 0;
  return 0;
}

int retro_vfs_mkdir_impl(const char *directory) {
  (void)directory;
  return -1;
}

libretro_vfs_implementation_file *
retro_vfs_file_open_impl(const char *path, unsigned mode, unsigned hints) {
  (void)path;
  (void)mode;
  (void)hints;
  return NULL;
}

int retro_vfs_file_close_impl(libretro_vfs_implementation_file *stream) {
  (void)stream;
  return -1;
}

int retro_vfs_file_error_impl(libretro_vfs_implementation_file *stream) {
  (void)stream;
  return -1;
}

int64_t retro_vfs_file_size_impl(libretro_vfs_implementation_file *stream) {
  (void)stream;
  return -1;
}

int64_t retro_vfs_file_truncate_impl(libretro_vfs_implementation_file *stream,
                                     int64_t length) {
  (void)stream;
  (void)length;
  return -1;
}

int64_t retro_vfs_file_tell_impl(libretro_vfs_implementation_file *stream) {
  (void)stream;
  return -1;
}

int64_t retro_vfs_file_seek_impl(libretro_vfs_implementation_file *stream,
                                 int64_t offset, int seek_position) {
  (void)stream;
  (void)offset;
  (void)seek_position;
  return -1;
}

int64_t retro_vfs_file_read_impl(libretro_vfs_implementation_file *stream,
                                 void *output, uint64_t length) {
  (void)stream;
  (void)output;
  (void)length;
  return -1;
}

int64_t retro_vfs_file_write_impl(libretro_vfs_implementation_file *stream,
                                  const void *input, uint64_t length) {
  (void)stream;
  (void)input;
  (void)length;
  return -1;
}

int retro_vfs_file_flush_impl(libretro_vfs_implementation_file *stream) {
  (void)stream;
  return -1;
}

int retro_vfs_file_remove_impl(const char *path) {
  (void)path;
  return -1;
}

int retro_vfs_file_rename_impl(const char *old_path, const char *new_path) {
  (void)old_path;
  (void)new_path;
  return -1;
}

const char *
retro_vfs_file_get_path_impl(libretro_vfs_implementation_file *stream) {
  (void)stream;
  return NULL;
}

size_t fill_pathname_join(char *output, const char *directory, const char *path,
                          size_t capacity) {
  size_t directory_length = strlen(directory);
  size_t path_length = strlen(path);
  bool separator =
      directory_length != 0 && directory[directory_length - 1] != '/';
  size_t required = directory_length + (separator ? 1 : 0) + path_length;
  if (capacity == 0)
    return required;
  size_t offset = 0;
  while (offset < directory_length && offset + 1 < capacity) {
    output[offset] = directory[offset];
    offset++;
  }
  if (separator && offset + 1 < capacity)
    output[offset++] = '/';
  for (size_t index = 0; index < path_length && offset + 1 < capacity; index++)
    output[offset++] = path[index];
  output[offset] = '\0';
  return required;
}

static void log_text(const char *text) {
  host_log_wrapper(text, (uint32_t)strlen(text));
}

static bool environment_callback(unsigned command, void *data) {
  switch (command) {
  case RETRO_ENVIRONMENT_GET_GAME_INFO_EXT:
    *(const struct retro_game_info_ext **)data = &game_info_ext;
    return true;
  case RETRO_ENVIRONMENT_SET_PIXEL_FORMAT:
    return *(const enum retro_pixel_format *)data ==
           RETRO_PIXEL_FORMAT_XRGB8888;
  case RETRO_ENVIRONMENT_GET_CAN_DUPE:
    *(bool *)data = true;
    return true;
  case RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE:
    *(bool *)data = false;
    return true;
  case RETRO_ENVIRONMENT_GET_LANGUAGE:
    *(unsigned *)data = RETRO_LANGUAGE_ENGLISH;
    return true;
  case RETRO_ENVIRONMENT_GET_TARGET_SAMPLE_RATE:
    *(unsigned *)data = 48000u;
    return true;
  case RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS:
  case RETRO_ENVIRONMENT_SET_VARIABLES:
  case RETRO_ENVIRONMENT_SET_CONTROLLER_INFO:
  case RETRO_ENVIRONMENT_SET_MEMORY_MAPS:
  case RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS:
  case RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL:
  case RETRO_ENVIRONMENT_SET_GEOMETRY:
  case RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO:
    return true;
  default:
    return false;
  }
}

static void video_callback(const void *data, unsigned width, unsigned height,
                           size_t pitch) {
  if (data != NULL) {
    if (width != FRAME_WIDTH || height != FRAME_HEIGHT ||
        pitch < width * sizeof(uint32_t)) {
      log_text("nes: rejected invalid video frame\n");
      return;
    }
    const uint8_t *source = (const uint8_t *)data;
    for (unsigned y = 0; y < FRAME_HEIGHT; y++) {
      const uint32_t *row = (const uint32_t *)(source + y * pitch);
      for (unsigned x = 0; x < FRAME_WIDTH; x++)
        frame[y * FRAME_WIDTH + x] = row[x] | 0xff000000u;
    }
  }
  host_present_frame_wrapper(frame, FRAME_WIDTH, FRAME_HEIGHT,
                             FRAME_WIDTH * sizeof(uint32_t));
}

static size_t audio_callback(const int16_t *data, size_t frames) {
  if (frames == 0 || frames > UINT32_MAX / AUDIO_CHANNELS)
    return 0;
  return host_audio_submit_wrapper(data, (uint32_t)(frames * AUDIO_CHANNELS)) ==
                 0
             ? frames
             : 0;
}

static void input_poll_callback(void) {}

static int16_t input_state_callback(unsigned port, unsigned device,
                                    unsigned index, unsigned id) {
  (void)index;
  if (port >= 2 || device != RETRO_DEVICE_JOYPAD || id >= 16)
    return 0;
  return (controllers[port] & (1u << id)) != 0;
}

int nes_core_init(const uint8_t *rom, uint32_t rom_size) {
  struct retro_game_info info;
  memset(&game_info_ext, 0, sizeof(game_info_ext));
  game_info_ext.full_path = content_path;
  game_info_ext.dir = content_dir;
  game_info_ext.name = content_name;
  game_info_ext.ext = content_ext;
  game_info_ext.data = rom;
  game_info_ext.size = rom_size;
  game_info_ext.persistent_data = true;

  memset(&info, 0, sizeof(info));
  info.path = content_path;
  info.data = rom;
  info.size = rom_size;

  retro_set_environment(environment_callback);
  retro_set_video_refresh(video_callback);
  retro_set_audio_sample_batch(audio_callback);
  retro_set_input_poll(input_poll_callback);
  retro_set_input_state(input_state_callback);
  retro_init();
  retro_set_controller_port_device(0, RETRO_DEVICE_JOYPAD);
  retro_set_controller_port_device(1, RETRO_DEVICE_JOYPAD);
  if (!retro_load_game(&info)) {
    log_text("nes: FCEUmm rejected cartridge\n");
    return 0;
  }
  log_text("nes: cartridge loaded\n");
  return 1;
}

void nes_core_set_buttons(uint32_t player, uint16_t buttons) {
  if (player < 2)
    controllers[player] = buttons;
}

void nes_core_run_frame(void) { retro_run(); }

uint8_t *nes_core_save_ram(void) {
  return (uint8_t *)retro_get_memory_data(RETRO_MEMORY_SAVE_RAM);
}

uint32_t nes_core_save_ram_size(void) {
  size_t size = retro_get_memory_size(RETRO_MEMORY_SAVE_RAM);
  return size > UINT32_MAX ? 0 : (uint32_t)size;
}
