#include "d3d_gpu.h"

int d3d_gpu_present;
int d3d_gpu_use_spans;
int d3d_gpu_force_cpu_spans;
int d3d_gpu_force_affine_columns;
volatile d3d_gpu_perf_capture_t d3d_gpu_perf_latest;
volatile d3d_gpu_perf_capture_t d3d_gpu_perf_worst;

void d3d_gpu_init(void) { d3d_gpu_present = 0; }
void d3d_gpu_set_fb(uint8_t *fb, int stride) { (void)fb; (void)stride; }
void d3d_gpu_prepare_framebuffer_for_present(void) {}
void d3d_gpu_upload_palookup(const uint8_t *table, int shades) { (void)table; (void)shades; }
void d3d_gpu_flush(void) {}
void d3d_gpu_perf_capture_pending(void) {}
void d3d_gpu_perf_discard_interval(void) {}
void d3d_gpu_perf_dump(void) {}
void d3d_gpu_perf_note_cpu_fallback(void) {}
void d3d_gpu_perf_note_tile_load(uint32_t tile, uint32_t bytes, uint32_t us) { (void)tile; (void)bytes; (void)us; }
void d3d_gpu_perf_note_moveloop(uint32_t steps, uint32_t backlog, uint32_t remaining) { (void)steps; (void)backlog; (void)remaining; }
void d3d_gpu_perf_note_phase(int phase, uint32_t us) { (void)phase; (void)us; }
void d3d_gpu_perf_note_zone(int zone, uint32_t us) { (void)zone; (void)us; }
void d3d_gpu_perf_report_frame(uint32_t frame, uint32_t render, uint32_t page, uint32_t wait, uint32_t drain, uint32_t flip, uint32_t acquire, uint32_t audio) { (void)frame; (void)render; (void)page; (void)wait; (void)drain; (void)flip; (void)acquire; (void)audio; }
void d3d_gpu_drain_batch(void) {}
uint32_t d3d_gpu_flip_to(int idx) { (void)idx; return 0; }
void d3d_gpu_blit_mirror(uint8_t *dst, const uint8_t *src, int count, int rows, int stride) { (void)dst; (void)src; (void)count; (void)rows; (void)stride; }
void d3d_gpu_clear_rect_fb(uint8_t *dest, uint16_t w, uint16_t h, uint8_t color) { (void)dest; (void)w; (void)h; (void)color; }
void d3d_gpu_upload_transluc(const uint8_t *table, uint32_t size) { (void)table; (void)size; }
int d3d_gpu_translucent_spans_ready(void) { return 0; }
void d3d_gpu_tex_invalidate(void) {}
void d3d_gpu_mark_tex_dirty(void) {}
void d3d_gpu_drain(void) {}
void d3d_gpu_pre_cpu_fb_access(void) {}
void d3d_gpu_prepare_cpu_fb_write(void) {}
int d3d_gpu_shade_for(const uint8_t *palette) { (void)palette; return -1; }
int d3d_gpu_shade_slot_for(const uint8_t *palette, int *slot) { (void)palette; (void)slot; return -1; }
int d3d_gpu_try_vline1(uint8_t *dest, int count, const uint8_t *pal, int32_t pos, int32_t step, uint8_t shift, const uint8_t *texture, int32_t *out) { (void)dest; (void)count; (void)pal; (void)pos; (void)step; (void)shift; (void)texture; (void)out; return 0; }
int d3d_gpu_try_mvline1(uint8_t *dest, int count, const uint8_t *pal, int32_t pos, int32_t step, uint8_t shift, const uint8_t *texture, int32_t *out) { (void)dest; (void)count; (void)pal; (void)pos; (void)step; (void)shift; (void)texture; (void)out; return 0; }
int d3d_gpu_try_vline4(uint8_t *fb, int count, uint8_t shift) { (void)fb; (void)count; (void)shift; return 0; }
int d3d_gpu_try_mvline4(uint8_t *fb, int count, uint8_t shift) { (void)fb; (void)count; (void)shift; return 0; }
int d3d_gpu_try_hline(uint8_t *dest, int count, int shade, uint32_t i4, uint32_t i5, uint32_t a1, uint32_t a2, uint8_t bits, uint8_t shift, const uint8_t *texture) { (void)dest; (void)count; (void)shade; (void)i4; (void)i5; (void)a1; (void)a2; (void)bits; (void)shift; (void)texture; return 0; }
void d3d_gpu_vline(uint8_t *dest, int count, int shade, uint32_t pos, uint32_t step, uint8_t shift, const uint8_t *texture) { (void)dest; (void)count; (void)shade; (void)pos; (void)step; (void)shift; (void)texture; }
void d3d_gpu_mvline(uint8_t *dest, int count, int shade, uint32_t pos, uint32_t step, uint8_t shift, const uint8_t *texture) { (void)dest; (void)count; (void)shade; (void)pos; (void)step; (void)shift; (void)texture; }
void d3d_gpu_vline4(uint8_t *fb, int count, const int shade[4], const uint32_t pos[4], const uint32_t step[4], uint8_t shift, const uint8_t *const texture[4]) { (void)fb; (void)count; (void)shade; (void)pos; (void)step; (void)shift; (void)texture; }
void d3d_gpu_mvline4(uint8_t *fb, int count, const int shade[4], const uint32_t pos[4], const uint32_t step[4], uint8_t shift, const uint8_t *const texture[4]) { (void)fb; (void)count; (void)shade; (void)pos; (void)step; (void)shift; (void)texture; }
void d3d_gpu_tvline(uint8_t *dest, int count, int shade, uint32_t pos, uint32_t step, uint8_t shift, const uint8_t *texture, int reverse) { (void)dest; (void)count; (void)shade; (void)pos; (void)step; (void)shift; (void)texture; (void)reverse; }
void d3d_gpu_tvline2(uint8_t *dest, int count, int shade_a, int shade_b, int slot_a, int slot_b, uint32_t pos_a, uint32_t step_a, uint32_t pos_b, uint32_t step_b, uint8_t shift, const uint8_t *tex_a, const uint8_t *tex_b, int reverse) { (void)dest; (void)count; (void)shade_a; (void)shade_b; (void)slot_a; (void)slot_b; (void)pos_a; (void)step_a; (void)pos_b; (void)step_b; (void)shift; (void)tex_a; (void)tex_b; (void)reverse; }
void d3d_gpu_sprite_vline(uint8_t *dest, int count, int shade, uint32_t bx, uint32_t xs, uint32_t by, uint32_t ys, uint16_t height, const uint8_t *texture, int reverse) { (void)dest; (void)count; (void)shade; (void)bx; (void)xs; (void)by; (void)ys; (void)height; (void)texture; (void)reverse; }
int d3d_gpu_rhline(uint8_t *dest, int count, int shade, uint32_t bx, uint32_t xs, uint32_t by, uint32_t ys, uint16_t height, const uint8_t *texture) { (void)dest; (void)count; (void)shade; (void)bx; (void)xs; (void)by; (void)ys; (void)height; (void)texture; return 0; }
int d3d_gpu_rmhline(uint8_t *dest, int count, int shade, uint32_t bx, uint32_t xs, uint32_t by, uint32_t ys, uint16_t height, const uint8_t *texture) { (void)dest; (void)count; (void)shade; (void)bx; (void)xs; (void)by; (void)ys; (void)height; (void)texture; return 0; }
void d3d_gpu_record_rotsprite_setup(int32_t x, int32_t y, int32_t height) { (void)x; (void)y; (void)height; }
void d3d_gpu_mhline(uint8_t *dest, int count, int shade, uint32_t i2, uint32_t i5, uint32_t a1, uint32_t a2, uint8_t bits, uint8_t shift, const uint8_t *texture) { (void)dest; (void)count; (void)shade; (void)i2; (void)i5; (void)a1; (void)a2; (void)bits; (void)shift; (void)texture; }
void d3d_gpu_thline(uint8_t *dest, int count, int shade, uint32_t i2, uint32_t i5, uint32_t a1, uint32_t a2, uint8_t bits, uint8_t shift, const uint8_t *texture, int reverse) { (void)dest; (void)count; (void)shade; (void)i2; (void)i5; (void)a1; (void)a2; (void)bits; (void)shift; (void)texture; (void)reverse; }
void d3d_gpu_hline(uint8_t *dest, int count, int shade, uint32_t i4, uint32_t i5, uint32_t a1, uint32_t a2, uint8_t bits, uint8_t shift, const uint8_t *texture) { (void)dest; (void)count; (void)shade; (void)i4; (void)i5; (void)a1; (void)a2; (void)bits; (void)shift; (void)texture; }
